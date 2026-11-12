//! Responses/Compact characterization before extracting the unified core.
//! Client-visible preservation and withheld historical privilege are separate checks.
use axum::{body::Body, http::StatusCode, response::Response};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use std::ops::Range;

use super::{
    http_restoration::{
        admit_http_request, DlpForwardError, RestorationAdmission, CLIENT_PROCESSING_ERROR_CODE,
        CLIENT_PROCESSING_ERROR_MESSAGE,
    },
    response_restoration::ArgumentNormalizationMode,
    runtime::{CompiledCustomRule, SecurityRuntimeConfig},
    tokenization::{TokenCategory, TokenError, TokenFragment},
    types::{AuditAction, RiskLevel},
    unified_test_support::{
        completed_response, decode_json, prepare_attempt, restore_json, ResponsesFixture,
        TestResult, CONTACT, PRINCIPAL, PROVIDER, SESSION, TARGET,
    },
};
use crate::app_config::AppType;

#[tokio::test]
#[serial_test::serial]
async fn responses_current_request_restores_without_continuity_identity() -> TestResult {
    let fixture = ResponsesFixture::new();
    let mut body = json!({"input": format!("Synthetic contact {CONTACT}")});
    let original = body.clone();
    let attempt = fixture.prepare(&mut body, false, None)?;
    assert!(attempt.has_active_tokens(), "current_mapping_missing");
    let text = body["input"].as_str().ok_or("synthetic text missing")?;
    assert!(!text.contains(CONTACT), "current_input_not_redacted");
    let (status, delivered) = restore_json(attempt, &completed_response(text)).await?;
    assert_eq!(status, StatusCode::OK);
    assert!(
        delivered["output"][0]["content"][0]["text"] == original["input"],
        "current_output_changed"
    );
    let mut continuation =
        json!({"previous_response_id":"synthetic-unified-response", "input":"Synthetic continue"});
    let next = fixture.prepare(&mut continuation, false, None)?;
    assert!(!next.has_active_tokens(), "anonymous_continuity_granted");
    Ok(())
}

#[tokio::test]
#[serial_test::serial]
async fn missing_cursor_degrades_history_but_keeps_current_redaction_and_restoration() -> TestResult
{
    let fixture = ResponsesFixture::new();
    let mut body = json!({"previous_response_id":"synthetic-missing-cursor", "input":CONTACT});
    let attempt = fixture.prepare(&mut body, false, Some(SESSION))?;
    assert!(
        body["previous_response_id"] == "synthetic-missing-cursor",
        "cursor_changed"
    );
    assert!(
        attempt.has_active_tokens(),
        "current_mapping_dropped_with_cursor"
    );
    let text = body["input"].as_str().ok_or("synthetic text missing")?;
    assert!(!text.contains(CONTACT), "current_input_not_redacted");
    let (status, delivered) = restore_json(attempt, &completed_response(text)).await?;
    assert_eq!(status, StatusCode::OK);
    assert!(
        delivered["output"][0]["content"][0]["text"] == CONTACT,
        "current_output_not_restored"
    );
    Ok(())
}

#[test]
#[serial_test::serial]
fn mixed_media_missing_history_preserves_request_without_granting_authority() -> TestResult {
    let fixture = ResponsesFixture::new();
    let literal = format!("rredact-custom-{}-ai", "b".repeat(20));
    let mut body = json!({"previous_response_id":"synthetic-unknown-cursor", "input":[
        {"type":"function_call", "call_id":"synthetic-call", "name":"synthetic-tool", "arguments":"{}"},
        {"type":"function_call_output", "call_id":"synthetic-call", "output":[
            {"type":"input_image", "image_url":"data:image/png;base64,c3ludGhldGlj", "detail":"auto"},
            {"type":"input_text", "text":literal}
        ]},
        {"role":"user", "content":"Synthetic continue"}
    ], "unknown_extension":{"number":3, "enabled":true}});
    let original = body.clone();
    let attempt = fixture.prepare(&mut body, false, Some(SESSION))?;
    assert!(body == original, "degraded_native_request_changed");
    assert!(
        !attempt.has_active_tokens(),
        "missing_history_granted_privilege"
    );
    assert!(
        attempt.lease.resolve(&literal).is_err(),
        "literal_granted_mapping_authority"
    );
    assert_eq!(fixture.store.stats()?.entries, 0);
    Ok(())
}

#[tokio::test]
#[serial_test::serial]
async fn explicit_block_is_not_context_degradation_or_partial_redaction() -> TestResult {
    let config = SecurityRuntimeConfig {
        custom_rules: vec![CompiledCustomRule {
            id: "synthetic-unified-block".into(),
            name: "Synthetic block".into(),
            rule_type: "regex".into(),
            pattern: "synthetic-stop".into(),
            regex: Some(regex::Regex::new("synthetic-stop")?),
            risk_level: RiskLevel::High,
            action: AuditAction::Block,
            enabled: true,
        }],
        ..Default::default()
    };
    let fixture = ResponsesFixture::with_runtime(config);
    let mut body = json!({"previous_response_id":"synthetic-missing-cursor", "input":format!("{CONTACT} synthetic-stop")});
    let original = body.clone();
    let admission = admit_http_request(
        &fixture.gateway,
        &AppType::Codex,
        true,
        false,
        false,
        &mut body,
        &Default::default(),
    )?;
    let RestorationAdmission::Intercepted(response) = admission else {
        return Err("explicit block was bypassed".into());
    };
    let (status, error) = decode_json(response).await?;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(
        error["error"]["code"] == "dlp_blocked",
        "policy_block_code_changed"
    );
    assert!(
        error["error"]["message"] == "Request blocked by local security policy.",
        "policy_block_message_changed"
    );
    assert!(body == original, "blocked_request_partially_changed");
    assert_eq!(fixture.store.stats()?.entries, 0);
    Ok(())
}

#[test]
#[serial_test::serial]
fn fresh_leases_and_literal_syntax_do_not_supply_completion_authority() -> TestResult {
    let fixture = ResponsesFixture::new();
    let mut issuer = fixture
        .store
        .admit("codex", Some("synthetic-other-session"), 1)?;
    let reference = issuer.prepare(
        &[TokenFragment {
            raw: "synthetic-unified-private",
            whole_span_categories: &[TokenCategory::Custom],
        }],
        &[],
        |references| Ok(references[0].clone()),
    )?;
    for (session, revision) in [
        ("synthetic-current-session", 1),
        ("synthetic-other-session", 2),
    ] {
        let receiver = fixture.store.admit("codex", Some(session), revision)?;
        assert!(
            receiver.resolve(&reference).err() == Some(TokenError::ContextUnavailable),
            "reference_crossed_authority_boundary"
        );
    }
    let mut body = json!({"input":reference});
    let original = body.clone();
    let attempt = fixture.prepare(&mut body, false, Some(SESSION))?;
    assert!(body == original, "foreign_reference_literal_changed");
    assert!(
        !attempt.has_active_tokens(),
        "foreign_reference_granted_privilege"
    );
    assert!(
        attempt.lease.resolve(&reference).is_err(),
        "foreign_mapping_resolved"
    );
    Ok(())
}

#[tokio::test]
#[serial_test::serial]
async fn completed_reference_resumption_requires_matching_session_and_published_policy(
) -> TestResult {
    let fixture = ResponsesFixture::new();
    let revision = super::runtime::admit_security_runtime()?.restoration_policy_revision;
    let mut body = json!({"input":CONTACT});
    let attempt = fixture.prepare(&mut body, false, Some(SESSION))?;
    let reference = body["input"]
        .as_str()
        .ok_or("synthetic text missing")?
        .to_owned();
    let (status, delivered) = restore_json(attempt, &completed_response(&reference)).await?;
    assert_eq!(status, StatusCode::OK);
    assert!(
        delivered["output"][0]["content"][0]["text"] == CONTACT,
        "completed_control_output_changed"
    );
    for (session, expected) in [(SESSION, true), ("synthetic-other-session", false)] {
        let mut receiver =
            fixture
                .store
                .admit_published_policy("codex", Some(session), revision)?;
        assert!(
            receiver.resolve(&reference).is_err(),
            "fresh_receiver_has_ambient_authority"
        );
        let binding =
            receiver.bind_actual_attempt(PROVIDER, "openai_responses", TARGET, Some(PRINCIPAL))?;
        receiver.resume_matching_completed_tokens(&binding, &[&reference])?;
        assert!(
            receiver.resolve(&reference).is_ok() == expected,
            "completed_session_isolation_changed"
        );
    }
    super::runtime::set_security_runtime_for_test(SecurityRuntimeConfig {
        custom_rules: vec![CompiledCustomRule {
            id: "synthetic-unified-policy-change".into(),
            name: "Synthetic policy revision".into(),
            rule_type: "regex".into(),
            pattern: "synthetic-policy-marker".into(),
            regex: Some(regex::Regex::new("synthetic-policy-marker")?),
            risk_level: RiskLevel::High,
            action: AuditAction::Block,
            enabled: true,
        }],
        ..Default::default()
    });
    let changed_revision = super::runtime::admit_security_runtime()?.restoration_policy_revision;
    assert!(
        changed_revision > revision,
        "policy_control_did_not_advance"
    );
    let mut receiver =
        fixture
            .store
            .admit_published_policy("codex", Some(SESSION), changed_revision)?;
    let binding =
        receiver.bind_actual_attempt(PROVIDER, "openai_responses", TARGET, Some(PRINCIPAL))?;
    receiver.resume_matching_completed_tokens(&binding, &[&reference])?;
    assert!(
        receiver.resolve(&reference).err() == Some(TokenError::ContextUnavailable),
        "completed_reference_crossed_policy_revision"
    );
    let mut literal = json!({"input":reference});
    let original = literal.clone();
    let next = fixture.prepare(&mut literal, false, Some(SESSION))?;
    assert!(literal == original, "old_policy_literal_changed");
    assert!(
        !next.has_active_tokens(),
        "old_policy_literal_granted_privilege"
    );
    Ok(())
}

#[tokio::test]
#[serial_test::serial]
async fn unmodified_json_keeps_wire_bytes_unknown_fields_and_headers() -> TestResult {
    let fixture = ResponsesFixture::new();
    let mut body = json!({"input":"Synthetic ordinary input"});
    let attempt = fixture.prepare(&mut body, false, Some(SESSION))?;
    assert!(
        !attempt.has_active_tokens(),
        "ordinary_input_granted_privilege"
    );
    let raw = b"{\n  \"unknown_extension\": {\"number\": 3, \"enabled\": true},\n  \"output\": [], \"status\": \"completed\", \"object\": \"response\", \"id\": \"synthetic-wire\"\n}";
    let response = Response::builder()
        .header("content-type", "application/json")
        .header("etag", "synthetic-etag")
        .header("x-synthetic-extension", "retained")
        .body(Body::from(raw.as_slice()))?;
    let delivered = attempt
        .restore_client_http_response(response, ArgumentNormalizationMode::Exact, None)
        .await;
    assert_eq!(delivered.status(), StatusCode::OK);
    assert!(
        delivered.headers()["etag"] == "synthetic-etag",
        "unmodified_header_changed"
    );
    assert!(
        delivered.headers()["x-synthetic-extension"] == "retained",
        "unknown_header_dropped"
    );
    let bytes = delivered.into_body().collect().await?.to_bytes();
    assert!(bytes.as_ref() == raw, "unmodified_wire_bytes_changed");
    Ok(())
}

#[tokio::test]
#[serial_test::serial]
async fn compact_restores_readable_output_without_mutating_opaque_compaction() -> TestResult {
    let fixture = ResponsesFixture::new();
    let mut body = json!({"input":CONTACT});
    let attempt = fixture.prepare(&mut body, true, Some(SESSION))?;
    let text = body["input"].as_str().ok_or("synthetic text missing")?;
    let mut response = completed_response(text);
    response["object"] = json!("response.compaction");
    response["created_at"] = json!(1);
    response["output"].as_array_mut().ok_or("synthetic output missing")?.push(json!({
        "id":"synthetic-compaction", "type":"compaction", "encrypted_content":"synthetic-opaque", "unknown_extension":7
    }));
    let opaque = response["output"][1].clone();
    let (status, delivered) = restore_json(attempt, &response).await?;
    assert_eq!(status, StatusCode::OK);
    assert!(
        delivered["output"][0]["content"][0]["text"] == CONTACT,
        "compact_output_not_restored"
    );
    assert!(
        delivered["output"][1] == opaque,
        "opaque_compaction_changed"
    );
    Ok(())
}

#[test]
#[serial_test::serial]
fn retries_bind_each_actual_attempt_including_zero_token_requests() -> TestResult {
    let fixture = ResponsesFixture::new();
    for input in [CONTACT, "Synthetic ordinary input"] {
        let mut body = json!({"input":input});
        let request = fixture.admit(&mut body, false, Some(SESSION))?;
        let first = prepare_attempt(&request, &mut body, PROVIDER, TARGET)?;
        assert!(
            first
                .validate_final_send_binding(PROVIDER, "openai_responses", TARGET, Some(PRINCIPAL))
                .is_ok(),
            "actual_send_binding_rejected"
        );
        for (provider, protocol, target, principal) in [
            (
                "synthetic-failover-provider",
                "openai_responses",
                TARGET,
                Some(PRINCIPAL),
            ),
            (PROVIDER, "openai_chat", TARGET, Some(PRINCIPAL)),
            (
                PROVIDER,
                "openai_responses",
                "https://other.invalid/v1/responses",
                Some(PRINCIPAL),
            ),
            (
                PROVIDER,
                "openai_responses",
                TARGET,
                Some("synthetic-other-principal"),
            ),
        ] {
            assert!(
                first
                    .validate_final_send_binding(provider, protocol, target, principal)
                    .is_err(),
                "attempt_binding_relaxed"
            );
        }
        let retry = prepare_attempt(&request, &mut body, "synthetic-failover-provider", TARGET)?;
        assert!(
            retry
                .validate_final_send_binding(
                    "synthetic-failover-provider",
                    "openai_responses",
                    TARGET,
                    Some(PRINCIPAL)
                )
                .is_ok(),
            "retry_not_rebound"
        );
        assert!(
            retry
                .validate_final_send_binding(PROVIDER, "openai_responses", TARGET, Some(PRINCIPAL))
                .is_err(),
            "retry_inherited_previous_binding"
        );
    }
    Ok(())
}

#[tokio::test]
#[serial_test::serial]
async fn genuine_upstream_failure_bypasses_success_restoration_unchanged() -> TestResult {
    let fixture = ResponsesFixture::new();
    let mut body = json!({"input":CONTACT});
    let attempt = fixture.prepare(&mut body, false, Some(SESSION))?;
    let raw = b"{ \"error\": {\"message\": \"synthetic provider reason\", \"code\": \"model_not_found\", \"extension\": true} }";
    let response = Response::builder()
        .status(StatusCode::TOO_MANY_REQUESTS)
        .header("content-type", "application/json")
        .header("retry-after", "7")
        .body(Body::from(raw.as_slice()))?;
    let delivered = attempt
        .restore_client_http_response(response, ArgumentNormalizationMode::Exact, None)
        .await;
    assert_eq!(delivered.status(), StatusCode::TOO_MANY_REQUESTS);
    assert!(
        delivered.headers()["retry-after"] == "7",
        "upstream_retry_header_changed"
    );
    let bytes = delivered.into_body().collect().await?.to_bytes();
    assert!(bytes.as_ref() == raw, "genuine_upstream_error_changed");
    Ok(())
}

#[tokio::test]
#[serial_test::serial]
async fn converted_client_completion_is_not_original_upstream_proof() -> TestResult {
    let fixture = ResponsesFixture::new();
    let mut body = json!({"input":CONTACT});
    let request = fixture.admit(&mut body, false, Some(SESSION))?;
    let attempt = request
        .prepare_attempt(
            PROVIDER,
            "openai_chat",
            "https://synthetic.invalid/v1/chat/completions",
            Some(PRINCIPAL),
            &mut body,
            |candidate| Ok(json!({"messages":[{"role":"user", "content":candidate["input"]}]})),
        )
        .map_err(|_| "synthetic converted attempt failed")?;
    let text = body["messages"][0]["content"]
        .as_str()
        .ok_or("synthetic converted text missing")?;
    let (status, error) = restore_json(attempt, &completed_response(text)).await?;
    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert!(
        error["error"]["code"] == CLIENT_PROCESSING_ERROR_CODE,
        "converted_proof_error_changed"
    );
    assert!(
        error["error"]["message"] == CLIENT_PROCESSING_ERROR_MESSAGE,
        "converted_proof_diagnostic_exposed"
    );
    let mut next =
        json!({"previous_response_id":"synthetic-unified-response", "input":"Synthetic continue"});
    let attempt = fixture.prepare(&mut next, false, Some(SESSION))?;
    assert!(
        !attempt.has_active_tokens(),
        "converted_success_published_context"
    );
    Ok(())
}

#[tokio::test]
#[serial_test::serial]
async fn completed_history_requires_client_restoration_and_matching_provider_binding() -> TestResult
{
    let fixture = ResponsesFixture::new();
    let mut body = json!({"input":CONTACT});
    let attempt = fixture.prepare(&mut body, false, Some(SESSION))?;
    let reference = body["input"]
        .as_str()
        .ok_or("synthetic text missing")?
        .to_owned();
    let mut premature =
        json!({"previous_response_id":"synthetic-unified-response", "input":"Synthetic continue"});
    let before_restoration = fixture.prepare(&mut premature, false, Some(SESSION))?;
    assert!(
        !before_restoration.has_active_tokens(),
        "unfinished_attempt_published_privilege"
    );
    assert!(
        before_restoration.lease.resolve(&reference).is_err(),
        "unfinished_attempt_disclosed_mapping"
    );
    let (status, delivered) = restore_json(attempt, &completed_response(&reference)).await?;
    assert_eq!(status, StatusCode::OK);
    assert!(
        delivered["output"][0]["content"][0]["text"] == CONTACT,
        "delivered_output_changed"
    );
    for (provider, expected) in [(PROVIDER, true), ("synthetic-other-provider", false)] {
        let mut continuation = json!({"previous_response_id":"synthetic-unified-response", "input":"Synthetic continue"});
        let original = continuation.clone();
        let request = fixture.admit(&mut continuation, false, Some(SESSION))?;
        let next = prepare_attempt(&request, &mut continuation, provider, TARGET)?;
        assert!(continuation == original, "continuation_changed");
        assert!(
            next.has_active_tokens() == expected,
            "historical_privilege_binding_changed"
        );
        assert!(
            next.lease.resolve(&reference).is_ok() == expected,
            "historical_mapping_binding_changed"
        );
    }
    Ok(())
}

#[tokio::test]
#[serial_test::serial]
async fn local_failures_have_fixed_codex_projection_not_internal_diagnostics() -> TestResult {
    let _fixture = ResponsesFixture::new();
    for error in [
        DlpForwardError::ContextUnavailable,
        DlpForwardError::RestoreUnsupported,
        DlpForwardError::RestoreFailed,
        DlpForwardError::Unavailable,
    ] {
        let (status, body) = decode_json(error.into_codex_response()).await?;
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert!(
            body["error"]["code"] == CLIENT_PROCESSING_ERROR_CODE,
            "local_failure_code_changed"
        );
        assert!(
            body["error"]["message"] == CLIENT_PROCESSING_ERROR_MESSAGE,
            "internal_diagnostic_exposed"
        );
        assert!(
            body["error"]["param"] == Value::Null,
            "local_failure_param_changed"
        );
    }
    Ok(())
}

#[test]
fn c1_protocol_pair_keeps_client_and_actual_upstream_independent() -> TestResult {
    use super::restoration_contract::{ClientProtocol, ProtocolPair, UpstreamProtocol};
    use crate::proxy::protocol_bridge::Protocol;
    let pair = ProtocolPair::new(
        ClientProtocol::new(Protocol::Responses),
        UpstreamProtocol::from_format("chat_completions").ok_or("protocol alias missing")?,
    );
    assert!(
        pair.client().protocol() == Protocol::Responses,
        "client_identity_changed"
    );
    assert!(
        pair.upstream().protocol() == Protocol::Chat,
        "upstream_identity_changed"
    );
    assert!(
        UpstreamProtocol::from_format("synthetic-unknown").is_none(),
        "unknown_protocol_accepted"
    );
    Ok(())
}

#[test]
fn c1_registry_defers_responses_and_limits_native_cells_to_integrated_protocols() {
    use super::{
        protocol_adapters::{registration, Registration},
        restoration_contract::{
            ClientProtocol, Framing, Operation, ProtocolPair, UpstreamProtocol,
        },
    };
    use crate::proxy::protocol_bridge::Protocol;
    for client in [
        Protocol::Responses,
        Protocol::Chat,
        Protocol::Anthropic,
        Protocol::Gemini,
    ] {
        for upstream in [
            Protocol::Responses,
            Protocol::Chat,
            Protocol::Anthropic,
            Protocol::Gemini,
        ] {
            let pair =
                ProtocolPair::new(ClientProtocol::new(client), UpstreamProtocol::new(upstream));
            let operation = match client {
                Protocol::Responses => Operation::Responses,
                Protocol::Chat => Operation::ChatCompletions,
                Protocol::Anthropic => Operation::Messages,
                Protocol::Gemini => Operation::GenerateContent,
            };
            for framing in [Framing::Json, Framing::Sse] {
                let expected = if client == Protocol::Responses {
                    Registration::DeferToResponsesFacade
                } else if client == Protocol::Chat && upstream == Protocol::Chat {
                    Registration::NativeChat
                } else if client == Protocol::Anthropic && upstream == Protocol::Anthropic {
                    Registration::NativeAnthropic
                } else if client == Protocol::Gemini && upstream == Protocol::Gemini {
                    Registration::NativeGemini
                } else {
                    Registration::Disabled
                };
                assert!(
                    registration(pair, operation, framing) == expected,
                    "registration_granted_or_lost_support"
                );
            }
            assert!(
                registration(pair, Operation::ResponsesCompact, Framing::Sse)
                    == Registration::Unsupported,
                "compact_sse_registered"
            );
        }
    }
}

#[test]
fn c1_atomic_patch_plan_owns_budgeted_text_and_exact_native_locations() -> TestResult {
    use super::{
        payload_budget::PayloadBudget,
        restoration_contract::{EditConstraint, SourceClass, TextPatch, TextSites},
    };
    let budget = PayloadBudget::with_limit(64 * 1024);
    let native =
        json!({"content":"aéz", "args":{"label":"abc", "number":7}, "unknown":[true, null]});
    let original = native.clone();
    let mut sites = TextSites::new(budget.clone())?;
    sites.observe(
        &native["content"],
        SourceClass::CurrentInput,
        EditConstraint::Editable,
    )?;
    sites.observe(
        &native["args"]["label"],
        SourceClass::ToolArguments,
        EditConstraint::Editable,
    )?;
    assert!(
        sites.site(0).ok_or("site missing")?.text() == "aéz",
        "inspection_text_changed"
    );
    assert!(
        sites.site(1).ok_or("site missing")?.source() == SourceClass::ToolArguments,
        "source_class_changed"
    );
    let used_before = budget.used();
    let plan = sites.plan(&[
        TextPatch::new(sites.site(1).ok_or("site missing")?, 0..1, "q"),
        TextPatch::new(sites.site(0).ok_or("site missing")?, 1..3, "XYZ"),
        TextPatch::new(sites.site(1).ok_or("site missing")?, 2..3, "r"),
    ])?;
    assert!(native == original, "planning_mutated_native_payload");
    assert_eq!(plan.edits().len(), 2);
    assert!(
        plan.edits()
            .iter()
            .any(|edit| std::ptr::eq(edit.native(), &native["content"]) && edit.text() == "aXYZz"),
        "native_text_patch_wrong"
    );
    assert!(
        plan.edits()
            .iter()
            .any(|edit| std::ptr::eq(edit.native(), &native["args"]["label"])
                && edit.text() == "qbr"),
        "native_argument_leaf_patch_wrong"
    );
    assert!(budget.used() > used_before, "patch_ownership_not_charged");
    drop(plan);
    assert_eq!(budget.used(), used_before);
    drop(sites);
    assert_eq!(budget.used(), 0);
    Ok(())
}

#[test]
fn c1_patch_validation_rejects_foreign_owner_immutable_and_invalid_ranges() -> TestResult {
    use super::{
        payload_budget::PayloadBudget,
        restoration_contract::{ContractError, EditConstraint, SourceClass, TextPatch, TextSites},
    };
    let budget = PayloadBudget::with_limit(64 * 1024);
    let native = json!(["aéz", "synthetic-immutable"]);
    let original = native.clone();
    let mut sites = TextSites::new(budget.clone())?;
    sites.observe(
        &native[0],
        SourceClass::CurrentInput,
        EditConstraint::Editable,
    )?;
    sites.observe(
        &native[1],
        SourceClass::HistoricalText,
        EditConstraint::Immutable,
    )?;
    let mut foreign = TextSites::new(budget.clone())?;
    foreign.observe(
        &native[0],
        SourceClass::CurrentInput,
        EditConstraint::Editable,
    )?;
    let editable = sites.site(0).ok_or("site missing")?;
    let immutable = sites.site(1).ok_or("site missing")?;
    for (patch, expected) in [
        (
            TextPatch::new(foreign.site(0).ok_or("site missing")?, 0..1, "x"),
            ContractError::ForeignSite,
        ),
        (
            TextPatch::new(immutable, 0..1, "x"),
            ContractError::ImmutableSite,
        ),
        (
            TextPatch::new(editable, 1..2, "x"),
            ContractError::InvalidRange,
        ),
        (
            TextPatch::new(editable, 0..99, "x"),
            ContractError::InvalidRange,
        ),
        (
            TextPatch::new(editable, Range { start: 3, end: 1 }, "x"),
            ContractError::InvalidRange,
        ),
    ] {
        let before = budget.used();
        assert!(
            matches!(sites.plan(&[TextPatch::new(editable, 0..1, "y"), patch]), Err(error) if error == expected),
            "invalid_patch_set_accepted"
        );
        assert_eq!(budget.used(), before);
        assert!(native == original, "failed_plan_partially_mutated_native");
    }
    for ranges in [(0..3, 1..3), (0..1, 0..1)] {
        assert!(
            matches!(
                sites.plan(&[
                    TextPatch::new(editable, ranges.0, "x"),
                    TextPatch::new(editable, ranges.1, "y")
                ]),
                Err(ContractError::OverlappingPatches)
            ),
            "overlapping_or_duplicate_patch_accepted"
        );
    }
    Ok(())
}

#[test]
fn c1_observation_rejects_duplicate_nodes_and_non_text_without_leaking_budget() -> TestResult {
    use super::{
        payload_budget::PayloadBudget,
        restoration_contract::{ContractError, EditConstraint, SourceClass, TextSites},
    };
    let budget = PayloadBudget::with_limit(64 * 1024);
    let native = json!(["synthetic-text", 7]);
    let mut sites = TextSites::new(budget.clone())?;
    sites.observe(
        &native[0],
        SourceClass::ToolResult,
        EditConstraint::Editable,
    )?;
    let before = budget.used();
    assert!(
        sites.observe(
            &native[0],
            SourceClass::HistoricalText,
            EditConstraint::Editable
        ) == Err(ContractError::DuplicateSite),
        "duplicate_site_accepted"
    );
    assert!(
        sites.observe(
            &native[1],
            SourceClass::CurrentInput,
            EditConstraint::Editable
        ) == Err(ContractError::NonTextSite),
        "non_string_site_accepted"
    );
    assert_eq!(budget.used(), before);
    assert!(sites.site(1).is_none(), "invalid_site_published");
    Ok(())
}

#[test]
fn c1_atomic_patch_budget_failure_returns_no_plan_or_partial_native_edits() -> TestResult {
    use super::{
        payload_budget::PayloadBudget,
        restoration_contract::{ContractError, EditConstraint, SourceClass, TextPatch, TextSites},
    };
    let budget = PayloadBudget::with_limit(4096);
    let native = json!(["synthetic-first", "synthetic-second"]);
    let original = native.clone();
    let mut sites = TextSites::new(budget.clone())?;
    sites.observe(
        &native[0],
        SourceClass::CurrentInput,
        EditConstraint::Editable,
    )?;
    sites.observe(
        &native[1],
        SourceClass::ToolResult,
        EditConstraint::Editable,
    )?;
    let replacement = "x".repeat(4096);
    let before = budget.used();
    assert!(
        matches!(
            sites.plan(&[
                TextPatch::new(sites.site(0).ok_or("site missing")?, 0..1, "y"),
                TextPatch::new(sites.site(1).ok_or("site missing")?, 0..1, &replacement),
            ]),
            Err(ContractError::Resource(TokenError::CapacityExceeded))
        ),
        "over_budget_patch_set_accepted"
    );
    assert!(
        native == original,
        "budget_failure_partially_mutated_native"
    );
    assert_eq!(budget.used(), before);
    drop(sites);
    assert_eq!(budget.used(), 0);
    Ok(())
}

#[test]
fn c1_unchanged_text_proposals_leave_no_native_edits_to_serialize() -> TestResult {
    use super::{
        payload_budget::PayloadBudget,
        restoration_contract::{EditConstraint, SourceClass, TextPatch, TextSites},
    };
    let budget = PayloadBudget::with_limit(4096);
    let native = json!(["abc", ""]);
    let mut sites = TextSites::new(budget.clone())?;
    for value in native.as_array().ok_or("synthetic array missing")? {
        sites.observe(value, SourceClass::CurrentInput, EditConstraint::Editable)?;
    }
    let plan = sites.plan(&[
        TextPatch::new(sites.site(0).ok_or("site missing")?, 0..1, "a"),
        TextPatch::new(sites.site(1).ok_or("site missing")?, 0..0, ""),
    ])?;
    assert!(
        plan.edits().is_empty(),
        "unchanged_proposal_requires_native_reencoding"
    );
    Ok(())
}

#[test]
fn c1_empty_and_adjacent_edits_keep_distinct_equal_text_nodes() -> TestResult {
    use super::{
        payload_budget::PayloadBudget,
        restoration_contract::{EditConstraint, SourceClass, TextPatch, TextSites},
    };
    let budget = PayloadBudget::with_limit(16 * 1024);
    let native = json!(["abc", "abc", ""]);
    let mut sites = TextSites::new(budget.clone())?;
    for value in native.as_array().ok_or("synthetic array missing")? {
        sites.observe(value, SourceClass::CurrentInput, EditConstraint::Editable)?;
    }
    let empty = sites.plan(&[])?;
    assert!(empty.edits().is_empty(), "empty_plan_has_edits");
    drop(empty);
    let plan = sites.plan(&[
        TextPatch::new(sites.site(1).ok_or("site missing")?, 1..2, ""),
        TextPatch::new(sites.site(1).ok_or("site missing")?, 2..3, "z"),
        TextPatch::new(sites.site(1).ok_or("site missing")?, 3..3, "q"),
        TextPatch::new(sites.site(2).ok_or("site missing")?, 0..0, "x"),
    ])?;
    assert_eq!(plan.edits().len(), 2);
    assert!(
        plan.edits()
            .iter()
            .any(|edit| std::ptr::eq(edit.native(), &native[1]) && edit.text() == "azq"),
        "adjacent_range_result_changed"
    );
    assert!(
        plan.edits()
            .iter()
            .any(|edit| std::ptr::eq(edit.native(), &native[2]) && edit.text() == "x"),
        "empty_node_insertion_missing"
    );
    assert!(
        !plan
            .edits()
            .iter()
            .any(|edit| std::ptr::eq(edit.native(), &native[0])),
        "equal_text_node_conflated"
    );
    Ok(())
}

#[test]
fn c1_patch_allocation_failure_drops_already_staged_edits() -> TestResult {
    use super::{
        payload_budget::PayloadBudget,
        payload_text::test_support::{self, Action, Phase},
        restoration_contract::{ContractError, EditConstraint, SourceClass, TextPatch, TextSites},
    };
    use std::cell::Cell;
    let budget = PayloadBudget::with_limit(16 * 1024);
    let native = json!(["synthetic-first", "synthetic-second"]);
    let original = native.clone();
    let mut sites = TextSites::new(budget.clone())?;
    for value in native.as_array().ok_or("synthetic array missing")? {
        sites.observe(value, SourceClass::CurrentInput, EditConstraint::Editable)?;
    }
    let before = budget.used();
    let allocations = Cell::new(0);
    let hook = test_support::install(move |observation| {
        if observation.phase == Phase::BeforeAllocation {
            allocations.set(allocations.get() + 1);
            if allocations.get() == 2 {
                return Action::Fail;
            }
        }
        Action::Continue
    });
    assert!(
        matches!(
            sites.plan(&[
                TextPatch::new(sites.site(0).ok_or("site missing")?, 0..1, "x"),
                TextPatch::new(sites.site(1).ok_or("site missing")?, 0..1, "y"),
            ]),
            Err(ContractError::Resource(TokenError::Unavailable))
        ),
        "injected_allocation_failure_bypassed"
    );
    drop(hook);
    assert!(
        native == original,
        "allocation_failure_changed_native_payload"
    );
    assert_eq!(budget.used(), before);
    Ok(())
}

#[test]
fn c1_registry_rejects_mismatched_operations_and_defers_compact_json_only() {
    use super::{
        protocol_adapters::{registration, Registration},
        restoration_contract::{
            ClientProtocol, Framing, Operation, ProtocolPair, UpstreamProtocol,
        },
    };
    use crate::proxy::protocol_bridge::Protocol;
    for client in [
        Protocol::Responses,
        Protocol::Chat,
        Protocol::Anthropic,
        Protocol::Gemini,
    ] {
        let pair = ProtocolPair::new(
            ClientProtocol::new(client),
            UpstreamProtocol::new(Protocol::Responses),
        );
        for operation in [
            Operation::Responses,
            Operation::ResponsesCompact,
            Operation::ChatCompletions,
            Operation::Messages,
            Operation::GenerateContent,
        ] {
            let valid = matches!(
                (client, operation),
                (
                    Protocol::Responses,
                    Operation::Responses | Operation::ResponsesCompact
                ) | (Protocol::Chat, Operation::ChatCompletions)
                    | (Protocol::Anthropic, Operation::Messages)
                    | (Protocol::Gemini, Operation::GenerateContent)
            );
            if !valid {
                assert!(
                    registration(pair, operation, Framing::Json) == Registration::Unsupported,
                    "mismatched_operation_registered"
                );
            }
        }
        let expected = if client == Protocol::Responses {
            Registration::DeferToResponsesFacade
        } else {
            Registration::Unsupported
        };
        assert!(
            registration(pair, Operation::ResponsesCompact, Framing::Json) == expected,
            "compact_json_scope_changed"
        );
    }
}

#[test]
fn c1_plan_keeps_peak_charges_and_outlives_site_metadata() -> TestResult {
    use super::{
        payload_budget::PayloadBudget,
        payload_text::test_support::{self, Action, Phase},
        restoration_contract::{EditConstraint, NativeTextEdit, SourceClass, TextPatch, TextSites},
    };
    use std::{cell::Cell, rc::Rc};
    let budget = PayloadBudget::with_limit(16 * 1024);
    let native = json!(["abc", "def"]);
    let mut sites = TextSites::new(budget.clone())?;
    for value in native.as_array().ok_or("synthetic array missing")? {
        sites.observe(value, SourceClass::CurrentInput, EditConstraint::Editable)?;
    }
    let before = budget.used();
    let allocations = Rc::new(Cell::new(0));
    let seen = allocations.clone();
    let hook = test_support::install(move |observation| {
        if observation.phase == Phase::BeforeAllocation {
            seen.set(seen.get() + 1);
        }
        let ordered_charge = 4 * std::mem::size_of::<&TextPatch<'_, '_, '_>>();
        let staged_charge = if seen.get() < 2 {
            0
        } else {
            3 + 4 * std::mem::size_of::<NativeTextEdit<'_>>()
        };
        assert!(
            observation.used >= before + ordered_charge + staged_charge + 3,
            "peak_text_charge_missing"
        );
        Action::Continue
    });
    let plan = sites.plan(&[
        TextPatch::new(sites.site(0).ok_or("site missing")?, 0..1, "x"),
        TextPatch::new(sites.site(1).ok_or("site missing")?, 0..1, "y"),
    ])?;
    drop(hook);
    assert_eq!(allocations.get(), 2);
    drop(sites);
    assert!(budget.used() > 0, "plan_reservation_dropped_with_sites");
    assert!(
        plan.edits()
            .iter()
            .any(|edit| std::ptr::eq(edit.native(), &native[0]) && edit.text() == "xbc"),
        "plan_lost_owned_text"
    );
    drop(plan);
    assert_eq!(budget.used(), 0);
    Ok(())
}

#[test]
fn c1_foreign_site_rejection_keeps_independent_budgets_unchanged() -> TestResult {
    use super::{
        payload_budget::PayloadBudget,
        restoration_contract::{ContractError, EditConstraint, SourceClass, TextPatch, TextSites},
    };
    let local_budget = PayloadBudget::with_limit(4096);
    let foreign_budget = PayloadBudget::with_limit(4096);
    let native = json!("synthetic-node");
    let mut local = TextSites::new(local_budget.clone())?;
    let mut foreign = TextSites::new(foreign_budget.clone())?;
    local.observe(&native, SourceClass::CurrentInput, EditConstraint::Editable)?;
    foreign.observe(&native, SourceClass::CurrentInput, EditConstraint::Editable)?;
    let before = (local_budget.used(), foreign_budget.used());
    assert!(
        matches!(
            local.plan(&[
                TextPatch::new(local.site(0).ok_or("site missing")?, 0..1, "x"),
                TextPatch::new(foreign.site(0).ok_or("site missing")?, 1..2, "y"),
            ]),
            Err(ContractError::ForeignSite)
        ),
        "foreign_budget_owner_accepted"
    );
    assert_eq!((local_budget.used(), foreign_budget.used()), before);
    drop(local);
    drop(foreign);
    assert_eq!((local_budget.used(), foreign_budget.used()), (0, 0));
    Ok(())
}

#[test]
fn c1_allocation_and_copy_failures_release_staged_candidates() -> TestResult {
    use super::{
        payload_budget::PayloadBudget,
        payload_text::test_support::{self, Action, Phase},
        restoration_contract::{ContractError, EditConstraint, SourceClass, TextPatch, TextSites},
    };
    use std::cell::Cell;
    for failure_phase in [Phase::AfterAllocation, Phase::BeforeCopy] {
        let budget = PayloadBudget::with_limit(16 * 1024);
        let native = json!(["abc", "def"]);
        let original = native.clone();
        let mut sites = TextSites::new(budget.clone())?;
        for value in native.as_array().ok_or("synthetic array missing")? {
            sites.observe(value, SourceClass::CurrentInput, EditConstraint::Editable)?;
        }
        let before = budget.used();
        let allocations = Cell::new(0);
        let hook = test_support::install(move |observation| {
            if observation.phase == Phase::BeforeAllocation {
                allocations.set(allocations.get() + 1);
            }
            if allocations.get() == 2 && observation.phase == failure_phase {
                Action::Fail
            } else {
                Action::Continue
            }
        });
        assert!(
            matches!(
                sites.plan(&[
                    TextPatch::new(sites.site(0).ok_or("site missing")?, 0..1, "x"),
                    TextPatch::new(sites.site(1).ok_or("site missing")?, 0..1, "y"),
                ]),
                Err(ContractError::Resource(TokenError::Unavailable))
            ),
            "late_failure_returned_partial_plan"
        );
        drop(hook);
        assert!(native == original, "late_failure_mutated_native");
        assert_eq!(budget.used(), before);
    }
    Ok(())
}

#[test]
fn c1_site_and_patch_counts_have_explicit_work_limits() -> TestResult {
    use super::{
        payload_budget::PayloadBudget,
        restoration_contract::{ContractError, EditConstraint, SourceClass, TextPatch, TextSites},
    };
    let budget = PayloadBudget::with_limit(2 * 1024 * 1024);
    let native = Value::Array((0..4097).map(|_| Value::String("x".into())).collect());
    let values = native.as_array().ok_or("synthetic array missing")?;
    let mut sites = TextSites::new(budget.clone())?;
    for value in &values[..4096] {
        sites.observe(value, SourceClass::CurrentInput, EditConstraint::Editable)?;
    }
    let before = budget.used();
    assert!(
        sites.observe(
            &values[4096],
            SourceClass::CurrentInput,
            EditConstraint::Editable
        ) == Err(ContractError::Resource(TokenError::CapacityExceeded)),
        "site_work_limit_missing"
    );
    assert_eq!(budget.used(), before);
    assert!(sites.site(4096).is_none(), "site_over_limit_published");
    let site = sites.site(0).ok_or("site missing")?;
    let patches: Vec<_> = (0..4097).map(|_| TextPatch::new(site, 0..1, "y")).collect();
    assert!(
        matches!(
            sites.plan(&patches),
            Err(ContractError::Resource(TokenError::CapacityExceeded))
        ),
        "patch_work_limit_missing"
    );
    assert_eq!(budget.used(), before);
    Ok(())
}

#[tokio::test]
#[serial_test::serial]
async fn c2_real_store_separates_client_and_ingress_without_changing_responses() -> TestResult {
    use super::restoration_contract::{ClientProtocol, ContinuityScope, IngressNamespace};
    use crate::proxy::protocol_bridge::Protocol;
    let fixture = ResponsesFixture::new();
    let revision = super::runtime::admit_security_runtime()?.restoration_policy_revision;
    let mut body = json!({"input": CONTACT});
    let attempt = fixture.prepare(&mut body, false, Some(SESSION))?;
    let reference = body["input"]
        .as_str()
        .ok_or("synthetic text missing")?
        .to_owned();
    let (status, _) = restore_json(attempt, &completed_response(&reference)).await?;
    assert_eq!(status, StatusCode::OK);
    let mut legacy = fixture.store.admit_scoped_published_policy(
        ContinuityScope::LegacyResponses,
        "codex",
        Some(SESSION),
        revision,
    )?;
    let legacy_binding =
        legacy.bind_actual_attempt(PROVIDER, "openai_responses", TARGET, Some(PRINCIPAL))?;
    assert!(
        legacy.resolve(&reference).is_err(),
        "fresh_scope_has_ambient_authority"
    );
    legacy.resume_matching_completed_tokens(&legacy_binding, &[&reference])?;
    assert!(
        legacy.resolve(&reference).is_ok(),
        "legacy_scope_lost_existing_completion"
    );

    let mut namespaces = vec![legacy_binding.namespace];
    for protocol in [Protocol::Chat, Protocol::Anthropic, Protocol::Gemini] {
        for ingress in [IngressNamespace::Direct, IngressNamespace::Gateway] {
            let scope = ContinuityScope::Native {
                client: ClientProtocol::new(protocol),
                ingress,
            };
            let result = fixture.store.admit_scoped_published_policy(
                scope,
                "codex",
                Some(SESSION),
                revision,
            );
            assert!(result.is_ok(), "scoped_namespace_admission_missing");
            let mut receiver = result?;
            let binding = receiver.bind_actual_attempt(
                PROVIDER,
                "openai_responses",
                TARGET,
                Some(PRINCIPAL),
            )?;
            assert!(
                !namespaces.contains(&binding.namespace),
                "protocol_or_ingress_namespace_alias"
            );
            namespaces.push(binding.namespace);
            let repeated = fixture.store.admit_scoped_published_policy(
                scope,
                "codex",
                Some(SESSION),
                revision,
            )?;
            let repeated_binding = repeated.bind_actual_attempt(
                PROVIDER,
                "openai_responses",
                TARGET,
                Some(PRINCIPAL),
            )?;
            assert!(
                binding.namespace == repeated_binding.namespace,
                "stable_scoped_identity_lost"
            );
            let _ = receiver.resume_matching_completed_tokens(&binding, &[&reference]);
            assert!(
                receiver.resolve(&reference).is_err(),
                "legacy_completion_crossed_native_scope"
            );
        }
    }
    assert_eq!(fixture.store.stats()?.namespaces, 7);
    Ok(())
}

#[test]
#[serial_test::serial]
fn c2_scoped_current_request_needs_no_continuity_or_pairing() -> TestResult {
    use super::{
        payload_budget::PayloadBudget,
        restoration_contract::{ClientProtocol, ContinuityScope, IngressNamespace},
        tokenization::DlpTokenStore,
    };
    use crate::proxy::protocol_bridge::Protocol;
    let _fixture = ResponsesFixture::new();
    let revision = super::runtime::admit_security_runtime()?.restoration_policy_revision;
    let store = DlpTokenStore::new();
    for protocol in [Protocol::Chat, Protocol::Anthropic, Protocol::Gemini] {
        let scope = ContinuityScope::Native {
            client: ClientProtocol::new(protocol),
            ingress: IngressNamespace::Direct,
        };
        for session in [None, Some("synthetic invalid identity")] {
            let result = store.admit_scoped_published_policy(scope, "codex", session, revision);
            assert!(result.is_ok(), "anonymous_scoped_request_missing");
            let mut lease = result?;
            let reference = lease.prepare_with_budget(
                &[TokenFragment {
                    raw: CONTACT,
                    whole_span_categories: &[TokenCategory::Custom],
                }],
                &[],
                PayloadBudget::with_limit(1024 * 1024),
                |references| Ok(references[0].as_str().to_owned()),
            )?;
            assert!(
                lease.resolve(&reference).is_ok(),
                "scoped_current_mapping_missing"
            );
            let fresh = store.admit_scoped_published_policy(scope, "codex", session, revision)?;
            assert!(
                fresh.resolve(&reference).is_err(),
                "anonymous_scoped_continuity_granted"
            );
        }
    }
    Ok(())
}

#[test]
#[serial_test::serial]
fn c2_native_scope_cannot_issue_responses_completion_or_operation_authority() -> TestResult {
    use super::{
        payload_budget::PayloadBudget,
        restoration_contract::{ClientProtocol, ContinuityScope, IngressNamespace},
        tokenization::DlpTokenStore,
    };
    use crate::proxy::{protocol_bridge::Protocol, providers::CodexAdapter};
    let _fixture = ResponsesFixture::new();
    let revision = super::runtime::admit_security_runtime()?.restoration_policy_revision;
    let store = DlpTokenStore::new();
    let descriptor = CodexAdapter::resolve_dlp_operation_descriptor(
        "https://synthetic.invalid/v1",
        "/responses",
        "openai_responses",
        TARGET,
        false,
    )
    .ok_or("synthetic descriptor missing")?;
    for protocol in [Protocol::Chat, Protocol::Anthropic, Protocol::Gemini] {
        let scope = ContinuityScope::Native {
            client: ClientProtocol::new(protocol),
            ingress: IngressNamespace::Direct,
        };
        let mut lease =
            store.admit_scoped_published_policy(scope, "codex", Some(SESSION), revision)?;
        let reference = lease.prepare_with_budget(
            &[TokenFragment {
                raw: CONTACT,
                whole_span_categories: &[TokenCategory::Custom],
            }],
            &[],
            PayloadBudget::with_limit(1024 * 1024),
            |references| Ok(references[0].as_str().to_owned()),
        )?;
        let binding =
            lease.bind_actual_attempt(PROVIDER, "openai_responses", TARGET, Some(PRINCIPAL))?;
        let before = store.stats()?;
        // Call the existing low-level owner directly; no witness or adapter proof is fabricated.
        let result = lease.publish_completed_context_extracted(
            &binding,
            "synthetic-forbidden-response",
            &[&reference],
            super::completed_context::CompletedCallPublications {
                originals: &[],
                delivered: &[],
            },
            &[],
        );
        assert!(
            matches!(result, Err(TokenError::ContextUnavailable)),
            "native_scope_published_responses_evidence"
        );
        assert_eq!(store.stats()?, before);
        let operation = lease.bind_actual_operation_attempt(
            PROVIDER,
            "openai_responses",
            TARGET,
            Some(PRINCIPAL),
            Some(&descriptor),
        );
        assert!(
            matches!(operation, Err(TokenError::ContextUnavailable)),
            "native_scope_accepted_responses_operation"
        );
    }
    Ok(())
}

#[test]
#[serial_test::serial]
fn c2_native_scope_rejects_responses_claims_before_optional_empty_success() -> TestResult {
    use super::restoration_contract::{ClientProtocol, ContinuityScope, IngressNamespace};
    use crate::proxy::protocol_bridge::Protocol;
    let fixture = ResponsesFixture::new();
    let revision = super::runtime::admit_security_runtime()?.restoration_policy_revision;
    let mut lease = fixture.store.admit_scoped_published_policy(
        ContinuityScope::Native {
            client: ClientProtocol::new(Protocol::Gemini),
            ingress: IngressNamespace::Direct,
        },
        "codex",
        Some(SESSION),
        revision,
    )?;
    let binding =
        lease.bind_actual_attempt(PROVIDER, "openai_responses", TARGET, Some(PRINCIPAL))?;
    assert!(
        lease.resume_completed_context(&binding, None, &[], &[])
            == Err(TokenError::ContextUnavailable),
        "native_scope_accepted_responses_claim_api"
    );
    assert!(
        lease.verify_output_call_ids(&binding, None, &[]) == Err(TokenError::ContextUnavailable),
        "native_scope_accepted_responses_result_api"
    );
    let delivered = completed_response("Synthetic unchanged output");
    assert!(
        lease
            .capture_delivered_history(&binding, &delivered)
            .is_none(),
        "native_scope_captured_responses_history"
    );
    Ok(())
}

#[test]
#[serial_test::serial]
fn c2_core_request_owner_forks_only_current_authority_and_retains_budget() -> TestResult {
    use super::{
        payload_budget::PayloadBudget, restoration_core::RequestAuthority,
        tokenization::DlpTokenStore,
    };
    let _fixture = ResponsesFixture::new();
    let revision = super::runtime::admit_security_runtime()?.restoration_policy_revision;
    let store = DlpTokenStore::new();
    let budget = PayloadBudget::with_limit(1024 * 1024);
    let result = RequestAuthority::admit_responses(&store, "codex", None, revision, budget.clone());
    assert!(result.is_ok(), "core_request_owner_missing");
    let mut owner = result?;
    let owner_budget = owner.budget();
    let reference = owner.lease_mut_for_responses().prepare_with_budget(
        &[TokenFragment {
            raw: CONTACT,
            whole_span_categories: &[TokenCategory::Custom],
        }],
        &[],
        owner_budget,
        |references| Ok(references[0].as_str().to_owned()),
    )?;
    let (first, first_binding) =
        owner.fork_bound_attempt(PROVIDER, "openai_responses", TARGET, Some(PRINCIPAL), None)?;
    assert!(
        first.resolve(&reference).is_ok(),
        "core_attempt_lost_current_authority"
    );
    let (second, second_binding) = owner.fork_bound_attempt(
        "synthetic-other-provider",
        "openai_responses",
        TARGET,
        Some(PRINCIPAL),
        None,
    )?;
    assert!(
        second.resolve(&reference).is_ok(),
        "core_retry_lost_current_authority"
    );
    assert!(
        first_binding.provider_fingerprint != second_binding.provider_fingerprint,
        "core_retry_reused_provider_binding"
    );
    let empty = RequestAuthority::admit_responses(&store, "codex", None, revision, budget.clone())?;
    assert!(
        empty.lease().resolve(&reference).is_err(),
        "core_owner_granted_fresh_lease_authority"
    );
    let held = owner.budget().reserve(31)?;
    assert_eq!(budget.used(), 31);
    drop(owner);
    assert_eq!(budget.used(), 31);
    drop(held);
    assert_eq!(budget.used(), 0);
    assert!(
        first.resolve(&reference).is_ok(),
        "core_attempt_did_not_own_lease"
    );
    Ok(())
}

#[test]
#[serial_test::serial]
fn c2_native_scope_cannot_use_responses_request_preparation() -> TestResult {
    use super::{
        payload_budget::PayloadBudget,
        protocol_adapters::responses,
        request_plan::PlanError,
        restoration_contract::{ClientProtocol, ContinuityScope, IngressNamespace},
    };
    use crate::proxy::protocol_bridge::Protocol;
    let fixture = ResponsesFixture::new();
    let snapshot = super::runtime::admit_security_runtime()?;
    for protocol in [Protocol::Chat, Protocol::Anthropic, Protocol::Gemini] {
        let mut lease = fixture.store.admit_scoped_published_policy(
            ContinuityScope::Native {
                client: ClientProtocol::new(protocol),
                ingress: IngressNamespace::Direct,
            },
            "codex",
            None,
            snapshot.restoration_policy_revision,
        )?;
        let budget = PayloadBudget::with_limit(1024 * 1024);
        let body = json!({"input":"Synthetic unchanged input"});
        let mut validated = false;
        let result = responses::prepare_request(
            &mut lease,
            &body,
            &snapshot.runtime,
            &[],
            &[],
            budget.clone(),
            |_, _, _| {
                validated = true;
                Ok(())
            },
        );
        assert!(
            matches!(
                result,
                Err(PlanError::Token(TokenError::ContextUnavailable))
            ),
            "native_scope_accepted_responses_preparation"
        );
        assert!(!validated, "native_scope_reached_responses_validation");
        assert!(lease.is_empty(), "native_scope_gained_responses_authority");
        assert_eq!(budget.used(), 0);
    }
    Ok(())
}

/// Drive the real response verifier and consume delivery; never mint a completion witness.
async fn c2_deliver_synthetic_call(fixture: &ResponsesFixture) -> TestResult<(String, Value)> {
    let mut source = json!({"input":CONTACT});
    let attempt = fixture.prepare(&mut source, false, Some(SESSION))?;
    let reference = source["input"]
        .as_str()
        .ok_or("synthetic reference missing")?
        .to_owned();
    let call = json!({"id":"synthetic-c2-call-item", "type":"function_call", "status":"completed",
        "call_id":"synthetic-c2-call", "name":"synthetic_tool", "arguments":"{}"});
    let response = json!({"id":"synthetic-c2-completed-call", "object":"response", "status":"completed", "output":[call]});
    let (status, _) = restore_json(attempt, &response).await?;
    assert_eq!(status, StatusCode::OK);
    Ok((reference, call))
}

#[tokio::test]
#[serial_test::serial]
async fn c2_independent_completed_reference_survives_unwitnessed_pair_without_ambient_authority(
) -> TestResult {
    let fixture = ResponsesFixture::new();
    let mut source = json!({"input":CONTACT});
    let source_attempt = fixture.prepare(&mut source, false, Some(SESSION))?;
    let reference = source["input"]
        .as_str()
        .ok_or("synthetic reference missing")?
        .to_owned();
    let (status, _) = restore_json(source_attempt, &completed_response(&reference)).await?;
    assert_eq!(status, StatusCode::OK);
    let revision = super::runtime::admit_security_runtime()?.restoration_policy_revision;
    let mut uncompleted = fixture
        .store
        .admit_published_policy("codex", Some(SESSION), revision)?;
    let other = uncompleted.prepare(
        &[TokenFragment {
            raw: "Synthetic uncompleted dependency",
            whole_span_categories: &[TokenCategory::Custom],
        }],
        &[],
        |refs| Ok(refs[0].clone()),
    )?;
    let literal = format!("rredact-custom-{}-ai", "c".repeat(20));
    for case in 0..3 {
        let text = format!("{reference} {other} {literal}");
        let mut body = if case == 0 {
            json!({"input":[{"role":"user", "content":text}]})
        } else {
            json!({"previous_response_id":"synthetic-c2-missing-context", "input":[
                {"type":"function_call", "call_id":"synthetic-c2-unwitnessed-call", "name":"synthetic_tool", "arguments":"{}"},
                {"type":"function_call_output", "call_id":"synthetic-c2-unwitnessed-call", "output":text}
            ]})
        };
        let original = body.clone();
        let request = fixture.admit(&mut body, false, Some(SESSION))?;
        let attempt = prepare_attempt(
            &request,
            &mut body,
            if case == 2 {
                "synthetic-c2-other-provider"
            } else {
                PROVIDER
            },
            TARGET,
        )?;
        assert!(body == original, "independent_reference_request_changed");
        assert!(
            attempt.lease.resolve(&reference).is_ok() == (case != 2),
            "independent_reference_binding_changed"
        );
        assert!(
            attempt.lease.resolve(&other).is_err(),
            "uncompleted_mapping_imported"
        );
        assert!(
            attempt.lease.resolve(&literal).is_err(),
            "literal_mapping_imported"
        );
        if case == 1 {
            let mut response = completed_response(&text);
            response["id"] = json!("synthetic-c2-independent-delivery");
            let (status, delivered) = restore_json(attempt, &response).await?;
            assert_eq!(status, StatusCode::OK);
            let text = delivered["output"][0]["content"][0]["text"]
                .as_str()
                .ok_or("synthetic output missing")?;
            assert!(text.contains(CONTACT), "completed_reference_not_restored");
            assert!(
                text.contains(&other) && text.contains(&literal),
                "unproven_literal_changed"
            );
        }
    }
    Ok(())
}

#[tokio::test]
#[serial_test::serial]
async fn c2_first_result_pairing_is_not_call_snapshot_or_result_body_authority() -> TestResult {
    use super::completed_context::CompletedCallSpec;
    let fixture = ResponsesFixture::new();
    let (reference, call) = c2_deliver_synthetic_call(&fixture).await?;
    let revision = super::runtime::admit_security_runtime()?.restoration_policy_revision;
    let mut receiver = fixture
        .store
        .admit_published_policy("codex", Some(SESSION), revision)?;
    let binding =
        receiver.bind_actual_attempt(PROVIDER, "openai_responses", TARGET, Some(PRINCIPAL))?;
    receiver.verify_output_call_ids(
        &binding,
        Some("synthetic-c2-completed-call"),
        &["synthetic-c2-call"],
    )?;
    assert!(receiver.is_empty(), "call_id_verification_granted_mapping");
    assert!(
        receiver.resolve(&reference).is_err(),
        "pairing_granted_call_lineage"
    );
    receiver.resume_completed_context(
        &binding,
        None,
        &[CompletedCallSpec {
            call_id: "synthetic-c2-call",
            name: "synthetic_tool",
            item_type: "function_call",
            payload: "{}",
            reasoning: None,
            encrypted_reasoning: None,
        }],
        &[],
    )?;
    assert!(
        receiver.resolve(&reference).is_ok(),
        "exact_call_snapshot_lost_lineage"
    );
    let mut uncompleted = fixture
        .store
        .admit_published_policy("codex", Some(SESSION), revision)?;
    let other = uncompleted.prepare(
        &[TokenFragment {
            raw: "Synthetic uncompleted result dependency",
            whole_span_categories: &[TokenCategory::Custom],
        }],
        &[],
        |refs| Ok(refs[0].clone()),
    )?;
    let literal = format!("rredact-custom-{}-ai", "d".repeat(20));
    for case in 0..4 {
        let mut replayed = call.clone();
        if case == 1 {
            replayed["arguments"] = json!("{\"changed\":true}");
        }
        if case == 2 {
            replayed["name"] = json!("synthetic_other_tool");
        }
        let mut body = json!({"input":[replayed,
            {"type":"function_call_output", "call_id":"synthetic-c2-call",
                "output":format!("Synthetic first unseen result {other} {literal}")}
        ]});
        let original = body.clone();
        let request = fixture.admit(&mut body, false, Some(SESSION))?;
        let attempt = prepare_attempt(
            &request,
            &mut body,
            if case == 3 {
                "synthetic-c2-other-provider"
            } else {
                PROVIDER
            },
            TARGET,
        )?;
        assert!(
            body == original,
            "first_result_body_required_prior_snapshot"
        );
        assert!(
            attempt.lease.resolve(&reference).is_ok() == (case == 0),
            "call_snapshot_authority_changed"
        );
        assert!(
            attempt.lease.resolve(&other).is_err(),
            "first_result_imported_uncompleted_mapping"
        );
        assert!(
            attempt.lease.resolve(&literal).is_err(),
            "first_result_literal_granted_mapping"
        );
    }
    Ok(())
}

#[tokio::test]
#[serial_test::serial]
async fn c2_first_and_replayed_result_bodies_are_inspected_without_publication() -> TestResult {
    let fixture = ResponsesFixture::new();
    let (prior, call) = c2_deliver_synthetic_call(&fixture).await?;
    let mut uncompleted_result_reference: Option<String> = None;
    for (case, current) in ["13800000000", "13700000000", "13600000000"]
        .into_iter()
        .enumerate()
    {
        let mut current_call = call.clone();
        if case == 2 {
            current_call["arguments"] = json!("{\"changed\":true}");
        }
        let mut body = json!({"input":[current_call,
            {"type":"function_call_output", "call_id":"synthetic-c2-call",
                "output":format!("Synthetic result {current} {}", uncompleted_result_reference.as_deref().unwrap_or(""))}
        ]});
        let attempt = fixture.prepare(&mut body, false, Some(SESSION))?;
        let text = body["input"][1]["output"]
            .as_str()
            .ok_or("synthetic result missing")?;
        assert!(
            !text.contains(current),
            "first_or_replayed_result_not_inspected"
        );
        let mut references = Vec::new();
        super::output_text_matching::visit_complete_references(text, |range| {
            references.push(text[range].to_owned());
            Ok(())
        })?;
        assert!(
            references
                .iter()
                .any(|reference| attempt.lease.resolve(reference).is_ok()),
            "result_current_mapping_missing"
        );
        if let Some(reference) = &uncompleted_result_reference {
            assert!(
                attempt.lease.resolve(reference).is_err(),
                "uncompleted_result_was_attested"
            );
        }
        assert!(
            attempt.lease.resolve(&prior).is_ok() == (case != 2),
            "result_pairing_replaced_snapshot_proof"
        );
        if case == 0 {
            uncompleted_result_reference = references
                .into_iter()
                .find(|reference| attempt.lease.resolve(reference).is_ok());
        }
        // Admission/preparation alone must never publish completed result authority.
        drop(attempt);
    }
    Ok(())
}

#[tokio::test]
#[serial_test::serial]
async fn c2_first_result_explicit_block_cannot_degrade_or_partially_mutate() -> TestResult {
    let config = SecurityRuntimeConfig {
        custom_rules: vec![CompiledCustomRule {
            id: "synthetic-c2-result-block".into(),
            name: "Synthetic result block".into(),
            rule_type: "regex".into(),
            pattern: "synthetic-stop".into(),
            regex: Some(regex::Regex::new("synthetic-stop")?),
            risk_level: RiskLevel::High,
            action: AuditAction::Block,
            enabled: true,
        }],
        ..Default::default()
    };
    let fixture = ResponsesFixture::with_runtime(config);
    let (_, call) = c2_deliver_synthetic_call(&fixture).await?;
    let baseline = fixture.store.stats()?.entries;
    for changed in [false, true] {
        let mut replayed = call.clone();
        if changed {
            replayed["arguments"] = json!("{\"changed\":true}");
        }
        let mut body = json!({"input":[replayed,
            {"type":"function_call_output", "call_id":"synthetic-c2-call",
                "output":"13800000000 synthetic-stop"}
        ]});
        let original = body.clone();
        let transformed = std::cell::Cell::new(false);
        let mut headers = axum::http::HeaderMap::new();
        headers.insert("session-id", SESSION.parse()?);
        let response = match admit_http_request(
            &fixture.gateway,
            &AppType::Codex,
            true,
            false,
            false,
            &mut body,
            &headers,
        )? {
            RestorationAdmission::Intercepted(response) => response,
            RestorationAdmission::Admitted(request) => {
                let result = request.prepare_attempt(
                    PROVIDER,
                    "openai_responses",
                    TARGET,
                    Some(PRINCIPAL),
                    &mut body,
                    |candidate| {
                        transformed.set(true);
                        Ok(candidate.clone())
                    },
                );
                match result {
                    Err(error) => error.into_app_response(&AppType::Codex),
                    Ok(_) => return Err("first_result_block_bypassed".into()),
                }
            }
            RestorationAdmission::Passthrough => {
                return Err("first_result_block_passthrough".into())
            }
        };
        let (status, error) = decode_json(response).await?;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(
            error["error"]["code"] == "dlp_blocked",
            "result_block_degraded"
        );
        assert!(!transformed.get(), "blocked_result_reached_transform");
        assert!(body == original, "blocked_result_partially_mutated");
        assert_eq!(fixture.store.stats()?.entries, baseline);
    }
    Ok(())
}

// Exercises the real final-client wrapper, never a boolean completion constructor.
async fn c3_sse_provenance_case(mode: &str, input: &str) -> TestResult {
    let fixture = ResponsesFixture::new();
    let mut body = json!({"input":input,"stream":true});
    let request = fixture.admit(&mut body, false, Some(SESSION))?;
    let protocol = if mode == "wrong_protocol" {
        "openai_chat"
    } else {
        "anthropic"
    };
    let target = "https://synthetic.invalid/v1/messages";
    let prepare = |body: &mut Value, protocol: &str| {
        request
            .prepare_attempt(
                PROVIDER,
                protocol,
                target,
                Some(PRINCIPAL),
                body,
                |candidate| Ok(candidate.clone()),
            )
            .map_err(|_| "synthetic_attempt_preparation_failed")
    };
    let attempt = prepare(&mut body, protocol)?;
    let token = body["input"]
        .as_str()
        .ok_or("synthetic_carrier_missing")?
        .to_owned();
    let evidence = match mode {
        "missing" => None,
        "same_attempt" | "interrupted" => attempt.upstream_sse_evidence_tracker(),
        "wrong_protocol" | "other_attempt" => {
            let donor = prepare(&mut body, "anthropic")?;
            donor.upstream_sse_evidence_tracker()
        }
        _ => return Err("synthetic_mode_unknown".into()),
    };
    if let Some(tracker) = &evidence {
        let raw = json!({"id":"synthetic-c3-original","type":"message","role":"assistant",
            "model":"synthetic-model","content":[{"type":"tool_use","id":"synthetic-c3-call",
            "name":"synthetic-tool","input":{"value":token}}],"stop_reason":"tool_use",
            "usage":{"input_tokens":1,"output_tokens":1}})
        .to_string();
        let mut tracker = tracker.lock().map_err(|_| "synthetic_tracker_poisoned")?;
        if mode == "interrupted" {
            tracker.observe(&super::completion_evidence::CompletionObservation::IoFailure);
        }
        tracker.observe_fully_read_anthropic_json(&raw);
    }
    let item = json!({"id":"synthetic-c3-item","type":"function_call","call_id":"synthetic-c3-call",
        "name":"synthetic-tool","arguments":json!({"value":token}).to_string(),"status":"completed"});
    let wire = format!(
        "event: response.output_item.done\ndata: {}\n\nevent: response.completed\ndata: {}\n\n",
        json!({"type":"response.output_item.done","output_index":0,"item":item}),
        json!({"type":"response.completed","response":{"id":"synthetic-c3-response","object":"response",
            "status":"completed","output":[item]}})
    );
    let response = Response::builder()
        .header("content-type", "text/event-stream")
        .body(Body::from(wire))?;
    let delivered = attempt
        .restore_client_http_response(response, ArgumentNormalizationMode::Exact, evidence)
        .await;
    let bytes = delivered.into_body().collect().await?.to_bytes();
    let text = std::str::from_utf8(&bytes)?;
    let events: Vec<Value> = text
        .lines()
        .filter_map(|line| line.strip_prefix("data: "))
        .filter(|data| *data != "[DONE]")
        .map(serde_json::from_str)
        .collect::<Result<_, _>>()?;
    let success = mode == "same_attempt";
    let tool_released = events.iter().any(|event| {
        event["type"] == "response.output_item.done" && event["item"]["type"] == "function_call"
    });
    assert!(
        tool_released == success,
        "original_evidence_tool_release_boundary"
    );
    assert!(
        events
            .iter()
            .any(|event| event["type"] == "response.completed")
            == success,
        "original_evidence_completion_boundary"
    );
    assert!(
        events
            .iter()
            .any(|event| event["type"] == "response.failed")
            != success,
        "original_evidence_failure_boundary"
    );
    assert!(
        !text.contains("rredact-cn_phone-"),
        "withheld_reference_released"
    );
    if input == CONTACT {
        assert!(
            text.contains(CONTACT) == success,
            "privileged_partial_release"
        );
    }
    let mut continuation =
        json!({"previous_response_id":"synthetic-c3-response","input":"Synthetic continue"});
    let next = fixture.admit(&mut continuation, false, Some(SESSION))?;
    let next = next
        .prepare_attempt(
            PROVIDER,
            protocol,
            target,
            Some(PRINCIPAL),
            &mut continuation,
            |candidate| Ok(candidate.clone()),
        )
        .map_err(|_| "synthetic_followup_preparation_failed")?;
    assert!(
        next.has_active_tokens() == (success && input == CONTACT),
        "invalid_completion_published_context"
    );
    Ok(())
}

#[tokio::test]
#[serial_test::serial]
async fn c3_converted_sse_requires_original_evidence_including_zero_token_attempts() -> TestResult {
    for input in [CONTACT, "Synthetic ordinary input"] {
        c3_sse_provenance_case("missing", input).await?;
    }
    Ok(())
}

#[tokio::test]
#[serial_test::serial]
async fn c3_converted_sse_rejects_wrong_original_protocol_evidence() -> TestResult {
    for input in [CONTACT, "Synthetic ordinary input"] {
        c3_sse_provenance_case("wrong_protocol", input).await?;
    }
    Ok(())
}

#[tokio::test]
#[serial_test::serial]
async fn c3_converted_sse_rejects_same_binding_other_actual_attempt_evidence() -> TestResult {
    for input in [CONTACT, "Synthetic ordinary input"] {
        c3_sse_provenance_case("other_attempt", input).await?;
    }
    Ok(())
}

#[tokio::test]
#[serial_test::serial]
async fn c3_converted_sse_accepts_only_its_completed_original_evidence() -> TestResult {
    for input in [CONTACT, "Synthetic ordinary input"] {
        c3_sse_provenance_case("same_attempt", input).await?;
        c3_sse_provenance_case("interrupted", input).await?;
    }
    Ok(())
}

#[test]
#[serial_test::serial]
fn c3_provenance_capacity_failure_does_not_commit_transformed_request() -> TestResult {
    let fixture = ResponsesFixture::new();
    let mut body = json!({"input":"Synthetic original input","unknown_extension":true});
    let original = body.clone();
    let request = fixture.admit(&mut body, false, Some(SESSION))?;
    let fault = super::restoration_core::provenance_fault::reject();
    let result = request.prepare_attempt(
        PROVIDER,
        "openai_responses",
        TARGET,
        Some(PRINCIPAL),
        &mut body,
        |candidate| {
            let mut transformed = candidate.clone();
            transformed["unknown_extension"] = json!(false);
            Ok(transformed)
        },
    );
    assert!(
        matches!(result, Err(DlpForwardError::CapacityExceeded)),
        "provenance_fault_not_observed"
    );
    assert!(
        body == original,
        "provenance_failure_committed_request_mutation"
    );
    assert!(
        fixture.store.stats()?.entries == 0,
        "provenance_failure_added_mapping"
    );
    drop(fault);
    let _retry = prepare_attempt(&request, &mut body, PROVIDER, TARGET)?;
    Ok(())
}

// Each authority probe and mapped/zero-token branch runs as an independent test.
#[derive(Clone, Copy)]
enum PublicationProbe {
    Cursor,
    Call,
    Opaque,
    CallId,
    StrictMapping,
    MatchingMapping,
}

#[derive(Clone, Copy)]
enum PublicationDelivery {
    Unpolled,
    DropAfter(usize),
    Complete,
    PositiveControl,
    UnavailableAfterTerminal,
}

const C3_RELEASE_FRAMES: usize = 5;

async fn c3_sse_publication_boundary_case(
    mapped: bool,
    authority: PublicationProbe,
    delivery: PublicationDelivery,
) -> TestResult {
    use super::completed_context::CompletedCallSpec;
    let fixture = ResponsesFixture::new();
    let mut body =
        json!({"input":if mapped { CONTACT } else { "Synthetic ordinary input" },"stream":true});
    let attempt = fixture.prepare(&mut body, false, Some(SESSION))?;
    let binding = attempt.binding.clone();
    let wire_text = body["input"].as_str().ok_or("synthetic_carrier_missing")?;
    // The zero-token branch also checks a real, independently uncompleted mapping.
    // It must remain unavailable even after a successful zero-dependency completion.
    let mut decoy =
        fixture
            .store
            .admit_published_policy("codex", Some(SESSION), binding.policy_revision)?;
    let decoy_reference = decoy.prepare(
        &[TokenFragment {
            raw: "Synthetic uncompleted dependency",
            whole_span_categories: &[TokenCategory::Custom],
        }],
        &[],
        |references| Ok(references[0].clone()),
    )?;
    let reference = if mapped { wire_text } else { &decoy_reference };
    let original_arguments = json!({"value":wire_text}).to_string();
    let delivered_arguments = json!({"value":if mapped { CONTACT } else { wire_text }}).to_string();
    let tool = json!({"id":"synthetic-c3-delivery-call-item","type":"function_call",
        "call_id":"synthetic-c3-delivery-call","name":"synthetic-tool",
        "arguments":original_arguments,"status":"completed",
        "encrypted_reasoning":"synthetic-opaque-unit"});
    let mut added = tool.clone();
    added["arguments"] = json!("");
    added["status"] = json!("in_progress");
    let response = json!({"id":"synthetic-c3-delivery-response","object":"response",
        "status":"completed","output":[tool]});
    let events = [
        json!({"type":"response.output_item.added","output_index":0,"item":added}),
        json!({"type":"response.function_call_arguments.delta","output_index":0,
            "item_id":"synthetic-c3-delivery-call-item","delta":original_arguments}),
        json!({"type":"response.function_call_arguments.done","output_index":0,
            "item_id":"synthetic-c3-delivery-call-item","arguments":original_arguments}),
        json!({"type":"response.output_item.done","output_index":0,"item":tool}),
        json!({"type":"response.completed","response":response}),
    ];
    let mut wire = String::new();
    for event in events {
        let kind = event["type"]
            .as_str()
            .ok_or("synthetic_event_kind_missing")?;
        wire.push_str(&format!("event: {kind}\ndata: {event}\n\n"));
    }
    let delivered = attempt
        .restore_client_http_response(
            Response::builder()
                .header("content-type", "text/event-stream")
                .body(Body::from(wire))?,
            ArgumentNormalizationMode::Exact,
            None,
        )
        .await;
    assert!(
        delivered.status() == StatusCode::OK,
        "synthetic_completion_rejected"
    );
    let mut downstream = delivered.into_body();
    let probe = |visible: bool| -> TestResult {
        let mut observer = fixture.store.admit_published_policy(
            "codex",
            Some(SESSION),
            binding.policy_revision,
        )?;
        assert!(observer.is_empty(), "fresh_observer_has_ambient_authority");
        let call = CompletedCallSpec {
            call_id: "synthetic-c3-delivery-call",
            item_type: "function_call",
            name: "synthetic-tool",
            payload: &delivered_arguments,
            reasoning: None,
            encrypted_reasoning: Some("synthetic-opaque-unit"),
        };
        let result = match authority {
            PublicationProbe::Cursor => observer.resume_completed_context(
                &binding,
                Some("synthetic-c3-delivery-response"),
                &[],
                &[],
            ),
            PublicationProbe::Call => {
                observer.resume_completed_context(&binding, None, &[call], &[])
            }
            PublicationProbe::Opaque => {
                observer.resume_completed_context(&binding, None, &[], &["synthetic-opaque-unit"])
            }
            PublicationProbe::CallId => observer.verify_output_call_ids(
                &binding,
                Some("synthetic-c3-delivery-response"),
                &["synthetic-c3-delivery-call"],
            ),
            PublicationProbe::StrictMapping => {
                observer.resume_completed_tokens(&binding, &[reference])
            }
            PublicationProbe::MatchingMapping => {
                observer.resume_matching_completed_tokens(&binding, &[reference])
            }
        };
        let mapping_probe = matches!(
            authority,
            PublicationProbe::StrictMapping | PublicationProbe::MatchingMapping
        );
        let matching_probe = matches!(authority, PublicationProbe::MatchingMapping);
        let read_only_probe = matches!(authority, PublicationProbe::CallId);
        if visible && (!mapping_probe || mapped) || matching_probe {
            assert!(result.is_ok(), "completed_authority_probe_rejected");
        } else {
            assert!(
                matches!(result, Err(TokenError::ContextUnavailable)),
                "mandatory_authority_published_before_delivery_resume"
            );
        }
        assert!(
            observer.resolve(reference).is_ok() == (visible && mapped && !read_only_probe),
            "mandatory_mapping_authority_visibility_changed"
        );
        assert!(
            observer.authorized_tokens()?.as_slice().len()
                == usize::from(visible && mapped && !read_only_probe),
            "mandatory_dependency_lineage_changed"
        );
        Ok(())
    };
    let check_boundary = !matches!(delivery, PublicationDelivery::PositiveControl);
    if check_boundary {
        probe(false)?;
    }
    if !matches!(delivery, PublicationDelivery::Unpolled) {
        for index in 1..=C3_RELEASE_FRAMES {
            let frame = downstream
                .frame()
                .await
                .ok_or("synthetic_release_frame_missing")?
                .map_err(|_| "synthetic_release_frame_read_failed")?;
            let data = frame.data_ref().ok_or("synthetic_data_frame_missing")?;
            let text = std::str::from_utf8(data).map_err(|_| "synthetic_frame_invalid_utf8")?;
            assert!(
                !text.contains("event: response.failed"),
                "synthetic_valid_completion_failed"
            );
            assert!(
                text.contains("event: response.completed") == (index == C3_RELEASE_FRAMES),
                "synthetic_terminal_position_changed"
            );
            if check_boundary && !matches!(delivery, PublicationDelivery::DropAfter(_)) {
                probe(false)?;
            }
            if matches!(delivery, PublicationDelivery::DropAfter(stop) if stop == index) {
                drop(downstream);
                probe(false)?;
                return Ok(());
            }
        }
        if matches!(delivery, PublicationDelivery::UnavailableAfterTerminal) {
            super::tokenization::publication_test_support::unavailable(&fixture.store)?;
            let next = downstream.frame().await;
            assert!(next.is_some(), "unavailable_activation_silently_completed");
            let error = match next {
                Some(Err(error)) => error,
                _ => return Err("unavailable_activation_appended_protocol_frame".into()),
            };
            assert!(
                error.to_string() == "Response stream failed.",
                "activation_error_not_sanitized"
            );
            assert!(
                downstream.frame().await.is_none(),
                "activation_error_repeated"
            );
            return Ok(());
        }
        assert!(
            downstream.frame().await.is_none(),
            "synthetic_terminal_not_last"
        );
        probe(true)?;
    } else {
        drop(downstream);
        probe(false)?;
    }
    Ok(())
}

macro_rules! publication_probe_tests {
    ($mapped:expr, $delivery:expr, $( $name:ident => $probe:ident ),+ $(,)?) => {
        $(
            #[tokio::test]
            #[serial_test::serial]
            async fn $name() -> TestResult {
                c3_sse_publication_boundary_case(
                    $mapped, PublicationProbe::$probe, $delivery,
                ).await
            }
        )+
    };
}

macro_rules! publication_delivery_tests {
    ($mapped:expr, $delivery:expr, $cursor:ident, $call:ident, $opaque:ident,
        $call_id:ident, $strict:ident, $matching:ident) => {
        publication_probe_tests!($mapped, $delivery,
            $cursor => Cursor, $call => Call, $opaque => Opaque,
            $call_id => CallId, $strict => StrictMapping, $matching => MatchingMapping,
        );
    };
}

publication_delivery_tests!(
    true,
    PublicationDelivery::Complete,
    c3_mandatory_sse_publication_mapped_wait_cursor,
    c3_mandatory_sse_publication_mapped_wait_call,
    c3_mandatory_sse_publication_mapped_wait_opaque,
    c3_mandatory_sse_publication_mapped_wait_call_id,
    c3_mandatory_sse_publication_mapped_wait_strict_mapping,
    c3_mandatory_sse_publication_mapped_wait_matching_mapping
);
publication_delivery_tests!(
    false,
    PublicationDelivery::Complete,
    c3_mandatory_sse_publication_zero_wait_cursor,
    c3_mandatory_sse_publication_zero_wait_call,
    c3_mandatory_sse_publication_zero_wait_opaque,
    c3_mandatory_sse_publication_zero_wait_call_id,
    c3_mandatory_sse_publication_zero_wait_strict_mapping,
    c3_mandatory_sse_publication_zero_wait_matching_mapping
);
publication_delivery_tests!(
    true,
    PublicationDelivery::PositiveControl,
    c3_mandatory_sse_publication_mapped_positive_cursor,
    c3_mandatory_sse_publication_mapped_positive_call,
    c3_mandatory_sse_publication_mapped_positive_opaque,
    c3_mandatory_sse_publication_mapped_positive_call_id,
    c3_mandatory_sse_publication_mapped_positive_strict_mapping,
    c3_mandatory_sse_publication_mapped_positive_matching_mapping
);
publication_delivery_tests!(
    false,
    PublicationDelivery::PositiveControl,
    c3_mandatory_sse_publication_zero_positive_cursor,
    c3_mandatory_sse_publication_zero_positive_call,
    c3_mandatory_sse_publication_zero_positive_opaque,
    c3_mandatory_sse_publication_zero_positive_call_id,
    c3_mandatory_sse_publication_zero_positive_strict_mapping,
    c3_mandatory_sse_publication_zero_positive_matching_mapping
);
publication_delivery_tests!(
    true,
    PublicationDelivery::Unpolled,
    c3_mandatory_sse_publication_mapped_unpolled_cursor,
    c3_mandatory_sse_publication_mapped_unpolled_call,
    c3_mandatory_sse_publication_mapped_unpolled_opaque,
    c3_mandatory_sse_publication_mapped_unpolled_call_id,
    c3_mandatory_sse_publication_mapped_unpolled_strict_mapping,
    c3_mandatory_sse_publication_mapped_unpolled_matching_mapping
);
publication_delivery_tests!(
    false,
    PublicationDelivery::Unpolled,
    c3_mandatory_sse_publication_zero_unpolled_cursor,
    c3_mandatory_sse_publication_zero_unpolled_call,
    c3_mandatory_sse_publication_zero_unpolled_opaque,
    c3_mandatory_sse_publication_zero_unpolled_call_id,
    c3_mandatory_sse_publication_zero_unpolled_strict_mapping,
    c3_mandatory_sse_publication_zero_unpolled_matching_mapping
);

macro_rules! publication_drop_tests {
    ($name:ident, $mapped:expr, $frame:expr) => {
        mod $name {
            use super::*;
            publication_delivery_tests!(
                $mapped,
                PublicationDelivery::DropAfter($frame),
                cursor,
                call,
                opaque,
                call_id,
                strict_mapping,
                matching_mapping
            );
        }
    };
}

publication_drop_tests!(c3_mandatory_sse_publication_mapped_drop1, true, 1);
publication_drop_tests!(c3_mandatory_sse_publication_mapped_drop2, true, 2);
publication_drop_tests!(c3_mandatory_sse_publication_mapped_drop3, true, 3);
publication_drop_tests!(c3_mandatory_sse_publication_mapped_drop4, true, 4);
publication_drop_tests!(c3_mandatory_sse_publication_mapped_drop5, true, 5);
publication_drop_tests!(c3_mandatory_sse_publication_zero_drop1, false, 1);
publication_drop_tests!(c3_mandatory_sse_publication_zero_drop2, false, 2);
publication_drop_tests!(c3_mandatory_sse_publication_zero_drop3, false, 3);
publication_drop_tests!(c3_mandatory_sse_publication_zero_drop4, false, 4);
publication_drop_tests!(c3_mandatory_sse_publication_zero_drop5, false, 5);

// Store-level controls use only a test facade; production issuance stays private.
mod c3_publication_transaction_visibility {
    use super::super::{
        completed_context::{ActualAttemptBinding, CompletedCallSpec},
        tokenization::{DlpRequestLease, DlpTokenStore},
    };
    use super::*;

    const CURSOR: &str = "synthetic-c3-slot-response";
    const OPAQUE: &str = "synthetic-c3-slot-opaque";

    fn call() -> CompletedCallSpec<'static> {
        CompletedCallSpec {
            call_id: "synthetic-c3-slot-call",
            item_type: "function_call",
            name: "synthetic_nonexecuting_tool",
            payload: "{}",
            reasoning: None,
            encrypted_reasoning: Some(OPAQUE),
        }
    }

    fn assert_visibility(
        store: &DlpTokenStore,
        binding: &ActualAttemptBinding,
        reference: &str,
        probe: PublicationProbe,
        visible: bool,
        mapped: bool,
    ) -> TestResult {
        let mut observer = store.admit("codex", Some(SESSION), 1)?;
        let result = match probe {
            PublicationProbe::Cursor => {
                observer.resume_completed_context(binding, Some(CURSOR), &[], &[])
            }
            PublicationProbe::Call => {
                observer.resume_completed_context(binding, None, &[call()], &[])
            }
            PublicationProbe::Opaque => {
                observer.resume_completed_context(binding, None, &[], &[OPAQUE])
            }
            PublicationProbe::CallId => {
                observer.verify_output_call_ids(binding, Some(CURSOR), &[call().call_id])
            }
            PublicationProbe::StrictMapping => {
                observer.resume_completed_tokens(binding, &[reference])
            }
            PublicationProbe::MatchingMapping => {
                observer.resume_matching_completed_tokens(binding, &[reference])
            }
        };
        let mapping = matches!(
            probe,
            PublicationProbe::StrictMapping | PublicationProbe::MatchingMapping
        );
        let permissive = matches!(probe, PublicationProbe::MatchingMapping);
        if permissive || visible && (!mapping || mapped) {
            assert!(result.is_ok(), "slot_visible_authority_rejected");
        } else {
            assert!(
                matches!(result, Err(TokenError::ContextUnavailable)),
                "hidden_slot_granted_authority"
            );
        }
        let resolves = visible && mapped && !matches!(probe, PublicationProbe::CallId);
        assert!(
            observer.resolve(reference).is_ok() == resolves,
            "hidden_slot_granted_mapping"
        );
        assert!(
            observer.authorized_tokens()?.len() == usize::from(resolves),
            "slot_dependency_count_changed"
        );
        Ok(())
    }

    fn fixture(
        mapped: bool,
    ) -> TestResult<(DlpTokenStore, DlpRequestLease, ActualAttemptBinding, String)> {
        let store = DlpTokenStore::new();
        let mut lease = store.admit("codex", Some(SESSION), 1)?;
        let mut issuer = store.admit("codex", Some(SESSION), 1)?;
        let reference = if mapped { &mut lease } else { &mut issuer }.prepare(
            &[TokenFragment {
                raw: "Synthetic slot dependency",
                whole_span_categories: &[TokenCategory::Custom],
            }],
            &[],
            |references| Ok(references[0].clone()),
        )?;
        let binding =
            lease.bind_actual_attempt(PROVIDER, "openai_responses", TARGET, Some(PRINCIPAL))?;
        Ok((store, lease, binding, reference))
    }

    fn case(mapped: bool, probe: PublicationProbe, cancel: bool) -> TestResult {
        let (store, lease, binding, reference) = fixture(mapped)?;
        let pending = lease.prepare_context_for_test(&binding, CURSOR, &[call()], &[OPAQUE])?;
        assert_visibility(&store, &binding, &reference, probe, false, mapped)?;
        if cancel {
            drop(pending);
            assert_visibility(&store, &binding, &reference, probe, false, mapped)?;
        } else {
            pending.activate()?;
            assert_visibility(&store, &binding, &reference, probe, true, mapped)?;
        }
        Ok(())
    }

    macro_rules! visibility_tests {
        ($mapped:expr, $cancel:expr, $( $name:ident => $probe:ident ),+ $(,)?) => { $(
            #[test]
            fn $name() -> TestResult { case($mapped, PublicationProbe::$probe, $cancel) }
        )+ };
    }
    visibility_tests!(true, false, mapped_cursor => Cursor, mapped_call => Call,
        mapped_opaque => Opaque, mapped_call_id => CallId,
        mapped_strict_mapping => StrictMapping, mapped_matching_mapping => MatchingMapping);
    visibility_tests!(false, false, zero_cursor => Cursor, zero_call => Call,
        zero_opaque => Opaque, zero_call_id => CallId,
        zero_strict_mapping => StrictMapping, zero_matching_mapping => MatchingMapping);
    visibility_tests!(true, true, cancelled_cursor => Cursor, cancelled_call => Call,
        cancelled_opaque => Opaque, cancelled_call_id => CallId,
        cancelled_strict_mapping => StrictMapping, cancelled_matching_mapping => MatchingMapping);
    #[test]
    fn duplicate_cancel_and_activation_preserve_other_owner_and_visible_record() -> TestResult {
        let (store, lease, binding, reference) = fixture(true)?;
        let first = lease.prepare_context_for_test(&binding, CURSOR, &[call()], &[OPAQUE])?;
        let second = lease.prepare_context_for_test(&binding, CURSOR, &[call()], &[OPAQUE])?;
        drop(first);
        assert_visibility(
            &store,
            &binding,
            &reference,
            PublicationProbe::Cursor,
            false,
            true,
        )?;
        second.activate()?;
        let committed = store.stats()?;
        let ledger = store.allocation_ledger_for_test()?;
        let physical = ledger.used();
        let duplicate = lease.prepare_context_for_test(&binding, CURSOR, &[call()], &[OPAQUE])?;
        drop(duplicate);
        assert!(
            store.stats()? == committed,
            "visible_duplicate_changed_logical_charge"
        );
        assert!(
            ledger.used() == physical,
            "visible_duplicate_leaked_allocation"
        );
        assert_visibility(
            &store,
            &binding,
            &reference,
            PublicationProbe::Cursor,
            true,
            true,
        )?;
        Ok(())
    }

    #[test]
    fn all_duplicate_cancellations_reuse_slot_with_exact_owner_and_record_cleanup() -> TestResult {
        let (store, lease, binding, reference) = fixture(true)?;
        let ledger = store.allocation_ledger_for_test()?;
        // Establish the retained backing high-water mark before the exact baseline.
        drop(lease.prepare_context_for_test(&binding, CURSOR, &[call()], &[OPAQUE])?);
        let logical = store.stats()?;
        let physical = ledger.used();
        for _ in 0..4 {
            let a = lease.prepare_context_for_test(&binding, CURSOR, &[call()], &[OPAQUE])?;
            let b = lease.prepare_context_for_test(&binding, CURSOR, &[call()], &[OPAQUE])?;
            let c = lease.prepare_context_for_test(&binding, CURSOR, &[call()], &[OPAQUE])?;
            drop(b);
            drop(a);
            assert_visibility(
                &store,
                &binding,
                &reference,
                PublicationProbe::StrictMapping,
                false,
                true,
            )?;
            drop(c);
            assert!(
                store.stats()? == logical,
                "cancelled_slot_leaked_logical_charge"
            );
            assert!(
                ledger.used() == physical,
                "cancelled_slot_leaked_physical_charge"
            );
        }
        Ok(())
    }

    #[test]
    fn mapping_write_and_two_pending_records_do_not_invalidate_admission() -> TestResult {
        let (store, lease, binding, reference) = fixture(true)?;
        let a = lease.prepare_context_for_test(&binding, CURSOR, &[call()], &[OPAQUE])?;
        let b = lease.prepare_context_for_test(&binding, "synthetic-c3-second-slot", &[], &[])?;
        let mut writer = store.admit("codex", Some(SESSION), 1)?;
        writer.prepare(
            &[TokenFragment {
                raw: "Synthetic concurrent mapping",
                whole_span_categories: &[TokenCategory::Custom],
            }],
            &[],
            |_| Ok(()),
        )?;
        b.activate()?;
        a.activate()?;
        assert_visibility(
            &store,
            &binding,
            &reference,
            PublicationProbe::Cursor,
            true,
            true,
        )?;
        let mut observer = store.admit("codex", Some(SESSION), 1)?;
        observer.resume_completed_context(&binding, Some("synthetic-c3-second-slot"), &[], &[])?;
        assert!(
            observer.resolve(&reference).is_ok(),
            "second_record_lost_full_lineage"
        );
        Ok(())
    }

    #[test]
    fn conflicting_duplicate_denies_without_erasing_pending_or_visible_record() -> TestResult {
        let (store, lease, binding, reference) = fixture(true)?;
        let mut pending =
            Some(lease.prepare_context_for_test(&binding, CURSOR, &[call()], &[OPAQUE])?);
        let ledger = store.allocation_ledger_for_test()?;
        for visible in [false, true] {
            if visible {
                pending
                    .take()
                    .ok_or("synthetic_pending_missing")?
                    .activate()?;
            }
            let before = store.stats()?;
            let physical = ledger.used();
            let result = lease.prepare_context_for_test(&binding, CURSOR, &[], &[]);
            assert!(
                matches!(result, Err(TokenError::ContextUnavailable)),
                "conflicting_slot_admitted"
            );
            assert!(
                ledger.used() == physical,
                "conflicting_slot_leaked_allocation"
            );
            assert!(
                store.stats()? == before,
                "conflicting_slot_changed_logical_charge"
            );
            assert_visibility(
                &store,
                &binding,
                &reference,
                PublicationProbe::Cursor,
                visible,
                true,
            )?;
        }
        Ok(())
    }

    #[test]
    fn pending_owner_pins_namespace_without_request_lease_until_cancelled() -> TestResult {
        let (store, lease, binding, _) = fixture(false)?;
        let pending = lease.prepare_context_for_test(&binding, CURSOR, &[call()], &[OPAQUE])?;
        drop(lease);
        let future = std::time::Instant::now() + std::time::Duration::from_secs(3600);
        store.expire_idle_at(future)?;
        assert!(store.stats()?.namespaces == 1, "pending_namespace_expired");
        drop(pending);
        store.expire_idle_at(future)?;
        assert!(
            store.stats()?.namespaces == 0,
            "cancelled_namespace_remained_pinned"
        );
        assert!(
            store.stats()?.metadata_bytes == 0,
            "expired_namespace_leaked_metadata"
        );
        Ok(())
    }
}

mod c3_publication_transaction_capacity {
    use super::super::tokenization::DlpTokenStore;
    use super::*;

    #[test]
    fn global_pending_capacity_is_reserved_before_other_namespaces_publish() -> TestResult {
        use super::super::completed_context::MAX_CONTEXT_ENTRIES_PER_NAMESPACE;
        let store = DlpTokenStore::new();
        let source = store.admit("codex", Some(SESSION), 1)?;
        let binding =
            source.bind_actual_attempt(PROVIDER, "openai_responses", TARGET, Some(PRINCIPAL))?;
        let opaque_values: Vec<_> = (0..MAX_CONTEXT_ENTRIES_PER_NAMESPACE - 1)
            .map(|index| format!("synthetic-capacity-unit-{index}"))
            .collect();
        let opaque_refs: Vec<_> = opaque_values.iter().map(String::as_str).collect();
        let pending = source.prepare_context_for_test(
            &binding,
            "synthetic-capacity-pending",
            &[],
            &opaque_refs,
        )?;
        for index in 0..15 {
            let session = format!("synthetic-capacity-namespace-{index}");
            let lease = store.admit("codex", Some(&session), 1)?;
            let actual =
                lease.bind_actual_attempt(PROVIDER, "openai_responses", TARGET, Some(PRINCIPAL))?;
            lease
                .prepare_context_for_test(&actual, "synthetic-capacity-visible", &[], &opaque_refs)?
                .activate()?;
        }
        let contender = store.admit("codex", Some("synthetic-capacity-contender"), 1)?;
        let actual =
            contender.bind_actual_attempt(PROVIDER, "openai_responses", TARGET, Some(PRINCIPAL))?;
        let denied =
            contender.prepare_context_for_test(&actual, "synthetic-capacity-overflow", &[], &[]);
        assert!(
            matches!(denied, Err(TokenError::CapacityExceeded)),
            "pending_global_capacity_not_reserved"
        );
        pending.activate()?;
        let mut observer = store.admit("codex", Some(SESSION), 1)?;
        observer.resume_completed_context(
            &binding,
            Some("synthetic-capacity-pending"),
            &[],
            &[],
        )?;
        Ok(())
    }

    #[test]
    fn final_store_destruction_releases_retained_backing_without_a_cycle() -> TestResult {
        let store = DlpTokenStore::new();
        let lease = store.admit("codex", Some(SESSION), 1)?;
        let binding =
            lease.bind_actual_attempt(PROVIDER, "openai_responses", TARGET, Some(PRINCIPAL))?;
        let ledger = store.allocation_ledger_for_test()?;
        let pending =
            lease.prepare_context_for_test(&binding, "synthetic-destruction-pending", &[], &[])?;
        drop(lease);
        drop(store);
        drop(pending);
        assert!(
            ledger.used().plaintext == 0,
            "store_destruction_retained_plaintext"
        );
        assert!(
            ledger.used().metadata
                == super::super::token_store_owners::StoreAllocationLedger::control_bytes()?,
            "store_destruction_retained_ownership_cycle"
        );
        Ok(())
    }
}

#[test]
fn c3_publication_transaction_rejection_destroys_staged_vectors_before_record_refund() -> TestResult
{
    use super::{
        completed_context::{
            CompletedCallEntry, CompletedCallSpec, CALL_ENTRY_METADATA, CONTEXT_ENTRY_METADATA,
            OPAQUE_ENTRY_METADATA,
        },
        token_store_owners::test_support::{install, Kind, Phase},
        tokenization::{DlpTokenStore, MappingEntry},
    };
    use std::{cell::Cell, rc::Rc};
    let store = DlpTokenStore::new();
    let mut lease = store.admit("codex", Some(SESSION), 1)?;
    lease.prepare(
        &[TokenFragment {
            raw: "Synthetic staging dependency",
            whole_span_categories: &[TokenCategory::Custom],
        }],
        &[],
        |_| Ok(()),
    )?;
    let binding =
        lease.bind_actual_attempt(PROVIDER, "openai_responses", TARGET, Some(PRINCIPAL))?;
    let ledger = store.allocation_ledger_for_test()?;
    let before = ledger.used();
    let logical = store.stats()?;
    let staged = std::mem::size_of::<CompletedCallEntry>() + std::mem::size_of::<[u8; 32]>();
    // The owner charge is bounded by its actual test facade size.
    let owner = std::mem::size_of::<super::tokenization::PendingContextForTest>();
    let metadata = CONTEXT_ENTRY_METADATA + CALL_ENTRY_METADATA + OPAQUE_ENTRY_METADATA + 64;
    let denied = Rc::new(Cell::new(false));
    let refunded = Rc::new(Cell::new(false));
    let observed = Rc::new(Cell::new(0));
    let (deny, refund, retained) = (denied.clone(), refunded.clone(), observed.clone());
    let hook = install(move |event| {
        if event.kind == Kind::Vector
            && event.phase == Phase::BeforeAllocation
            && event.bytes == std::mem::size_of::<MappingEntry>()
        {
            deny.set(true);
            return Err(TokenError::CapacityExceeded);
        }
        if event.kind == Kind::Vector && event.phase == Phase::BeforeDrop && event.bytes == metadata
        {
            refund.set(true);
            retained.set(event.used.metadata.saturating_sub(before.metadata));
        }
        Ok(())
    });
    let result = lease.prepare_context_for_test(
        &binding,
        "synthetic-rejected-slot",
        &[CompletedCallSpec {
            call_id: "synthetic-rejected-call",
            item_type: "function_call",
            name: "synthetic_nonexecuting_tool",
            payload: "{}",
            reasoning: None,
            encrypted_reasoning: Some("synthetic-rejected-opaque"),
        }],
        &["synthetic-rejected-opaque"],
    );
    drop(hook);
    assert!(denied.get(), "staged_dependency_denial_not_reached");
    assert!(
        matches!(result, Err(TokenError::CapacityExceeded)),
        "staged_dependency_denial_bypassed"
    );
    assert!(refunded.get(), "record_refund_not_observed");
    assert!(
        observed.get() == owner,
        "record_refunded_before_staged_vectors_destroyed"
    );
    assert!(staged > 0, "staged_vector_fixture_empty");
    assert!(
        ledger.used() == before,
        "staging_rejection_leaked_allocation"
    );
    assert!(
        store.stats()? == logical,
        "staging_rejection_leaked_logical_charge"
    );
    Ok(())
}

fn c3_publication_duplicate_owner_only_case(visible: bool) -> TestResult {
    use super::{
        token_store_owners::AllocationCharge,
        tokenization::{DlpTokenStore, PendingContextForTest},
    };
    let store = DlpTokenStore::new();
    let lease = store.admit("codex", Some(SESSION), 1)?;
    let binding =
        lease.bind_actual_attempt(PROVIDER, "openai_responses", TARGET, Some(PRINCIPAL))?;
    let mut first = Some(lease.prepare_context_for_test(
        &binding,
        "synthetic-owner-only-duplicate",
        &[],
        &[],
    )?);
    if visible {
        first
            .take()
            .ok_or("synthetic_pending_missing")?
            .activate()?;
    }
    let ledger = store.allocation_ledger_for_test()?;
    let before = ledger.used();
    let held = ledger.reserve(AllocationCharge {
        plaintext: 0,
        metadata: ledger.limits_for_test().metadata
            - before.metadata
            - std::mem::size_of::<PendingContextForTest>(),
    })?;
    let duplicate =
        lease.prepare_context_for_test(&binding, "synthetic-owner-only-duplicate", &[], &[]);
    let admitted = duplicate.is_ok();
    drop(duplicate);
    drop(held);
    assert!(
        ledger.used() == before,
        "duplicate_owner_only_leaked_allocation"
    );
    assert!(admitted, "duplicate_required_second_record_reservation");
    drop(first);
    Ok(())
}

#[test]
fn c3_publication_transaction_hidden_duplicate_needs_only_owner_backing() -> TestResult {
    c3_publication_duplicate_owner_only_case(false)
}
#[test]
fn c3_publication_transaction_visible_duplicate_needs_only_owner_backing() -> TestResult {
    c3_publication_duplicate_owner_only_case(true)
}

#[test]
fn c3_publication_transaction_growth_retry_drops_obsolete_backing_before_reallocation() -> TestResult
{
    use super::{
        token_store_owners::test_support::{install, Kind, Phase},
        tokenization::DlpTokenStore,
    };
    use std::{cell::Cell, rc::Rc};
    let store = DlpTokenStore::new();
    let source = store.admit("codex", Some(SESSION), 1)?;
    let binding =
        source.bind_actual_attempt(PROVIDER, "openai_responses", TARGET, Some(PRINCIPAL))?;
    let contender = source.fork_attempt()?;
    let actual = binding.clone();
    let ledger = store.allocation_ledger_for_test()?;
    let slot_bytes = store.context_slot_bytes_for_test();
    let before = ledger.used().metadata;
    let baseline_after_contention = Rc::new(Cell::new(0));
    let observed_retry = Rc::new(Cell::new(0));
    let injected = Rc::new(Cell::new(false));
    let (baseline, observed, ran) = (
        baseline_after_contention.clone(),
        observed_retry.clone(),
        injected.clone(),
    );
    let allocations = ledger.clone();
    let hook = install(move |event| {
        if event.kind == Kind::Vector
            && event.phase == Phase::AfterAllocation
            && event.bytes == slot_bytes
            && !ran.replace(true)
        {
            contender
                .prepare_context_for_test(&actual, "synthetic-growth-contender", &[], &[])?
                .activate()?;
            baseline.set(allocations.used().metadata - slot_bytes);
        }
        if event.kind == Kind::Vector
            && event.phase == Phase::BeforeAllocation
            && event.bytes == 2 * slot_bytes
        {
            observed.set(event.used.metadata);
        }
        Ok(())
    });
    let pending =
        source.prepare_context_for_test(&binding, "synthetic-growth-primary", &[], &[])?;
    drop(hook);
    assert!(
        injected.get() && baseline_after_contention.get() > before,
        "growth_contention_not_reached"
    );
    assert!(
        observed_retry.get() == baseline_after_contention.get() + 2 * slot_bytes,
        "growth_retry_retained_obsolete_replacement"
    );
    pending.activate()?;
    Ok(())
}

mod c3_publication_transaction_retirement {
    use super::super::{
        completed_context::{ActualAttemptBinding, CompletedCallSpec},
        token_store_owners::test_support::{install, Kind, Phase},
        tokenization::{publication_test_support as support, DlpTokenStore},
    };
    use super::*;
    use std::{cell::Cell, rc::Rc};

    const CURSOR: &str = "synthetic-retirement-response";
    const REUSED: &str = "synthetic-retirement-reused";
    const OPAQUE: &str = "synthetic-retirement-opaque";

    fn call() -> CompletedCallSpec<'static> {
        CompletedCallSpec {
            call_id: "synthetic-retirement-call",
            item_type: "function_call",
            name: "synthetic_nonexecuting_tool",
            payload: "{}",
            reasoning: None,
            encrypted_reasoning: Some(OPAQUE),
        }
    }

    fn probe(
        store: &DlpTokenStore,
        binding: &ActualAttemptBinding,
        cursor: &str,
        visible: bool,
    ) -> TestResult {
        let mut observer = store.admit("codex", Some(SESSION), 1)?;
        let result = observer.resume_completed_context(binding, Some(cursor), &[], &[]);
        assert!(result.is_ok() == visible, "retirement_visibility_changed");
        if !visible {
            assert!(
                matches!(result, Err(TokenError::ContextUnavailable)),
                "retirement_visibility_error_changed"
            );
        }
        Ok(())
    }

    fn reused_slot_case(activate: bool) -> TestResult {
        let store = DlpTokenStore::new();
        let lease = store.admit("codex", Some(SESSION), 1)?;
        let binding =
            lease.bind_actual_attempt(PROVIDER, "openai_responses", TARGET, Some(PRINCIPAL))?;
        let ledger = store.allocation_ledger_for_test()?;
        drop(lease.prepare_context_for_test(&binding, CURSOR, &[call()], &[OPAQUE])?);
        let logical = store.stats()?;
        let physical = ledger.used();
        let old = lease.prepare_context_for_test(&binding, CURSOR, &[call()], &[OPAQUE])?;
        let before = support::counts(&store)?;
        let next = lease.fork_attempt()?;
        let actual = binding.clone();
        let observed = store.clone();
        let replacement = Rc::new(std::cell::RefCell::new(None));
        let result = replacement.clone();
        let hook = support::before_retirement(move || {
            let run = (|| -> TestResult<_> {
                assert!(
                    support::counts(&observed)? == before,
                    "retired_record_refunded_early"
                );
                let new = next.prepare_context_for_test(&actual, REUSED, &[], &[])?;
                let during = support::counts(&observed)?;
                assert!(
                    during.1 == 3 && during.2 == 2,
                    "reused_slot_lost_retired_reservation"
                );
                if activate {
                    new.activate()?;
                    Ok(None)
                } else {
                    Ok(Some(new))
                }
            })();
            result.replace(Some(run));
        });
        drop(old);
        drop(hook);
        let pending = replacement
            .borrow_mut()
            .take()
            .ok_or("retirement_hook_not_reached")??;
        let after = support::counts(&store)?;
        assert!(
            after.1 == usize::from(!activate),
            "old_cleanup_refunded_reused_entries"
        );
        assert!(
            after.2 == usize::from(!activate),
            "old_cleanup_released_reused_pin"
        );
        probe(&store, &binding, CURSOR, false)?;
        probe(&store, &binding, REUSED, activate)?;
        drop(pending);
        if !activate {
            assert!(
                store.stats()? == logical,
                "reused_hidden_slot_leaked_logical_charge"
            );
            assert!(
                ledger.used() == physical,
                "reused_hidden_slot_leaked_physical_charge"
            );
        } else {
            assert!(
                after.0 == 1 && after.3 == 0,
                "reused_visible_slot_charge_changed"
            );
        }
        Ok(())
    }

    #[test]
    fn detached_cleanup_cannot_erase_or_refund_reused_hidden_slot() -> TestResult {
        reused_slot_case(false)
    }

    #[test]
    fn detached_cleanup_cannot_erase_or_refund_reused_visible_slot() -> TestResult {
        reused_slot_case(true)
    }

    #[test]
    fn overlapping_duplicate_cancellations_retire_record_without_old_owner_reference() -> TestResult
    {
        let store = DlpTokenStore::new();
        let lease = store.admit("codex", Some(SESSION), 1)?;
        let binding =
            lease.bind_actual_attempt(PROVIDER, "openai_responses", TARGET, Some(PRINCIPAL))?;
        let ledger = store.allocation_ledger_for_test()?;
        drop(lease.prepare_context_for_test(&binding, CURSOR, &[call()], &[OPAQUE])?);
        let logical = store.stats()?;
        let physical = ledger.used();
        let first = lease.prepare_context_for_test(&binding, CURSOR, &[call()], &[OPAQUE])?;
        let second = lease.prepare_context_for_test(&binding, CURSOR, &[call()], &[OPAQUE])?;
        let observed = store.clone();
        let allocations = ledger.clone();
        let ran = Rc::new(Cell::new(false));
        let reached = ran.clone();
        let hook = support::before_retirement(move || {
            reached.set(true);
            drop(second);
            let counts = support::counts(&observed).unwrap_or((
                usize::MAX,
                usize::MAX,
                usize::MAX,
                usize::MAX,
            ));
            assert!(
                counts.0 == 0 && counts.1 == 0 && counts.2 == 1,
                "overlapping_cancel_lost_owner_pin"
            );
            let owner = std::mem::size_of::<super::super::tokenization::PendingContextForTest>();
            assert!(
                counts.3 == owner,
                "overlapping_cancel_retained_record_reservation"
            );
            assert!(
                allocations.used().metadata == physical.metadata + owner,
                "nonlast_owner_retained_record_backing"
            );
        });
        drop(first);
        drop(hook);
        assert!(ran.get(), "overlapping_retirement_not_reached");
        assert!(
            store.stats()? == logical,
            "overlapping_cancel_leaked_logical_charge"
        );
        assert!(
            ledger.used() == physical,
            "overlapping_cancel_leaked_physical_charge"
        );
        probe(&store, &binding, CURSOR, false)?;
        Ok(())
    }

    #[test]
    fn record_destruction_keeps_reservation_and_last_pin_until_payload_is_gone() -> TestResult {
        let store = DlpTokenStore::new();
        let lease = store.admit("codex", Some(SESSION), 1)?;
        let binding =
            lease.bind_actual_attempt(PROVIDER, "openai_responses", TARGET, Some(PRINCIPAL))?;
        let pending = lease.prepare_context_for_test(&binding, CURSOR, &[call()], &[OPAQUE])?;
        let before = support::counts(&store)?;
        drop(lease);
        let observed = store.clone();
        let checks = Rc::new(Cell::new(0));
        let checked = checks.clone();
        let hook = install(move |event| {
            if event.kind == Kind::Vector
                && event.phase == Phase::BeforeDrop
                && event.bytes
                    == std::mem::size_of::<super::super::completed_context::CompletedCallEntry>()
            {
                assert!(
                    support::counts(&observed)? == before,
                    "payload_destruction_refunded_early"
                );
                observed.expire_idle_at(
                    std::time::Instant::now() + std::time::Duration::from_secs(3600),
                )?;
                assert!(
                    observed.stats()?.namespaces == 1,
                    "retiring_payload_lost_expiry_pin"
                );
                checked.set(checked.get() + 1);
            }
            Ok(())
        });
        drop(pending);
        drop(hook);
        assert!(checks.get() == 1, "record_payload_destruction_not_observed");
        assert!(
            support::counts(&store)? == (0, 0, 0, 0),
            "retired_payload_cleanup_incomplete"
        );
        store.expire_idle_at(std::time::Instant::now() + std::time::Duration::from_secs(3600))?;
        assert!(
            store.stats()?.namespaces == 0,
            "retired_payload_kept_namespace_alive"
        );
        Ok(())
    }

    fn unavailable_case(activate: bool, duplicate: bool) -> TestResult {
        let store = DlpTokenStore::new();
        let lease = store.admit("codex", Some(SESSION), 1)?;
        let binding =
            lease.bind_actual_attempt(PROVIDER, "openai_responses", TARGET, Some(PRINCIPAL))?;
        let ledger = store.allocation_ledger_for_test()?;
        drop(lease.prepare_context_for_test(&binding, CURSOR, &[call()], &[OPAQUE])?);
        let before = ledger.used();
        let first = lease.prepare_context_for_test(&binding, CURSOR, &[call()], &[OPAQUE])?;
        let second = if duplicate {
            Some(lease.prepare_context_for_test(&binding, CURSOR, &[call()], &[OPAQUE])?)
        } else {
            None
        };
        support::unavailable(&store)?;
        if activate {
            assert!(
                matches!(first.activate(), Err(TokenError::Unavailable)),
                "unavailable_store_activated"
            );
        } else {
            drop(first);
        }
        if duplicate {
            let counts = support::counts(&store)?;
            assert!(
                counts.1 == 2 && counts.2 == 1,
                "unavailable_nonlast_cancel_lost_record"
            );
        }
        drop(second);
        assert!(
            support::counts(&store)? == (0, 0, 0, 0),
            "unavailable_cleanup_leaked_reservation"
        );
        assert!(
            ledger.used() == before,
            "unavailable_cleanup_leaked_allocation"
        );
        assert!(
            matches!(store.stats(), Err(TokenError::Unavailable)),
            "cleanup_reenabled_unavailable_store"
        );
        Ok(())
    }

    #[test]
    fn unavailable_store_cancellation_refunds_without_reenabling() -> TestResult {
        unavailable_case(false, false)
    }
    #[test]
    fn unavailable_store_duplicate_cancellation_refunds_once() -> TestResult {
        unavailable_case(false, true)
    }
    #[test]
    fn unavailable_activation_denies_and_cleans_last_owner() -> TestResult {
        unavailable_case(true, false)
    }
    #[test]
    fn unavailable_activation_denies_without_deleting_other_owner() -> TestResult {
        unavailable_case(true, true)
    }
}

mod c3_publication_transaction_admission_races {
    use super::super::{
        completed_context::{CompletedCallSpec, CompletedContextRecord},
        token_store_owners::{
            arc_backing_bytes,
            test_support::{install, Kind, Phase},
        },
        tokenization::{publication_test_support as support, DlpTokenStore},
    };
    use super::*;
    use std::{
        cell::{Cell, RefCell},
        rc::Rc,
    };

    const CURSOR: &str = "synthetic-admission-response";
    const OLD: &str = "synthetic-admission-old-slot";

    #[derive(Clone, Copy)]
    enum Rival {
        Removed,
        Hidden,
        Visible,
        Conflict,
    }

    fn case(rival: Rival) -> TestResult {
        let store = DlpTokenStore::new();
        let source = store.admit("codex", Some(SESSION), 1)?;
        let binding =
            source.bind_actual_attempt(PROVIDER, "openai_responses", TARGET, Some(PRINCIPAL))?;
        let old = source.prepare_context_for_test(&binding, OLD, &[], &[])?;
        let ledger = store.allocation_ledger_for_test()?;
        let contender = source.fork_attempt()?;
        let actual = binding.clone();
        let record_bytes = arc_backing_bytes::<CompletedContextRecord>()?;
        let competing = Rc::new(RefCell::new(None));
        let retained = competing.clone();
        let ran = Rc::new(Cell::new(false));
        let injected = ran.clone();
        let mut old = Some(old);
        let hook = install(move |event| {
            if event.kind == Kind::Shared
                && event.phase == Phase::AfterAllocation
                && event.bytes == record_bytes
                && !injected.replace(true)
            {
                drop(old.take());
                if !matches!(rival, Rival::Removed) {
                    let call = CompletedCallSpec {
                        call_id: "synthetic-admission-conflict-call",
                        item_type: "function_call",
                        name: "synthetic_nonexecuting_tool",
                        payload: "{}",
                        reasoning: None,
                        encrypted_reasoning: None,
                    };
                    let calls = if matches!(rival, Rival::Conflict) {
                        std::slice::from_ref(&call)
                    } else {
                        &[]
                    };
                    let next = contender.prepare_context_for_test(&actual, CURSOR, calls, &[])?;
                    if matches!(rival, Rival::Visible) {
                        next.activate()?;
                    } else {
                        retained.replace(Some(next));
                    }
                }
            }
            Ok(())
        });
        let primary = source.prepare_context_for_test(&binding, CURSOR, &[], &[]);
        drop(hook);
        assert!(ran.get(), "admission_race_not_reached");
        let counts = support::counts(&store)?;
        if matches!(rival, Rival::Conflict) {
            assert!(
                matches!(primary, Err(TokenError::ContextUnavailable)),
                "final_duplicate_conflict_admitted"
            );
            assert!(
                counts.0 == 0 && counts.1 == 1 && counts.2 == 1,
                "conflicting_candidate_changed_rival_ownership"
            );
        } else {
            let primary = primary?;
            assert!(
                counts.0 + counts.1 == 1,
                "admission_race_duplicated_record_charge"
            );
            assert!(
                counts.2 == if matches!(rival, Rival::Hidden) { 2 } else { 1 },
                "admission_race_lost_owner_pin"
            );
            primary.activate()?;
        }
        if let Some(next) = competing.borrow_mut().take() {
            next.activate()?;
        }
        let after = support::counts(&store)?;
        assert!(
            after.0 == 1 && after.1 == 0 && after.2 == 0 && after.3 == 0,
            "admission_race_left_pending_reservation"
        );
        let committed = store.stats()?;
        let physical = ledger.used();
        let mut observer = store.admit("codex", Some(SESSION), 1)?;
        observer.resume_completed_context(&binding, Some(CURSOR), &[], &[])?;
        assert!(
            matches!(
                observer.resume_completed_context(&binding, Some(OLD), &[], &[]),
                Err(TokenError::ContextUnavailable)
            ),
            "removed_admission_record_remained_visible"
        );
        drop(observer);
        assert!(
            store.stats()? == committed && ledger.used() == physical,
            "admission_race_leaked_transient_backing"
        );
        Ok(())
    }

    #[test]
    fn removal_during_staging_reuses_empty_slot() -> TestResult {
        case(Rival::Removed)
    }
    #[test]
    fn hidden_duplicate_inserted_during_staging_is_joined_atomically() -> TestResult {
        case(Rival::Hidden)
    }
    #[test]
    fn visible_duplicate_inserted_during_staging_is_not_erased() -> TestResult {
        case(Rival::Visible)
    }
    #[test]
    fn conflicting_duplicate_inserted_during_staging_rejects_candidate() -> TestResult {
        case(Rival::Conflict)
    }
}

mod c3_publication_transaction_exceptional_state {
    use super::super::{
        token_store_owners::StoreAllocationLedger,
        tokenization::{DlpTokenStore, PendingContextForTest},
    };
    use super::*;

    fn poisoned_case(activate: bool) -> TestResult {
        let store = DlpTokenStore::new();
        let lease = store.admit("codex", Some(SESSION), 1)?;
        let binding =
            lease.bind_actual_attempt(PROVIDER, "openai_responses", TARGET, Some(PRINCIPAL))?;
        let ledger = store.allocation_ledger_for_test()?;
        let pending =
            lease.prepare_context_for_test(&binding, "synthetic-poisoned-pending", &[], &[])?;
        let before = ledger.used();
        store.poison_for_test();
        if activate {
            assert!(
                matches!(pending.activate(), Err(TokenError::Unavailable)),
                "poisoned_store_activated"
            );
        } else {
            drop(pending);
        }
        let retained = ledger.used();
        assert!(
            retained.metadata + std::mem::size_of::<PendingContextForTest>() == before.metadata,
            "poisoned_cleanup_fabricated_record_refund"
        );
        assert!(
            matches!(store.stats(), Err(TokenError::Unavailable)),
            "poisoned_store_recovered_as_usable"
        );
        assert!(
            matches!(
                store.expire_idle_at(
                    std::time::Instant::now() + std::time::Duration::from_secs(3600)
                ),
                Err(TokenError::Unavailable)
            ),
            "poisoned_namespace_expired_as_usable"
        );
        drop(lease);
        drop(store);
        assert!(
            ledger.used().plaintext == 0
                && ledger.used().metadata == StoreAllocationLedger::control_bytes()?,
            "poisoned_store_destruction_retained_backing"
        );
        Ok(())
    }

    #[test]
    fn poisoned_cancellation_retains_uncertain_record_charge_until_store_destruction() -> TestResult
    {
        poisoned_case(false)
    }
    #[test]
    fn poisoned_activation_denies_without_recovering_mutex() -> TestResult {
        poisoned_case(true)
    }
}

mod c3_publication_transaction_bounds {
    use super::super::{
        token_store_owners::{
            test_support::{install, Kind, Phase},
            AllocationCharge,
        },
        tokenization::{publication_test_support as support, DlpTokenStore, PendingContextForTest},
    };
    use super::*;
    use std::{cell::Cell, rc::Rc};

    #[test]
    fn serial_exhaustion_rejects_new_record_but_preserves_existing_duplicate_owner() -> TestResult {
        let store = DlpTokenStore::new();
        let source = store.admit("codex", Some(SESSION), 1)?;
        let binding =
            source.bind_actual_attempt(PROVIDER, "openai_responses", TARGET, Some(PRINCIPAL))?;
        support::set_next_serial(&source, u64::MAX - 1)?;
        let first =
            source.prepare_context_for_test(&binding, "synthetic-last-slot-serial", &[], &[])?;
        let ledger = store.allocation_ledger_for_test()?;
        let logical = store.stats()?;
        let physical = ledger.used();
        let counts = support::counts(&store)?;
        assert!(
            matches!(
                source.prepare_context_for_test(
                    &binding,
                    "synthetic-overflow-slot-serial",
                    &[],
                    &[]
                ),
                Err(TokenError::CapacityExceeded)
            ),
            "context_serial_wrapped"
        );
        assert!(
            store.stats()? == logical
                && ledger.used() == physical
                && support::counts(&store)? == counts,
            "serial_exhaustion_changed_pending_record"
        );
        let duplicate =
            source.prepare_context_for_test(&binding, "synthetic-last-slot-serial", &[], &[])?;
        drop(first);
        duplicate.activate()?;
        assert!(
            support::counts(&store)? == (1, 0, 0, 0),
            "serial_exhaustion_invalidated_duplicate"
        );
        Ok(())
    }

    fn owner_budget_case(visible: bool) -> TestResult {
        let store = DlpTokenStore::new();
        let lease = store.admit("codex", Some(SESSION), 1)?;
        let binding =
            lease.bind_actual_attempt(PROVIDER, "openai_responses", TARGET, Some(PRINCIPAL))?;
        let mut first =
            Some(lease.prepare_context_for_test(&binding, "synthetic-bounded-owners", &[], &[])?);
        if visible {
            first
                .take()
                .ok_or("synthetic_pending_missing")?
                .activate()?;
        }
        let ledger = store.allocation_ledger_for_test()?;
        let logical = store.stats()?;
        let physical = ledger.used();
        let counts = support::counts(&store)?;
        let owner = std::mem::size_of::<PendingContextForTest>();
        // Leave one byte less than the minimum owner: reject without widening ownership.
        let almost = ledger.reserve(AllocationCharge {
            plaintext: 0,
            metadata: ledger.limits_for_test().metadata - physical.metadata - owner + 1,
        })?;
        assert!(
            matches!(
                lease.prepare_context_for_test(&binding, "synthetic-bounded-owners", &[], &[]),
                Err(TokenError::CapacityExceeded)
            ),
            "owner_admitted_without_full_bookkeeping_charge"
        );
        drop(almost);
        assert!(
            store.stats()? == logical
                && ledger.used() == physical
                && support::counts(&store)? == counts,
            "owner_difference_one_denial_leaked_charge"
        );
        let held = ledger.reserve(AllocationCharge {
            plaintext: 0,
            metadata: ledger.limits_for_test().metadata - physical.metadata - owner,
        })?;
        let admitted =
            lease.prepare_context_for_test(&binding, "synthetic-bounded-owners", &[], &[])?;
        assert!(
            ledger.used().metadata == ledger.limits_for_test().metadata,
            "owner_limit_not_reached"
        );
        assert!(
            matches!(
                lease.prepare_context_for_test(&binding, "synthetic-bounded-owners", &[], &[]),
                Err(TokenError::CapacityExceeded)
            ),
            "owner_bookkeeping_not_bounded"
        );
        drop(admitted);
        drop(held);
        assert!(
            store.stats()? == logical
                && ledger.used() == physical
                && support::counts(&store)? == counts,
            "owner_budget_denial_changed_existing_record"
        );
        if let Some(first) = first {
            first.activate()?;
        }
        let mut observer = store.admit("codex", Some(SESSION), 1)?;
        observer.resume_completed_context(&binding, Some("synthetic-bounded-owners"), &[], &[])?;
        Ok(())
    }

    #[test]
    fn hidden_duplicate_owner_count_is_bounded_by_metadata_budget() -> TestResult {
        owner_budget_case(false)
    }
    #[test]
    fn visible_duplicate_owner_count_is_bounded_by_metadata_budget() -> TestResult {
        owner_budget_case(true)
    }

    #[test]
    fn slot_growth_charges_both_indices_and_cancellation_retains_only_new_high_water() -> TestResult
    {
        let store = DlpTokenStore::new();
        let source = store.admit("codex", Some(SESSION), 1)?;
        let binding =
            source.bind_actual_attempt(PROVIDER, "openai_responses", TARGET, Some(PRINCIPAL))?;
        drop(source.prepare_context_for_test(&binding, "synthetic-growth-seed", &[], &[])?);
        let ledger = store.allocation_ledger_for_test()?;
        let logical = store.stats()?;
        let baseline = ledger.used();
        let first =
            source.prepare_context_for_test(&binding, "synthetic-growth-first", &[], &[])?;
        let slot = store.context_slot_bytes_for_test();
        let observed = ledger.clone();
        let before_growth = Rc::new(Cell::new(0));
        let retained_old = Rc::new(Cell::new(0));
        let (before, retained) = (before_growth.clone(), retained_old.clone());
        let hook = install(move |event| {
            if event.kind == Kind::Vector
                && event.phase == Phase::BeforeAllocation
                && event.bytes == 2 * slot
            {
                before.set(event.used.metadata);
                assert!(
                    observed.used().metadata == event.used.metadata,
                    "growth_new_backing_not_charged"
                );
            }
            if event.kind == Kind::Vector && event.phase == Phase::BeforeDrop && event.bytes == slot
            {
                retained.set(event.used.metadata);
            }
            Ok(())
        });
        let second =
            source.prepare_context_for_test(&binding, "synthetic-growth-second", &[], &[])?;
        drop(hook);
        assert!(
            before_growth.get() > 2 * slot && retained_old.get() == before_growth.get(),
            "growth_did_not_retain_overlapping_index_charge"
        );
        drop(first);
        drop(second);
        assert!(
            ledger.used().metadata == baseline.metadata + slot,
            "growth_cancellation_wrong_retained_high_water"
        );
        assert!(
            store.stats()? == logical,
            "growth_cancellation_leaked_logical_record_charge"
        );
        Ok(())
    }
}

mod c3_publication_transaction_lifetime {
    use super::super::{
        completed_context::CompletedCallSpec,
        token_store_owners::StoreAllocationLedger,
        tokenization::{publication_test_support as support, DlpTokenStore},
    };
    use super::*;

    const CURSOR: &str = "synthetic-lifetime-response";
    const OPAQUE: &str = "synthetic-lifetime-opaque";
    fn call() -> CompletedCallSpec<'static> {
        CompletedCallSpec {
            call_id: "synthetic-lifetime-call",
            item_type: "function_call",
            name: "synthetic_nonexecuting_tool",
            payload: "{}",
            reasoning: None,
            encrypted_reasoning: Some(OPAQUE),
        }
    }

    #[test]
    fn activation_releases_last_pin_after_all_request_leases_are_gone() -> TestResult {
        let store = DlpTokenStore::new();
        let lease = store.admit("codex", Some(SESSION), 1)?;
        let binding =
            lease.bind_actual_attempt(PROVIDER, "openai_responses", TARGET, Some(PRINCIPAL))?;
        let pending = lease.prepare_context_for_test(&binding, CURSOR, &[call()], &[OPAQUE])?;
        drop(lease);
        let future = std::time::Instant::now() + std::time::Duration::from_secs(3600);
        store.expire_idle_at(future)?;
        assert!(
            store.stats()?.namespaces == 1,
            "pending_activation_lost_pin"
        );
        pending.activate()?;
        assert!(
            support::counts(&store)? == (2, 0, 0, 0),
            "activation_retained_owner_pin"
        );
        store.expire_idle_at(future)?;
        assert!(
            store.stats()?.namespaces == 0,
            "activated_namespace_retained_expiry_pin"
        );
        Ok(())
    }

    #[test]
    fn mapped_nonempty_hidden_record_destruction_has_no_store_ownership_cycle() -> TestResult {
        let store = DlpTokenStore::new();
        let mut lease = store.admit("codex", Some(SESSION), 1)?;
        lease.prepare(
            &[TokenFragment {
                raw: "Synthetic nonempty ownership dependency",
                whole_span_categories: &[TokenCategory::Custom],
            }],
            &[],
            |_| Ok(()),
        )?;
        let binding =
            lease.bind_actual_attempt(PROVIDER, "openai_responses", TARGET, Some(PRINCIPAL))?;
        let pending = lease.prepare_context_for_test(&binding, CURSOR, &[call()], &[OPAQUE])?;
        let ledger = store.allocation_ledger_for_test()?;
        assert!(
            ledger.used().plaintext > 0,
            "nonempty_record_fixture_missing_dependency"
        );
        drop(lease);
        drop(store);
        drop(pending);
        assert!(
            ledger.used().plaintext == 0
                && ledger.used().metadata == StoreAllocationLedger::control_bytes()?,
            "nonempty_hidden_record_retained_ownership_cycle"
        );
        Ok(())
    }

    #[test]
    fn growing_table_preserves_hidden_owners_and_visible_mapping_call_opaque_authority(
    ) -> TestResult {
        let store = DlpTokenStore::new();
        let mut lease = store.admit("codex", Some(SESSION), 1)?;
        let reference = lease.prepare(
            &[TokenFragment {
                raw: "Synthetic migrated mapping dependency",
                whole_span_categories: &[TokenCategory::Custom],
            }],
            &[],
            |references| Ok(references[0].clone()),
        )?;
        let binding =
            lease.bind_actual_attempt(PROVIDER, "openai_responses", TARGET, Some(PRINCIPAL))?;
        lease
            .prepare_context_for_test(&binding, CURSOR, &[call()], &[OPAQUE])?
            .activate()?;
        let hidden =
            lease.prepare_context_for_test(&binding, "synthetic-lifetime-hidden", &[], &[])?;
        let duplicate =
            lease.prepare_context_for_test(&binding, "synthetic-lifetime-hidden", &[], &[])?;
        let grown =
            lease.prepare_context_for_test(&binding, "synthetic-lifetime-grown", &[], &[])?;
        let counts = support::counts(&store)?;
        assert!(
            counts.0 == 2 && counts.1 == 2 && counts.2 == 3,
            "table_growth_lost_pending_owners"
        );
        drop(hidden);
        let mut observer = store.admit("codex", Some(SESSION), 1)?;
        assert!(
            matches!(
                observer.resume_completed_context(
                    &binding,
                    Some("synthetic-lifetime-hidden"),
                    &[],
                    &[]
                ),
                Err(TokenError::ContextUnavailable)
            ),
            "table_growth_exposed_hidden_record"
        );
        observer.resume_completed_context(&binding, Some(CURSOR), &[call()], &[OPAQUE])?;
        assert!(
            observer.resolve(&reference).is_ok(),
            "table_growth_lost_mapping_dependency"
        );
        observer.verify_output_call_ids(&binding, Some(CURSOR), &[call().call_id])?;
        duplicate.activate()?;
        grown.activate()?;
        assert!(
            support::counts(&store)? == (4, 0, 0, 0),
            "table_growth_leaked_pending_reservation"
        );
        let mut resumed = store.admit("codex", Some(SESSION), 1)?;
        resumed.resume_completed_context(&binding, Some("synthetic-lifetime-hidden"), &[], &[])?;
        assert!(
            resumed.resolve(&reference).is_ok(),
            "migrated_pending_record_lost_dependency"
        );
        Ok(())
    }
}

mod c3_publication_transaction_rejection_cleanup {
    use super::super::{
        completed_context::{CompletedContextRecord, MAX_CONTEXT_ENTRIES_PER_NAMESPACE},
        token_store_owners::{
            arc_backing_bytes,
            test_support::{install, Kind, Phase},
        },
        tokenization::{publication_test_support as support, DlpTokenStore},
    };
    use super::*;
    use std::{cell::Cell, rc::Rc};

    fn allocation_case(kind: Kind, phase: Phase) -> TestResult {
        let store = DlpTokenStore::new();
        let source = store.admit("codex", Some(SESSION), 1)?;
        let binding =
            source.bind_actual_attempt(PROVIDER, "openai_responses", TARGET, Some(PRINCIPAL))?;
        let ledger = store.allocation_ledger_for_test()?;
        let logical = store.stats()?;
        let physical = ledger.used();
        let counts = support::counts(&store)?;
        let bytes = if kind == Kind::Shared {
            arc_backing_bytes::<CompletedContextRecord>()?
        } else {
            store.context_slot_bytes_for_test()
        };
        let ran = Rc::new(Cell::new(false));
        let denied = ran.clone();
        let hook = install(move |event| {
            if event.kind == kind && event.phase == phase && event.bytes == bytes {
                denied.set(true);
                return Err(TokenError::CapacityExceeded);
            }
            Ok(())
        });
        let pending =
            source.prepare_context_for_test(&binding, "synthetic-allocation-rejection", &[], &[]);
        drop(hook);
        assert!(ran.get(), "allocation_rejection_not_reached");
        assert!(
            matches!(pending, Err(TokenError::CapacityExceeded)),
            "allocation_rejection_bypassed"
        );
        assert!(
            store.stats()? == logical
                && ledger.used() == physical
                && support::counts(&store)? == counts,
            "allocation_rejection_leaked_owner_record_or_index"
        );
        source
            .prepare_context_for_test(&binding, "synthetic-allocation-rejection", &[], &[])?
            .activate()?;
        let mut observer = store.admit("codex", Some(SESSION), 1)?;
        observer.resume_completed_context(
            &binding,
            Some("synthetic-allocation-rejection"),
            &[],
            &[],
        )?;
        Ok(())
    }

    #[test]
    fn before_record_allocation_rejection_cleans_and_retry_succeeds() -> TestResult {
        allocation_case(Kind::Shared, Phase::BeforeAllocation)
    }
    #[test]
    fn after_record_allocation_rejection_cleans_and_retry_succeeds() -> TestResult {
        allocation_case(Kind::Shared, Phase::AfterAllocation)
    }
    #[test]
    fn before_index_allocation_rejection_cleans_and_retry_succeeds() -> TestResult {
        allocation_case(Kind::Vector, Phase::BeforeAllocation)
    }
    #[test]
    fn after_index_allocation_rejection_cleans_and_retry_succeeds() -> TestResult {
        allocation_case(Kind::Vector, Phase::AfterAllocation)
    }

    #[test]
    fn namespace_pending_capacity_denies_then_recovers_after_cancellation() -> TestResult {
        let store = DlpTokenStore::new();
        let source = store.admit("codex", Some(SESSION), 1)?;
        let binding =
            source.bind_actual_attempt(PROVIDER, "openai_responses", TARGET, Some(PRINCIPAL))?;
        let values: Vec<_> = (0..MAX_CONTEXT_ENTRIES_PER_NAMESPACE - 1)
            .map(|index| format!("synthetic-local-capacity-unit-{index}"))
            .collect();
        let refs: Vec<_> = values.iter().map(String::as_str).collect();
        let first = source.prepare_context_for_test(
            &binding,
            "synthetic-local-capacity-full",
            &[],
            &refs,
        )?;
        let ledger = store.allocation_ledger_for_test()?;
        let logical = store.stats()?;
        let physical = ledger.used();
        let counts = support::counts(&store)?;
        assert!(
            counts.1 == MAX_CONTEXT_ENTRIES_PER_NAMESPACE,
            "namespace_pending_capacity_not_reserved"
        );
        assert!(
            matches!(
                source.prepare_context_for_test(
                    &binding,
                    "synthetic-local-capacity-overflow",
                    &[],
                    &[]
                ),
                Err(TokenError::CapacityExceeded)
            ),
            "namespace_pending_capacity_overcommitted"
        );
        assert!(
            store.stats()? == logical
                && ledger.used() == physical
                && support::counts(&store)? == counts,
            "namespace_capacity_rejection_leaked_reservation"
        );
        drop(first);
        source
            .prepare_context_for_test(&binding, "synthetic-local-capacity-overflow", &[], &[])?
            .activate()?;
        assert!(
            support::counts(&store)? == (1, 0, 0, 0),
            "namespace_capacity_not_recovered"
        );
        Ok(())
    }

    #[test]
    fn activated_duplicate_still_pins_until_last_pending_owner_is_cancelled() -> TestResult {
        let store = DlpTokenStore::new();
        let source = store.admit("codex", Some(SESSION), 1)?;
        let binding =
            source.bind_actual_attempt(PROVIDER, "openai_responses", TARGET, Some(PRINCIPAL))?;
        let first =
            source.prepare_context_for_test(&binding, "synthetic-duplicate-expiry", &[], &[])?;
        let second =
            source.prepare_context_for_test(&binding, "synthetic-duplicate-expiry", &[], &[])?;
        drop(source);
        first.activate()?;
        let future = std::time::Instant::now() + std::time::Duration::from_secs(3600);
        store.expire_idle_at(future)?;
        assert!(
            support::counts(&store)?.2 == 1 && store.stats()?.namespaces == 1,
            "visible_record_ignored_pending_duplicate_pin"
        );
        drop(second);
        assert!(
            support::counts(&store)? == (1, 0, 0, 0),
            "visible_duplicate_cancel_changed_record"
        );
        store.expire_idle_at(future)?;
        assert!(
            store.stats()?.namespaces == 0,
            "last_visible_duplicate_cancel_retained_pin"
        );
        Ok(())
    }
}

#[tokio::test]
#[serial_test::serial]
async fn c3_sealed_delivery_unavailable_activation_returns_only_sanitized_stream_error(
) -> TestResult {
    c3_sse_publication_boundary_case(
        true,
        PublicationProbe::Cursor,
        PublicationDelivery::UnavailableAfterTerminal,
    )
    .await
}

#[tokio::test]
#[serial_test::serial]
async fn c3_sealed_client_sse_accepts_valid_data_only_native_frames() -> TestResult {
    let fixture = ResponsesFixture::new();
    let mut body = json!({"input": CONTACT, "stream": true});
    let attempt = fixture.prepare(&mut body, false, Some(SESSION))?;
    let response = json!({"id":"synthetic-data-only", "object":"response", "status":"completed",
        "output":[{"id":"synthetic-message", "type":"message", "role":"assistant",
            "status":"completed", "content":[{"type":"output_text", "text":body["input"]}]}]});
    let event = json!({"type":"response.completed", "response":response});
    let delivered = attempt
        .restore_client_http_response(
            Response::builder()
                .header("content-type", "text/event-stream")
                .body(Body::from(format!("data: {event}\n\n")))?,
            ArgumentNormalizationMode::Exact,
            None,
        )
        .await;
    let bytes = delivered.into_body().collect().await?.to_bytes();
    let text = std::str::from_utf8(&bytes)?;
    assert!(
        !text.contains("response.failed"),
        "data_only_native_completion_rejected"
    );
    assert!(
        text.contains("response.completed") && text.contains(CONTACT),
        "data_only_native_output_missing"
    );
    Ok(())
}

#[tokio::test]
#[serial_test::serial]
async fn c3_sealed_real_native_completion_rejects_missing_attempt_even_without_mappings(
) -> TestResult {
    use super::{
        http_restoration::wrap_sse_stream_with_restoration,
        response_restoration::SseRestorationState,
    };
    use futures::StreamExt;

    for input in [CONTACT, "Synthetic ordinary input"] {
        let fixture = ResponsesFixture::new();
        let mut body = json!({"input":input,"stream":true});
        let attempt = fixture.prepare(&mut body, false, Some(SESSION))?;
        let value = completed_response(body["input"].as_str().ok_or("synthetic_input_missing")?);
        let block = format!(
            "event: response.completed\ndata: {}\n\n",
            json!({"type":"response.completed","response":value})
        );
        // Deliberately bypass the HTTP attempt requirement, but not the real verifier.
        let state = SseRestorationState::new(
            attempt.lease,
            attempt.binding,
            attempt.budget,
            ArgumentNormalizationMode::Exact,
        )
        .map_err(|_| "synthetic_state_failed")?;
        let upstream = futures::stream::iter([Ok::<_, std::io::Error>(bytes::Bytes::from(block))]);
        let mut output = wrap_sse_stream_with_restoration(upstream, state);
        let mut bytes = Vec::new();
        while let Some(frame) = output.next().await {
            bytes.extend_from_slice(&frame?);
        }
        let text = std::str::from_utf8(&bytes)?;
        assert!(
            text.contains("response.failed") && !text.contains("response.completed"),
            "missing_attempt_real_completion_accepted"
        );
    }
    Ok(())
}

#[test]
#[serial_test::serial]
fn c3_sealed_real_donor_proof_rejects_wrong_attempt_and_framing() -> TestResult {
    use super::{
        http_restoration::VerifiedCompletion,
        restoration_core::AttemptProvenance,
        unified_test_support::{c3_completion_blocks, c3_completion_state},
    };
    for input in [CONTACT, "Synthetic ordinary input"] {
        for mode in ["matching", "other_attempt", "other_batch", "framing_only"] {
            let fixture = ResponsesFixture::new();
            let mut body = json!({"input":input,"stream":true});
            let request = fixture.admit(&mut body, false, Some(SESSION))?;
            let donor = prepare_attempt(&request, &mut body, PROVIDER, TARGET)?;
            let recipient = prepare_attempt(&request, &mut body, PROVIDER, TARGET)?;
            let provenance = AttemptProvenance::new(&donor.budget)?;
            let recipient_provenance = if mode == "other_attempt" {
                AttemptProvenance::new(&recipient.budget)?
            } else {
                provenance.clone()
            };
            let blocks =
                c3_completion_blocks(body["input"].as_str().ok_or("synthetic_input_missing")?);
            let donor = c3_completion_state(donor, provenance, None, &blocks)?;
            let mut other = blocks.clone();
            if mode == "other_batch" {
                other = c3_completion_blocks("Synthetic different batch");
            }
            if mode == "framing_only" {
                other.insert(0, ": synthetic framing comment\n\n".into());
            }
            let recipient = c3_completion_state(recipient, recipient_provenance, None, &other)?;
            // Every donor proof comes from paired raw/native observations and real transport EOF.
            let proof = VerifiedCompletion::verify_frames_for_test(&donor, &blocks, None)
                .map_err(|_| "synthetic_real_donor_failed")?;
            let result = recipient.finalize(proof);
            assert!(
                result.is_ok() == (mode == "matching"),
                "real_donor_attempt_or_batch_binding_lost"
            );
            drop(result);
        }
    }
    Ok(())
}

#[test]
#[serial_test::serial]
fn c3_sealed_real_original_verifier_rejects_same_attempt_wrong_protocol_and_reuse() -> TestResult {
    use super::{
        completion_json::JsonCompletionProtocol,
        http_restoration::{UpstreamSseEvidenceTracker, VerifiedCompletion},
        restoration_core::AttemptProvenance,
        unified_test_support::{c3_completion_blocks, c3_completion_state},
    };
    for input in [CONTACT, "Synthetic ordinary input"] {
        let fixture = ResponsesFixture::new();
        let mut body = json!({"input":input,"stream":true});
        let attempt = fixture.prepare(&mut body, false, Some(SESSION))?;
        let budget = attempt.budget.clone();
        let provenance = AttemptProvenance::new(&budget)?;
        let blocks = c3_completion_blocks(body["input"].as_str().ok_or("synthetic_input_missing")?);
        let state = c3_completion_state(
            attempt,
            provenance,
            Some(JsonCompletionProtocol::ChatCompletions),
            &blocks,
        )?;
        let original = json!({"id":"synthetic-p3-original","type":"message","role":"assistant","model":"synthetic-model",
            "content":[{"type":"text","text":"Synthetic ordinary output"}],"stop_reason":"end_turn","usage":{"input_tokens":1,"output_tokens":1}}).to_string();
        let mut tracker = UpstreamSseEvidenceTracker::anthropic(budget);
        tracker.bind_for_test(&state);
        tracker.observe_fully_read_anthropic_json(&original);
        assert!(
            VerifiedCompletion::verify_frames_for_test(&state, &blocks, Some(&mut tracker))
                .is_err(),
            "same_attempt_wrong_protocol_donor_accepted"
        );
        assert!(
            VerifiedCompletion::verify_frames_for_test(&state, &blocks, Some(&mut tracker))
                .is_err(),
            "consumed_original_tracker_reused"
        );
    }
    Ok(())
}

#[tokio::test]
#[serial_test::serial]
async fn c3_sealed_real_verifier_completion_and_delivery_are_one_shot() -> TestResult {
    use super::{
        completion_json::JsonCompletionProtocol,
        http_restoration::{UpstreamSseEvidenceTracker, VerifiedCompletion},
        restoration_core::AttemptProvenance,
        unified_test_support::{c3_completion_blocks, c3_completion_state},
    };
    use futures::StreamExt;
    let fixture = ResponsesFixture::new();
    let mut body = json!({"input":CONTACT,"stream":true});
    let attempt = fixture.prepare(&mut body, false, Some(SESSION))?;
    let budget = attempt.budget.clone();
    let provenance = AttemptProvenance::new(&budget)?;
    let blocks = c3_completion_blocks(body["input"].as_str().ok_or("synthetic_input_missing")?);
    let state = c3_completion_state(
        attempt,
        provenance,
        Some(JsonCompletionProtocol::AnthropicMessages),
        &blocks,
    )?;
    let original = json!({"id":"synthetic-p3-original","type":"message","role":"assistant","model":"synthetic-model",
        "content":[{"type":"text","text":"Synthetic ordinary output"}],"stop_reason":"end_turn","usage":{"input_tokens":1,"output_tokens":1}}).to_string();
    let mut tracker = UpstreamSseEvidenceTracker::anthropic(budget);
    tracker.bind_for_test(&state);
    tracker.observe_fully_read_anthropic_json(&original);
    let proof = VerifiedCompletion::verify_frames_for_test(&state, &blocks, Some(&mut tracker))
        .map_err(|_| "synthetic_real_completion_failed")?;
    assert!(
        VerifiedCompletion::verify_frames_for_test(&state, &blocks, Some(&mut tracker)).is_err(),
        "successful_tracker_consumed_twice"
    );
    let mut delivery = state
        .finalize(proof)
        .map_err(|_| "synthetic_finalization_failed")?
        .into_delivery_stream();
    let mut count = 0;
    while let Some(frame) = delivery.next().await {
        frame?;
        count += 1;
    }
    assert!(count > 0, "sealed_delivery_empty");
    assert!(
        delivery.next().await.is_none(),
        "sealed_delivery_completed_twice"
    );
    Ok(())
}

#[test]
#[serial_test::serial]
fn c3_sealed_real_verifier_rejects_repeated_native_terminal() -> TestResult {
    use super::{
        http_restoration::VerifiedCompletion,
        restoration_core::AttemptProvenance,
        unified_test_support::{c3_completion_blocks, c3_completion_state},
    };
    let fixture = ResponsesFixture::new();
    let mut body = json!({"input":"Synthetic ordinary input","stream":true});
    let attempt = fixture.prepare(&mut body, false, Some(SESSION))?;
    let provenance = AttemptProvenance::new(&attempt.budget)?;
    let mut blocks = c3_completion_blocks("Synthetic ordinary output");
    let state = c3_completion_state(attempt, provenance, None, &blocks)?;
    blocks.push(blocks[0].clone());
    assert!(
        VerifiedCompletion::verify_frames_for_test(&state, &blocks, None).is_err(),
        "repeated_native_terminal_verified"
    );
    Ok(())
}

#[tokio::test]
#[serial_test::serial]
async fn c3_sealed_allocation_denial_before_http_tool_release_cleans_up_and_recovers() -> TestResult
{
    use super::{
        completed_context::{CompletedCallSpec, CompletedContextRecord},
        token_store_owners::{
            arc_backing_bytes,
            test_support::{install, Kind, Phase},
        },
    };
    use std::{cell::Cell, rc::Rc};

    for mapped in [true, false] {
        for kind in [Kind::Shared, Kind::Vector] {
            for phase in [Phase::BeforeAllocation, Phase::AfterAllocation] {
                let fixture = ResponsesFixture::new();
                let mut body = json!({"input":if mapped { CONTACT } else { "Synthetic ordinary input" },"stream":true});
                let request = fixture.admit(&mut body, false, Some(SESSION))?;
                // Establish mapping backing without retaining an attempt lease in the baseline.
                let warmup = prepare_attempt(&request, &mut body, PROVIDER, TARGET)?;
                let payload_budget = warmup.budget.clone();
                drop(warmup);
                let payload_baseline = payload_budget.used();
                let logical = fixture.store.stats()?;
                let ledger = fixture.store.allocation_ledger_for_test()?;
                let physical = ledger.used();
                // Retry the same admitted request/store after rejection, not a fresh fixture.
                for deny in [true, false] {
                    let attempt = prepare_attempt(&request, &mut body, PROVIDER, TARGET)?;
                    let binding = attempt.binding.clone();
                    let request_budget = attempt.budget.clone();
                    let text = body["input"].as_str().ok_or("synthetic_input_missing")?;
                    let arguments = json!({"value":text}).to_string();
                    let delivered_arguments =
                        json!({"value":if mapped { CONTACT } else { text }}).to_string();
                    let item = json!({"id":"synthetic-p4-item","type":"function_call","call_id":"synthetic-p4-call",
                        "name":"synthetic-tool","arguments":arguments,"status":"completed","encrypted_reasoning":"synthetic-p4-opaque"});
                    let wire = format!("event: response.output_item.done\ndata: {}\n\nevent: response.completed\ndata: {}\n\n",
                        json!({"type":"response.output_item.done","output_index":0,"item":item}),
                        json!({"type":"response.completed","response":{"id":"synthetic-p4-response","object":"response","status":"completed","output":[item]}}));
                    let upstream = Response::builder()
                        .header("content-type", "text/event-stream")
                        .body(Body::from(wire))?;
                    let bytes = if kind == Kind::Shared {
                        arc_backing_bytes::<CompletedContextRecord>()?
                    } else {
                        fixture.store.context_slot_bytes_for_test()
                    };
                    let reached = Rc::new(Cell::new(false));
                    let observed = reached.clone();
                    let fault = install(move |event| {
                        if deny
                            && event.kind == kind
                            && event.phase == phase
                            && event.bytes == bytes
                        {
                            observed.set(true);
                            return Err(TokenError::CapacityExceeded);
                        }
                        Ok(())
                    });
                    // The hook stays live while the actual HTTP body runs its verifier and finalizer.
                    let response = attempt
                        .restore_client_http_response(
                            upstream,
                            ArgumentNormalizationMode::Exact,
                            None,
                        )
                        .await;
                    assert!(
                        response.status() == StatusCode::OK,
                        "synthetic_stream_status_changed"
                    );
                    let output = response.into_body().collect().await?.to_bytes();
                    drop(fault);
                    let output = std::str::from_utf8(&output)?;
                    assert!(reached.get() == deny, "sealed_allocation_fault_not_reached");
                    assert!(
                        output.contains("event: response.completed") != deny,
                        "sealed_allocation_terminal_boundary"
                    );
                    assert!(
                        output.contains("event: response.output_item.done") != deny,
                        "sealed_allocation_privileged_tool_released"
                    );
                    assert!(
                        output.contains("event: response.failed") == deny,
                        "sealed_allocation_failure_projection"
                    );
                    assert!(
                        !output.contains("rredact-cn_phone-"),
                        "sealed_allocation_reference_leaked"
                    );
                    assert!(
                        request_budget.used() == payload_baseline,
                        "sealed_allocation_request_owners_retained"
                    );
                    let call = CompletedCallSpec {
                        call_id: "synthetic-p4-call",
                        item_type: "function_call",
                        name: "synthetic-tool",
                        payload: &delivered_arguments,
                        reasoning: None,
                        encrypted_reasoning: Some("synthetic-p4-opaque"),
                    };
                    for probe in 0..4 {
                        let mut observer = fixture.store.admit_published_policy(
                            "codex",
                            Some(SESSION),
                            binding.policy_revision,
                        )?;
                        let result = match probe {
                            0 => observer.resume_completed_context(
                                &binding,
                                Some("synthetic-p4-response"),
                                &[],
                                &[],
                            ),
                            1 => observer.resume_completed_context(&binding, None, &[call], &[]),
                            2 => observer.resume_completed_context(
                                &binding,
                                None,
                                &[],
                                &["synthetic-p4-opaque"],
                            ),
                            _ => observer.verify_output_call_ids(
                                &binding,
                                Some("synthetic-p4-response"),
                                &["synthetic-p4-call"],
                            ),
                        };
                        assert!(
                            result.is_ok() != deny,
                            "sealed_allocation_published_authority"
                        );
                        if deny {
                            assert!(
                                matches!(result, Err(TokenError::ContextUnavailable)),
                                "sealed_allocation_unexpected_authority_error"
                            );
                        }
                        assert!(
                            observer.authorized_tokens()?.as_slice().len()
                                == usize::from(!deny && mapped && probe != 3),
                            "sealed_allocation_mapping_authority"
                        );
                    }
                    if deny {
                        let after = fixture.store.stats()?;
                        assert!(
                            after.entries == logical.entries
                                && after.metadata_bytes == logical.metadata_bytes
                                && ledger.used() == physical,
                            "sealed_allocation_store_charge_not_refunded"
                        );
                    }
                }
            }
        }
    }
    Ok(())
}

#[test]
#[serial_test::serial]
fn c3_shared_text_characterization_native_scopes_all_utf8_splits() -> TestResult {
    use super::{
        payload_budget::PayloadBudget,
        payload_metadata::PayloadVec,
        response_restoration::restore_text_with_lease,
        restoration_contract::{ClientProtocol, ContinuityScope, IngressNamespace},
    };
    use crate::proxy::protocol_bridge::Protocol;
    let fixture = ResponsesFixture::new();
    let revision = super::runtime::admit_security_runtime()?.restoration_policy_revision;
    for protocol in [Protocol::Chat, Protocol::Anthropic, Protocol::Gemini] {
        let mut lease = fixture.store.admit_scoped_published_policy(
            ContinuityScope::Native {
                client: ClientProtocol::new(protocol),
                ingress: IngressNamespace::Direct,
            },
            "synthetic-native-core",
            None,
            revision,
        )?;
        let budget = PayloadBudget::with_limit(1024 * 1024);
        let reference = lease.prepare_with_budget(
            &[TokenFragment {
                raw: CONTACT,
                whole_span_categories: &[TokenCategory::Custom],
            }],
            &[],
            budget.clone(),
            |references| Ok(references[0].as_str().to_owned()),
        )?;
        let input = format!("前🔒{reference}尾");
        let expected = format!("前🔒{CONTACT}尾");
        let baseline = budget.used();
        for split in (0..=input.len()).filter(|split| input.is_char_boundary(*split)) {
            let mut tokens = PayloadVec::new(budget.clone())?;
            let mut count = 0;
            let (safe, tail) = super::output_text_matching::split_prefix_and_tail(&input[..split]);
            let first = restore_text_with_lease(safe, &lease, &budget, &mut tokens, &mut count)
                .map_err(|_| "synthetic_native_prefix_failed")?;
            let remainder = format!("{tail}{}", &input[split..]);
            let second =
                restore_text_with_lease(&remainder, &lease, &budget, &mut tokens, &mut count)
                    .map_err(|_| "synthetic_native_suffix_failed")?;
            assert!(
                format!("{}{}", first.as_str(), second.as_str()) == expected,
                "native_shared_split_changed_text"
            );
            assert!(
                tokens.as_slice().len() == 1 && count == 1,
                "native_shared_dependency_or_occurrence_changed"
            );
            drop(first);
            drop(second);
            drop(tokens);
            assert!(
                budget.used() == baseline,
                "native_shared_split_charge_retained"
            );
        }
        let mut stranger = fixture.store.admit_scoped_published_policy(
            ContinuityScope::Native {
                client: ClientProtocol::new(protocol),
                ingress: IngressNamespace::Direct,
            },
            "synthetic-native-core",
            None,
            revision,
        )?;
        let mut tokens = PayloadVec::new(budget.clone())?;
        let mut count = 0;
        let unchanged =
            restore_text_with_lease(&reference, &stranger, &budget, &mut tokens, &mut count)
                .map_err(|_| "synthetic_native_literal_failed")?;
        assert!(
            unchanged.as_str() == reference && count == 0 && tokens.as_slice().is_empty(),
            "native_shared_syntax_granted_authority"
        );
        assert!(
            stranger.resolve(&reference).is_err(),
            "native_shared_foreign_authority"
        );
        // Keep the mutable stranger lease explicit: no history or pairing is used here.
        let _ = &mut stranger;
    }
    Ok(())
}

#[test]
#[serial_test::serial]
fn c3_shared_text_characterization_capacity_failure_cleans_temporary_owners() -> TestResult {
    use super::{
        payload_budget::PayloadBudget, payload_metadata::PayloadVec,
        response_restoration::restore_text_with_lease,
    };
    let fixture = ResponsesFixture::new();
    let mut body = json!({"input": CONTACT});
    let attempt = fixture.prepare(&mut body, false, None)?;
    let reference = body["input"]
        .as_str()
        .ok_or("synthetic_reference_missing")?;
    let budget = PayloadBudget::with_limit(0);
    let mut tokens = PayloadVec::new(budget.clone())?;
    let mut count = 0;
    assert!(
        restore_text_with_lease(reference, &attempt.lease, &budget, &mut tokens, &mut count)
            .is_err(),
        "native_shared_denied_budget_restored"
    );
    assert!(
        budget.used() == 0 && tokens.as_slice().is_empty() && count == 0,
        "native_shared_denial_retained_owner"
    );
    Ok(())
}

#[test]
#[serial_test::serial]
fn c3_shared_text_output_failure_preserves_dependency_and_occurrence_state() -> TestResult {
    use super::{
        payload_budget::PayloadBudget,
        payload_metadata::PayloadVec,
        payload_text::test_support::{self, Action, Phase},
        payload_text::PayloadText,
        restoration_core::restore_text,
    };
    use std::{cell::Cell, rc::Rc};
    let fixture = ResponsesFixture::new();
    let mut body = json!({"input": CONTACT});
    let attempt = fixture.prepare(&mut body, false, None)?;
    let reference = body["input"]
        .as_str()
        .ok_or("synthetic_reference_missing")?;
    for phase in [
        Phase::BeforeAllocation,
        Phase::AfterAllocation,
        Phase::BeforeCopy,
    ] {
        let budget = PayloadBudget::with_limit(64 * 1024);
        let mut tokens = PayloadVec::new(budget.clone())?;
        tokens.push(PayloadText::try_from_str(
            "synthetic-existing-dependency",
            budget.clone(),
            64,
        )?)?;
        let baseline = budget.used();
        let mut count = 7;
        let reached = Rc::new(Cell::new(false));
        let observed = reached.clone();
        let hook = test_support::install(move |event| {
            if event.phase == phase && event.bytes == CONTACT.len() {
                observed.set(true);
                Action::Fail
            } else {
                Action::Continue
            }
        });
        let result = restore_text(reference, &attempt.lease, &budget, &mut tokens, &mut count);
        drop(hook);
        assert!(
            reached.get() && result.is_err(),
            "shared_text_output_fault_not_reached"
        );
        drop(result);
        assert!(
            tokens.as_slice().len() == 1 && count == 7,
            "shared_text_failure_committed_dependency_or_occurrence"
        );
        assert!(
            budget.used() == baseline,
            "shared_text_failure_retained_temporary_owner"
        );
        let restored = restore_text(reference, &attempt.lease, &budget, &mut tokens, &mut count)
            .map_err(|_| "synthetic_shared_text_retry_failed")?;
        assert!(
            restored.as_str() == CONTACT && count == 8 && tokens.as_slice().len() == 2,
            "shared_text_retry_did_not_commit"
        );
        drop(restored);
        drop(tokens);
        assert!(budget.used() == 0, "shared_text_retry_owner_retained");
    }
    Ok(())
}

#[test]
#[serial_test::serial]
fn c3_stream_text_characterization_all_splits_and_snapshot_conflicts() -> TestResult {
    use super::response_restoration::SseRestorationState;
    let fixture = ResponsesFixture::new();
    for mismatch in [false, true] {
        let mut request_body = json!({"input": CONTACT});
        let request = fixture.admit(&mut request_body, false, None)?;
        let attempt = prepare_attempt(&request, &mut request_body, PROVIDER, TARGET)?;
        let reference = request_body["input"]
            .as_str()
            .ok_or("synthetic_reference_missing")?;
        let raw = format!("前🔒{reference}尾");
        let expected = format!("前🔒{CONTACT}尾");
        for split in (0..=raw.len()).filter(|split| raw.is_char_boundary(*split)) {
            let mut body = json!({"input": CONTACT});
            let attempt = prepare_attempt(&request, &mut body, PROVIDER, TARGET)?;
            let budget = attempt.budget.clone();
            let mut state = SseRestorationState::new(
                attempt.lease,
                attempt.binding,
                budget.clone(),
                super::response_restoration::ArgumentNormalizationMode::Exact,
            )
            .map_err(|_| "synthetic_stream_state_failed")?;
            let mut delivered = String::new();
            for delta in [&raw[..split], &raw[split..]] {
                let frames = state.process_sse_block(&format!(
                    "event: response.output_text.delta\ndata: {}\n\n",
                    json!({"type":"response.output_text.delta","output_index":0,"content_index":0,"delta":delta})
                )).map_err(|_| "synthetic_stream_delta_failed")?;
                for frame in frames {
                    let frame = std::str::from_utf8(&frame)?;
                    for line in frame.lines().filter_map(|line| line.strip_prefix("data: ")) {
                        let value: serde_json::Value = serde_json::from_str(line)?;
                        delivered
                            .push_str(value["delta"].as_str().ok_or("synthetic_delta_missing")?);
                    }
                }
            }
            assert!(
                delivered == expected,
                "stream_characterization_split_changed_text"
            );
            assert!(
                state.restored_occurrences() == 1,
                "stream_characterization_occurrence_changed"
            );
            let done = if mismatch {
                "synthetic-conflicting-snapshot"
            } else {
                raw.as_str()
            };
            let result = state.process_sse_block(&format!(
                "event: response.output_text.done\ndata: {}\n\n",
                json!({"type":"response.output_text.done","output_index":0,"content_index":0,"text":done})
            ));
            assert!(
                result.is_ok() != mismatch,
                "stream_characterization_snapshot_conflict_changed"
            );
            drop(result);
            drop(state);
        }
        drop(attempt);
    }
    Ok(())
}

#[test]
fn c3_stream_text_observation_failure_preserves_prior_state() -> TestResult {
    use super::{
        payload_budget::PayloadBudget,
        payload_buffer::test_support::{self, Action, Phase},
        restoration_core::StreamTextState,
    };
    use std::{cell::Cell, rc::Rc};
    for phase in [
        Phase::BeforeAllocation,
        Phase::CapacityObserved,
        Phase::BeforeCopy,
    ] {
        let budget = PayloadBudget::with_limit(64 * 1024);
        let mut state = StreamTextState::new(budget.clone(), 1024)
            .map_err(|_| "synthetic_stream_state_failed")?;
        drop(
            state
                .observe_delta("prior", budget.clone(), 1024)
                .map_err(|_| "synthetic_stream_observation_failed")?,
        );
        state
            .commit_delta("rr", "prior", budget.clone())
            .map_err(|_| "synthetic_stream_commit_failed")?;
        let baseline = budget.used();
        let reached = Rc::new(Cell::new(false));
        let observed = reached.clone();
        let hook = test_support::install(move |event| {
            if event.phase == phase && event.requested == 5 {
                observed.set(true);
                Action::FailAllocation
            } else {
                Action::Continue
            }
        });
        let result = state.observe_delta("xyz", budget.clone(), 1024);
        drop(hook);
        assert!(
            reached.get() && result.is_err(),
            "stream_observation_fault_not_reached"
        );
        drop(result);
        assert!(
            state.saw_deltas
                && state.raw_deltas.as_slice() == b"prior"
                && state.pending_tail.as_slice() == b"rr"
                && state.restored_deltas.as_slice() == b"prior",
            "stream_observation_failure_changed_prior_state"
        );
        assert!(
            budget.used() == baseline,
            "stream_observation_failure_retained_owner"
        );
        let (combined, pending) = state
            .observe_delta("xyz", budget.clone(), 1024)
            .map_err(|_| "synthetic_stream_retry_failed")?;
        assert!(
            combined.as_slice() == b"rrxyz"
                && pending == 2
                && state.raw_deltas.as_slice() == b"priorxyz",
            "stream_observation_retry_failed"
        );
        drop(combined);
        drop(state);
        assert!(budget.used() == 0, "stream_observation_drop_retained_owner");
    }
    Ok(())
}

#[test]
fn c3_stream_text_commit_failure_preserves_prior_tail_and_output() -> TestResult {
    use super::{
        payload_budget::PayloadBudget,
        payload_buffer::test_support::{self, Action, Phase},
        restoration_core::StreamTextState,
    };
    use std::{cell::Cell, rc::Rc};
    for phase in [
        Phase::BeforeAllocation,
        Phase::CapacityObserved,
        Phase::BeforeCopy,
    ] {
        let budget = PayloadBudget::with_limit(64 * 1024);
        let mut state = StreamTextState::new(budget.clone(), 1024)
            .map_err(|_| "synthetic_stream_state_failed")?;
        state
            .commit_delta("rr", "prior", budget.clone())
            .map_err(|_| "synthetic_stream_commit_failed")?;
        let baseline = budget.used();
        let reached = Rc::new(Cell::new(false));
        let observed = reached.clone();
        let hook = test_support::install(move |event| {
            if event.phase == phase && event.requested == 9 {
                observed.set(true);
                Action::FailAllocation
            } else {
                Action::Continue
            }
        });
        let result = state.commit_delta("rrr", "next", budget.clone());
        drop(hook);
        assert!(
            reached.get() && result.is_err(),
            "stream_commit_fault_not_reached"
        );
        assert!(
            state.pending_tail.as_slice() == b"rr" && state.restored_deltas.as_slice() == b"prior",
            "stream_commit_failure_changed_prior_tail_or_output"
        );
        assert!(
            budget.used() == baseline,
            "stream_commit_failure_retained_owner"
        );
        state
            .commit_delta("rrr", "next", budget.clone())
            .map_err(|_| "synthetic_stream_commit_retry_failed")?;
        assert!(
            state.pending_tail.as_slice() == b"rrr"
                && state.restored_deltas.as_slice() == b"priornext",
            "stream_commit_retry_failed"
        );
        drop(state);
        assert!(budget.used() == 0, "stream_commit_drop_retained_owner");
    }
    Ok(())
}

#[test]
fn c3_tool_barrier_rejection_preserves_activation_and_byte_state() -> TestResult {
    use super::{payload_budget::PayloadBudget, restoration_core::ToolBarrier};
    let budget = PayloadBudget::with_limit(4096);
    let mut barrier = ToolBarrier::new(budget.clone(), 8, 2)?;
    assert!(
        barrier.hold(1u8, 9).is_err(),
        "barrier_oversize_not_rejected"
    );
    assert!(
        !barrier.is_active() && barrier.retained_bytes() == 0 && barrier.len() == 0,
        "barrier_rejection_changed_empty_state"
    );
    barrier.hold(2u8, 3)?;
    barrier.hold(3u8, 3)?;
    let baseline = budget.used();
    assert!(
        barrier.hold(4u8, 1).is_err(),
        "barrier_unit_limit_not_rejected"
    );
    assert!(
        barrier.is_active() && barrier.retained_bytes() == 6 && barrier.units() == [2, 3],
        "barrier_rejection_changed_retained_state"
    );
    assert!(
        budget.used() == baseline,
        "barrier_rejection_retained_charge"
    );
    drop(barrier);
    assert!(budget.used() == 0, "barrier_abort_retained_charge");
    Ok(())
}

#[test]
fn c3_tool_barrier_metadata_failure_rolls_back_and_retries() -> TestResult {
    use super::{
        payload_budget::PayloadBudget,
        payload_metadata::test_support::{self, Action, Kind, Phase},
        payload_text::PayloadText,
        restoration_core::ToolBarrier,
    };
    use std::{cell::Cell, rc::Rc};
    for phase in [Phase::BeforeAllocation, Phase::AfterAllocation] {
        let budget = PayloadBudget::with_limit(4096);
        let mut barrier = ToolBarrier::new(budget.clone(), 100, 4)?;
        let reached = Rc::new(Cell::new(false));
        let observed = reached.clone();
        let hook = test_support::install(move |event| {
            if event.kind == Kind::Vector && event.phase == phase {
                observed.set(true);
                Action::Fail
            } else {
                Action::Continue
            }
        });
        let owner = PayloadText::try_from_str("synthetic-unit", budget.clone(), 100)?;
        let result = barrier.hold(owner, 14);
        drop(hook);
        assert!(
            reached.get() && result.is_err(),
            "barrier_metadata_fault_not_reached"
        );
        assert!(
            !barrier.is_active() && barrier.retained_bytes() == 0 && barrier.len() == 0,
            "barrier_metadata_failure_changed_state"
        );
        assert!(
            budget.used() == 0,
            "barrier_metadata_failure_retained_owner"
        );
        barrier.hold(
            PayloadText::try_from_str("synthetic-unit", budget.clone(), 100)?,
            14,
        )?;
        assert!(
            barrier.is_active() && barrier.retained_bytes() == 14 && barrier.len() == 1,
            "barrier_metadata_retry_failed"
        );
        drop(barrier);
        assert!(budget.used() == 0, "barrier_metadata_abort_retained_owner");
    }
    Ok(())
}

#[test]
fn c3_stream_snapshot_failure_is_atomic_and_retryable() -> TestResult {
    use super::{
        payload_budget::PayloadBudget,
        payload_text::test_support::{self, Action, Phase},
        restoration_core::StreamTextState,
    };
    use std::{cell::Cell, rc::Rc};
    for phase in [
        Phase::BeforeAllocation,
        Phase::AfterAllocation,
        Phase::BeforeCopy,
    ] {
        let budget = PayloadBudget::with_limit(4096);
        let mut state = StreamTextState::new(budget.clone(), 1024)
            .map_err(|_| "synthetic_snapshot_state_failed")?;
        let reached = Rc::new(Cell::new(false));
        let observed = reached.clone();
        let hook = test_support::install(move |event| {
            if event.phase == phase && event.bytes == 8 {
                observed.set(true);
                Action::Fail
            } else {
                Action::Continue
            }
        });
        let result = state.record_done("raw", "restored", budget.clone(), 1024);
        drop(hook);
        assert!(
            reached.get() && result.is_err(),
            "snapshot_fault_not_reached"
        );
        assert!(
            state.raw_done.is_none() && state.restored_done.is_none(),
            "snapshot_failure_committed_partial_owner"
        );
        assert!(budget.used() == 0, "snapshot_failure_retained_charge");
        state
            .validate_done("raw")
            .map_err(|_| "synthetic_snapshot_validation_failed")?;
        assert!(
            !state
                .record_done("raw", "restored", budget.clone(), 1024)
                .map_err(|_| "synthetic_snapshot_retry_failed")?,
            "snapshot_retry_invented_delta"
        );
        assert!(
            state.validate_done("raw").is_err(),
            "snapshot_duplicate_accepted"
        );
        drop(state);
        assert!(budget.used() == 0, "snapshot_abort_retained_charge");
    }
    Ok(())
}

#[test]
fn c3_tool_barrier_adopt_rejects_nonempty_destination_without_displacing_owners() -> TestResult {
    use super::{payload_budget::PayloadBudget, restoration_core::ToolBarrier};
    let budget = PayloadBudget::with_limit(4096);
    let mut barrier = ToolBarrier::new(budget.clone(), 8, 2)?;
    barrier.hold(11u8, 3)?;
    let mut donor = barrier.deferred_owner()?;
    donor.push(22u8, 2)?;
    let baseline = budget.used();
    assert!(
        barrier.adopt_deferred(&mut donor).is_err(),
        "c3_barrier_adopt_nonempty_not_rejected"
    );
    assert!(
        barrier.units() == [11] && barrier.retained_bytes() == 3 && barrier.is_active(),
        "c3_barrier_adopt_displaced_destination_owner"
    );
    assert!(
        donor.units() == [22] && donor.retained_bytes() == 2 && budget.used() == baseline,
        "c3_barrier_adopt_mutated_rejected_donor"
    );
    drop(donor);
    drop(barrier);
    assert!(
        budget.used() == 0,
        "c3_barrier_adopt_rejection_drop_retained_charge"
    );
    Ok(())
}

#[test]
fn c3_tool_barrier_adopt_rejects_excess_bytes_before_transfer() -> TestResult {
    use super::{payload_budget::PayloadBudget, restoration_core::ToolBarrier};
    let budget = PayloadBudget::with_limit(4096);
    let mut barrier = ToolBarrier::new(budget.clone(), 8, 2)?;
    let mut donor = barrier.deferred_owner()?;
    // Test-only malformed metadata reaches the actual transfer, not producer refusal.
    donor.push_unchecked_for_test(22u8, 9)?;
    let baseline = budget.used();
    assert!(
        barrier.adopt_deferred(&mut donor).is_err(),
        "c3_barrier_adopt_excess_bytes_not_rejected"
    );
    assert!(
        barrier.len() == 0 && barrier.retained_bytes() == 0 && !barrier.is_active(),
        "c3_barrier_adopt_accepted_excess_bytes"
    );
    assert!(
        donor.units() == [22] && donor.retained_bytes() == 9 && budget.used() == baseline,
        "c3_barrier_adopt_excess_bytes_lost_owner"
    );
    drop(donor);
    drop(barrier);
    assert!(
        budget.used() == 0,
        "c3_barrier_adopt_excess_bytes_drop_retained_charge"
    );
    Ok(())
}

#[test]
fn c3_tool_barrier_adopt_rejects_excess_units_before_transfer() -> TestResult {
    use super::{payload_budget::PayloadBudget, restoration_core::ToolBarrier};
    let budget = PayloadBudget::with_limit(4096);
    let mut barrier = ToolBarrier::new(budget.clone(), 8, 1)?;
    let mut donor = barrier.deferred_owner()?;
    donor.push(22u8, 1)?;
    donor.push_unchecked_for_test(33u8, 1)?;
    let baseline = budget.used();
    assert!(
        barrier.adopt_deferred(&mut donor).is_err(),
        "c3_barrier_adopt_excess_units_not_rejected"
    );
    assert!(
        barrier.len() == 0 && barrier.retained_bytes() == 0 && !barrier.is_active(),
        "c3_barrier_adopt_accepted_excess_units"
    );
    assert!(
        donor.units() == [22, 33] && donor.retained_bytes() == 2 && budget.used() == baseline,
        "c3_barrier_adopt_excess_units_lost_owner"
    );
    drop(donor);
    drop(barrier);
    assert!(
        budget.used() == 0,
        "c3_barrier_adopt_excess_units_drop_retained_charge"
    );
    Ok(())
}

#[test]
fn c3_tool_barrier_adopt_empty_active_destination_preserves_charge_characterization() -> TestResult
{
    use super::{payload_budget::PayloadBudget, restoration_core::ToolBarrier};
    let budget = PayloadBudget::with_limit(4096);
    let mut barrier = ToolBarrier::new(budget.clone(), 8, 2)?;
    barrier.hold_all();
    let mut donor = barrier.deferred_owner()?;
    donor.push(22u8, 3)?;
    donor.push(33u8, 5)?;
    let baseline = budget.used();
    barrier.adopt_deferred(&mut donor)?;
    assert!(
        barrier.units() == [22, 33] && barrier.retained_bytes() == 8 && barrier.is_active(),
        "c3_barrier_adopt_active_empty_control_rejected"
    );
    assert!(
        donor.len() == 0 && donor.retained_bytes() == 0 && budget.used() == baseline,
        "c3_barrier_adopt_control_changed_charge"
    );
    drop(donor);
    drop(barrier);
    assert!(
        budget.used() == 0,
        "c3_barrier_adopt_control_drop_retained_charge"
    );
    Ok(())
}

#[test]
fn c3_tool_barrier_adopt_rejects_foreign_budget_without_moving_owners() -> TestResult {
    use super::{payload_budget::PayloadBudget, restoration_core::ToolBarrier};
    let destination_budget = PayloadBudget::with_limit(4096);
    let donor_budget = PayloadBudget::with_limit(4096);
    let mut barrier = ToolBarrier::new(destination_budget.clone(), 8, 2)?;
    let mut donor_barrier = ToolBarrier::new(donor_budget.clone(), 8, 2)?;
    let mut donor = donor_barrier.deferred_owner()?;
    donor.push(22u8, 2)?;
    let destination_baseline = destination_budget.used();
    let donor_baseline = donor_budget.used();
    assert!(
        barrier.adopt_deferred(&mut donor).is_err(),
        "c3_barrier_adopt_foreign_budget_not_rejected"
    );
    assert!(
        barrier.len() == 0 && barrier.retained_bytes() == 0 && !barrier.is_active(),
        "c3_barrier_adopt_accepted_foreign_budget_owner"
    );
    assert!(
        donor.units() == [22] && donor.retained_bytes() == 2,
        "c3_barrier_adopt_foreign_budget_donor_displaced"
    );
    assert!(
        destination_budget.used() == destination_baseline && donor_budget.used() == donor_baseline,
        "c3_barrier_adopt_foreign_budget_changed_charge"
    );
    drop(donor);
    drop(donor_barrier);
    drop(barrier);
    assert!(
        destination_budget.used() == 0 && donor_budget.used() == 0,
        "c3_barrier_adopt_foreign_budget_drop_retained_charge"
    );
    Ok(())
}

// CH2 integrator amendment: production HTTP proof and sealed native delivery.
fn ch2_core_json_choices(reference: &str) -> Value {
    json!({"id":"synthetic-ch2-response","object":"chat.completion","choices":[
        {"index":7,"message":{"role":"assistant","content":reference},"finish_reason":"stop"},
        {"index":2,"message":{"role":"assistant","content":reference},"finish_reason":"stop"}
    ],"extension":{"types":[true,null,7]}})
}

fn ch2_core_json_proof(
    fixture: &super::native_test_support::NativeFixture,
    source: &Value,
) -> Result<super::http_restoration::VerifiedNativeJson, super::response_restoration::RestoreError>
{
    use super::native_test_support::{ScriptedSource, Step};
    use bytes::Bytes;
    let raw = serde_json::to_vec(source)
        .map_err(|_| super::response_restoration::RestoreError::RestoreFailed)?;
    let (transport, stats) = ScriptedSource::new([
        Step::Pending,
        Step::Bytes(Bytes::from(raw)),
        Step::Bytes(Bytes::new()),
        Step::Pending,
    ]);
    let result = futures::executor::block_on(fixture.json_proof(source, transport));
    assert!(stats.eof.get() == 1, "ch2_core_json_actual_eof_missing");
    assert!(stats.pending.get() == 2, "ch2_core_json_pending_not_driven");
    result
}

#[test]
fn ch2_core_json_multiple_choices_reach_sealed_delivery() -> TestResult {
    use super::{
        native_test_support::{chat, record_delivery, NativeFixture, MAX_ENTITY, TEXT},
        response_restoration::finalize_native_json,
        restoration_contract::{
            EditConstraint, ProtocolPair, SourceClass, TextSites, UpstreamProtocol,
        },
        restoration_core::{stage_native_json_with_lifecycle, NativeJsonObservation},
    };
    use crate::proxy::protocol_bridge::Protocol;
    use bytes::Bytes;
    for mapped in [false, true] {
        let fixture = NativeFixture::new(mapped)?;
        let source = ch2_core_json_choices(&fixture.reference);
        let wire = Bytes::from(serde_json::to_vec(&source)?);
        let proof = ch2_core_json_proof(&fixture, &source);
        assert!(
            proof.is_ok(),
            "ch2_core_native_json_multiple_choices_rejected"
        );
        let proof = proof.map_err(|_| "ch2_core_json_proof_rejected")?;
        assert!(
            fixture.counts()? == (0, 0, 0),
            "ch2_core_inspection_published"
        );
        let mut sites = TextSites::new(fixture.budget.clone())?;
        for choice in source["choices"]
            .as_array()
            .ok_or("ch2_core_choices_missing")?
        {
            sites.observe(
                &choice["message"]["content"],
                SourceClass::HistoricalText,
                EditConstraint::Editable,
            )?;
        }
        let delivered = [
            &source["choices"][0]["message"],
            &source["choices"][1]["message"],
        ];
        let observation =
            NativeJsonObservation::new(&source, chat(), sites, &delivered, fixture.budget.clone())
                .map_err(|_| "ch2_core_json_observation_rejected")?;
        let staged = stage_native_json_with_lifecycle(
            &observation,
            &fixture.lease,
            &fixture.binding,
            ProtocolPair::new(chat(), UpstreamProtocol::new(Protocol::Chat)),
            &fixture.attempt,
        )
        .map_err(|_| "ch2_core_json_stage_rejected")?;
        assert!(
            staged.output_tokens().len() == usize::from(mapped),
            "ch2_core_json_ledger_not_unique"
        );
        assert!(
            staged.restored_occurrences() == 2 * usize::from(mapped),
            "ch2_core_json_occurrences_lost"
        );
        let finalized = finalize_native_json(
            staged,
            &fixture.lease,
            proof,
            &fixture.attempt,
            wire.clone(),
            MAX_ENTITY,
        )
        .map_err(|_| "ch2_core_json_finalization_rejected")?;
        let chunks =
            futures::executor::block_on(record_delivery(finalized.into_delivery_stream()))?;
        assert!(chunks.len() == 1, "ch2_core_json_delivery_count");
        let actual: Value = serde_json::from_slice(&chunks[0])?;
        assert!(
            actual["extension"] == source["extension"],
            "ch2_core_json_extension_changed"
        );
        for (index, choice) in actual["choices"]
            .as_array()
            .ok_or("ch2_core_delivered_choices_missing")?
            .iter()
            .enumerate()
        {
            assert!(
                choice["index"] == source["choices"][index]["index"],
                "ch2_core_json_choice_reindexed"
            );
            assert!(
                choice["message"]["content"].as_str()
                    == Some(if mapped { TEXT } else { &fixture.reference }),
                "ch2_core_json_choice_not_restored"
            );
        }
        if !mapped {
            assert!(chunks[0] == wire, "ch2_core_json_zero_map_bytes_changed");
        }
        let snapshot = json!([
            actual["choices"][0]["message"],
            actual["choices"][1]["message"]
        ]);
        assert!(
            fixture
                .lease
                .verify_native_delivered(&fixture.binding, chat(), &snapshot)
                .is_ok(),
            "ch2_core_json_ordered_history_missing"
        );
        drop(observation);
        assert!(
            fixture.budget.used() <= super::native_test_support::LIMIT,
            "ch2_core_json_budget_exceeded"
        );
    }
    Ok(())
}

#[test]
fn ch2_core_json_multiple_tool_choices_keep_independent_identity() -> TestResult {
    use super::native_test_support::NativeFixture;
    let fixture = NativeFixture::new(false)?;
    let mut source = ch2_core_json_choices("Synthetic public text");
    for choice in source["choices"]
        .as_array_mut()
        .ok_or("ch2_core_choices_missing")?
    {
        choice["finish_reason"] = json!("tool_calls");
        choice["message"]["tool_calls"] = json!([{"id":"synthetic-shared-choice-local-id","type":"function","function":{"name":"synthetic-tool","arguments":"{\"value\":\"synthetic\"}"}}]);
    }
    let proof = ch2_core_json_proof(&fixture, &source);
    assert!(
        proof.is_ok(),
        "ch2_core_native_json_choice_local_tools_rejected"
    );
    assert!(
        fixture.counts()? == (0, 0, 0),
        "ch2_core_json_proof_granted_publication"
    );
    Ok(())
}

#[test]
fn ch2_core_json_invalid_index_or_choice_never_proves_completion() -> TestResult {
    use super::native_test_support::NativeFixture;
    for case in 0..9 {
        let fixture = NativeFixture::new(false)?;
        let mut source = ch2_core_json_choices("Synthetic public text");
        match case {
            0 => source["choices"][1]["index"] = json!(7),
            1 => {
                source["choices"][0]
                    .as_object_mut()
                    .ok_or("ch2_core_choice_missing")?
                    .remove("index");
            }
            2 => source["choices"][1]["index"] = json!(-1),
            3 => source["choices"][1]["index"] = json!(1.5),
            4 => source["choices"][1]["index"] = json!(128),
            5 => source["choices"][1]["finish_reason"] = json!("length"),
            6 => source["choices"][1]["finish_reason"] = Value::Null,
            7 => {
                source["choices"][1]["message"]["function_call"] =
                    json!({"name":"synthetic-tool","arguments":"{}"})
            }
            _ => source["choices"][1]["message"]["role"] = json!("user"),
        }
        assert!(
            ch2_core_json_proof(&fixture, &source).is_err(),
            "ch2_core_invalid_json_choice_accepted"
        );
        assert!(
            fixture.counts()? == (0, 0, 0),
            "ch2_core_invalid_json_choice_published"
        );
    }
    Ok(())
}

#[test]
fn ch2_core_json_conversion_mode_remains_single_choice() -> TestResult {
    use super::{
        completion_json::{JsonCompletion, JsonCompletionProtocol},
        payload_budget::PayloadBudget,
    };
    let source = ch2_core_json_choices("Synthetic public text");
    let raw = serde_json::to_string(&source)?;
    let mut converted = JsonCompletion::new(JsonCompletionProtocol::ChatCompletions);
    converted.observe_raw_json(&raw, &PayloadBudget::with_limit(1024 * 1024));
    assert!(
        !converted.permits_completion(),
        "ch2_core_conversion_multi_choice_relaxed"
    );
    Ok(())
}

fn ch2_core_sse_chunks(tools: bool) -> Vec<Value> {
    let first = if tools {
        json!({"index":9,"delta":{"role":"assistant","tool_calls":[{"index":5,"id":"synthetic-ch2-call","type":"function","function":{"name":"synthetic-tool","arguments":"{\"value\":"}}]},"finish_reason":null})
    } else {
        json!({"index":9,"delta":{"role":"assistant","content":"Synthetic first"},"finish_reason":null})
    };
    let second = json!({"index":2,"delta":{"role":"assistant","content":"Synthetic second"},"finish_reason":null});
    let last = if tools {
        json!({"index":9,"delta":{"tool_calls":[{"index":5,"function":{"arguments":"\"synthetic\"}"}}]},"finish_reason":"tool_calls"})
    } else {
        json!({"index":9,"delta":{"content":" complete"},"finish_reason":"stop"})
    };
    vec![
        json!({"id":"synthetic-ch2-sse","choices":[first,second]}),
        json!({"id":"synthetic-ch2-sse","choices":[{"index":2,"delta":{},"finish_reason":"stop"}]}),
        json!({"id":"synthetic-ch2-sse","choices":[last]}),
        json!({"id":"synthetic-ch2-sse","choices":[],"usage":{"completion_tokens":7}}),
    ]
}

fn ch2_core_sse_wire(chunks: &[Value], done: bool) -> String {
    let mut wire = chunks
        .iter()
        .map(|chunk| format!("data: {chunk}\n\n"))
        .collect::<String>();
    if done {
        wire.push_str("data: [DONE]\n\n");
    }
    wire
}

fn ch2_core_sse_proof(
    fixture: &super::native_test_support::NativeFixture,
    wire: &str,
) -> Result<super::http_restoration::VerifiedNativeSse, super::response_restoration::RestoreError> {
    use super::{http_restoration::test_transport, native_test_support::ScriptedSource};
    use bytes::Bytes;
    let raw = Bytes::copy_from_slice(wire.as_bytes());
    let (original, original_stats) = ScriptedSource::split(raw.clone(), &[1, raw.len() / 2])
        .map_err(|_| super::response_restoration::RestoreError::RestoreFailed)?;
    let (client, client_stats) = ScriptedSource::split(raw, &[2, wire.len() / 3])
        .map_err(|_| super::response_restoration::RestoreError::RestoreFailed)?;
    let proof = futures::executor::block_on(test_transport::native_sse_from_streams(
        &fixture.attempt,
        &fixture.binding,
        &fixture.budget,
        original,
        client,
    ));
    assert!(
        original_stats.eof.get() == 1,
        "ch2_core_sse_original_eof_missing"
    );
    if proof.is_ok() {
        assert!(
            client_stats.eof.get() == 1,
            "ch2_core_sse_client_eof_missing"
        );
        assert!(
            original_stats.pending.get() > 0 && client_stats.pending.get() > 0,
            "ch2_core_sse_pending_missing"
        );
    }
    proof
}

#[test]
fn ch2_core_sse_interleaved_choices_require_independent_terminals() -> TestResult {
    use super::native_test_support::{chat, NativeFixture};
    let fixture = NativeFixture::new(false)?;
    let wire = ch2_core_sse_wire(&ch2_core_sse_chunks(false), true);
    let proof = ch2_core_sse_proof(&fixture, &wire);
    assert!(
        proof.is_ok(),
        "ch2_core_native_sse_multiple_choices_rejected"
    );
    let proof = proof.map_err(|_| "ch2_core_sse_proof_rejected")?;
    assert!(
        proof
            .validate_restored(
                wire.as_bytes(),
                &fixture.binding,
                &fixture.attempt,
                chat(),
                std::io::Cursor::new(wire.as_bytes()),
                &fixture.budget
            )
            .is_ok(),
        "ch2_core_sse_final_strict_check_rejected"
    );
    assert!(
        fixture.counts()? == (0, 0, 0),
        "ch2_core_sse_proof_published"
    );
    Ok(())
}

#[test]
fn ch2_core_sse_sparse_tool_index_is_choice_local() -> TestResult {
    use super::native_test_support::{chat, NativeFixture};
    let fixture = NativeFixture::new(false)?;
    let mut chunks = ch2_core_sse_chunks(true);
    chunks[0]["choices"][1]["delta"] = chunks[0]["choices"][0]["delta"].clone();
    chunks[1]["choices"][0] = chunks[2]["choices"][0].clone();
    chunks[1]["choices"][0]["index"] = json!(2);
    let wire = ch2_core_sse_wire(&chunks, true);
    let proof = ch2_core_sse_proof(&fixture, &wire);
    assert!(
        proof.is_ok(),
        "ch2_core_native_sse_sparse_choice_local_tools_rejected"
    );
    assert!(
        proof
            .map_err(|_| "ch2_core_sse_tools_proof_rejected")?
            .validate_restored(
                wire.as_bytes(),
                &fixture.binding,
                &fixture.attempt,
                chat(),
                std::io::Cursor::new(wire.as_bytes()),
                &fixture.budget
            )
            .is_ok(),
        "ch2_core_sse_tool_final_strict_check_rejected"
    );
    assert!(
        fixture.counts()? == (0, 0, 0),
        "ch2_core_sse_tool_proof_published"
    );
    Ok(())
}

#[test]
fn ch2_core_sse_invalid_index_terminal_or_tool_never_proves_completion() -> TestResult {
    use super::native_test_support::NativeFixture;
    for case in 0..12 {
        let fixture = NativeFixture::new(false)?;
        let mut chunks = ch2_core_sse_chunks(true);
        match case {
            0 => chunks[0]["choices"][1]["index"] = json!(9),
            1 => chunks[0]["choices"][0]["index"] = json!(-1),
            2 => chunks[0]["choices"][0]["index"] = json!(128),
            3 => chunks[1]["choices"][0]["finish_reason"] = Value::Null,
            4 => chunks[1]["choices"][0]["finish_reason"] = json!("length"),
            5 => chunks[2]["choices"][0]["index"] = json!(2),
            6 => {
                chunks[2]["choices"][0]["delta"]["tool_calls"][0]["id"] =
                    json!("synthetic-conflicting-call")
            }
            7 => chunks[2]["choices"][0]["delta"]["tool_calls"][0]["index"] = json!(128),
            8 => {
                chunks[2]["choices"][0]["delta"]["tool_calls"][0]["function"]["arguments"] =
                    json!("broken")
            }
            9 => chunks[2]["choices"][0]["finish_reason"] = json!("stop"),
            10 => chunks[2]["id"] = json!("synthetic-other-response"),
            _ => {
                chunks.pop();
            }
        }
        let wire = ch2_core_sse_wire(&chunks, case != 11);
        assert!(
            ch2_core_sse_proof(&fixture, &wire).is_err(),
            "ch2_core_invalid_sse_completion_accepted"
        );
        assert!(
            fixture.counts()? == (0, 0, 0),
            "ch2_core_invalid_sse_published"
        );
    }
    Ok(())
}

#[test]
fn ch2_core_sse_conversion_mode_remains_single_choice() -> TestResult {
    use super::{
        completion_evidence::{CompletionObservation, SseCompletion},
        payload_budget::PayloadBudget,
    };
    let mut completion = SseCompletion::chat();
    let budget = PayloadBudget::with_limit(1024 * 1024);
    for chunk in ch2_core_sse_chunks(false) {
        let raw = format!("data: {chunk}\n\n");
        completion.observe(&CompletionObservation::RawFrame(&raw), &budget);
        completion.observe(&CompletionObservation::ChatChunk(&chunk), &budget);
    }
    completion.observe(
        &CompletionObservation::RawFrame("data: [DONE]\n\n"),
        &budget,
    );
    completion.observe(&CompletionObservation::ChatDone, &budget);
    assert!(
        !completion.permits_completion(),
        "ch2_core_sse_conversion_multi_choice_relaxed"
    );
    Ok(())
}

// Escaped argument references must be matched only after strict object decoding.
fn ch2_core_escaped_arguments(reference: &str) -> String {
    let escaped = reference
        .encode_utf16()
        .map(|unit| format!("\\u{unit:04x}"))
        .collect::<String>();
    format!("{{\"value\":\"{escaped}\",\"types\":[true,null,7],\"nested\":{{\"again\":\"{escaped}\"}}}}")
}

#[test]
fn ch2_core_typed_json_decodes_arguments_before_restoration_and_delivery() -> TestResult {
    use super::{
        native_test_support::{chat, record_delivery, NativeFixture, MAX_ENTITY, TEXT},
        response_restoration::finalize_native_json,
        restoration_contract::{
            EditConstraint, ProtocolPair, SourceClass, TextSites, UpstreamProtocol,
        },
        restoration_core::{stage_native_json_with_lifecycle, NativeJsonObservation},
    };
    use crate::proxy::protocol_bridge::Protocol;
    use bytes::Bytes;
    for mapped in [true, false] {
        let fixture = NativeFixture::new(mapped)?;
        let baseline = fixture.budget.used();
        let arguments = ch2_core_escaped_arguments(&fixture.reference);
        let source = json!({"id":"synthetic-ch2-typed","choices":[{"index":7,"message":{"role":"assistant","content":fixture.reference,
            "tool_calls":[{"id":"synthetic-call","type":"function","function":{"name":"synthetic-tool","arguments":arguments}}]},"finish_reason":"tool_calls"}],"opaque":{"value":fixture.reference}});
        let wire = Bytes::from(serde_json::to_vec_pretty(&source)?);
        let proof = ch2_core_json_proof(&fixture, &source)
            .map_err(|_| "ch2_core_typed_json_proof_rejected")?;
        let mut sites = TextSites::new(fixture.budget.clone())?;
        sites.observe(
            &source["choices"][0]["message"]["content"],
            SourceClass::HistoricalText,
            EditConstraint::Editable,
        )?;
        sites.observe(
            &source["choices"][0]["message"]["tool_calls"][0]["function"]["arguments"],
            SourceClass::ToolArguments,
            EditConstraint::Editable,
        )?;
        let observation = NativeJsonObservation::new(
            &source,
            chat(),
            sites,
            &[&source["choices"][0]["message"]],
            fixture.budget.clone(),
        )
        .map_err(|_| "ch2_core_typed_json_observation_rejected")?;
        let staged = stage_native_json_with_lifecycle(
            &observation,
            &fixture.lease,
            &fixture.binding,
            ProtocolPair::new(chat(), UpstreamProtocol::new(Protocol::Chat)),
            &fixture.attempt,
        )
        .map_err(|_| "ch2_core_typed_json_stage_rejected")?;
        assert!(
            staged.restored_occurrences() == 3 * usize::from(mapped)
                && staged.output_tokens().len() == usize::from(mapped),
            "ch2_core_typed_json_decoded_ledger_missing"
        );
        assert!(
            staged.as_value()["opaque"] == source["opaque"],
            "ch2_core_typed_json_opaque_changed"
        );
        assert!(
            fixture.counts()? == (0, 0, 0),
            "ch2_core_typed_json_staging_published"
        );
        let finalized = finalize_native_json(
            staged,
            &fixture.lease,
            proof,
            &fixture.attempt,
            wire.clone(),
            MAX_ENTITY,
        )
        .map_err(|_| "ch2_core_typed_json_finalization_rejected")?;
        let delivered =
            futures::executor::block_on(record_delivery(finalized.into_delivery_stream()))?;
        let actual: Value = serde_json::from_slice(&delivered[0])?;
        let decoded: Value = serde_json::from_str(
            actual["choices"][0]["message"]["tool_calls"][0]["function"]["arguments"]
                .as_str()
                .ok_or("ch2_core_typed_arguments_missing")?,
        )?;
        let expected = if mapped { TEXT } else { &fixture.reference };
        assert!(
            decoded["value"].as_str() == Some(expected)
                && decoded["nested"]["again"].as_str() == Some(expected),
            "ch2_core_typed_json_decoded_leaf_not_restored"
        );
        assert!(
            decoded["types"] == json!([true, null, 7]),
            "ch2_core_typed_json_types_changed"
        );
        assert!(
            fixture
                .lease
                .verify_native_delivered(
                    &fixture.binding,
                    chat(),
                    &json!([actual["choices"][0]["message"]])
                )
                .is_ok(),
            "ch2_core_typed_json_history_missing"
        );
        if !mapped {
            assert!(
                delivered[0] == wire,
                "ch2_core_typed_json_zero_map_bytes_changed"
            );
        }
        // Delivery bytes retain their charged encoding until the last owner drops.
        drop(delivered);
        drop(observation);
        assert!(
            fixture.budget.used() == baseline,
            "ch2_core_typed_json_charge_retained"
        );
    }
    Ok(())
}

#[test]
fn ch2_core_typed_json_invalid_or_immutable_arguments_fail_without_publication() -> TestResult {
    use super::{
        native_test_support::{chat, NativeFixture},
        restoration_contract::{
            EditConstraint, ProtocolPair, SourceClass, TextSites, UpstreamProtocol,
        },
        restoration_core::{stage_native_json_with_lifecycle, NativeJsonObservation},
    };
    use crate::proxy::protocol_bridge::Protocol;
    for mode in 0..5 {
        let fixture = NativeFixture::new(mode == 4)?;
        let baseline = fixture.budget.used();
        let arguments = match mode {
            0 => "not-json".to_owned(),
            1 => "[]".to_owned(),
            2 => "".to_owned(),
            3 => "{\"value\":".to_owned(),
            _ => ch2_core_escaped_arguments(&fixture.reference),
        };
        let source = json!({"arguments":arguments});
        let mut sites = TextSites::new(fixture.budget.clone())?;
        sites.observe(
            &source["arguments"],
            SourceClass::ToolArguments,
            if mode == 4 {
                EditConstraint::Immutable
            } else {
                EditConstraint::Editable
            },
        )?;
        let observation =
            NativeJsonObservation::new(&source, chat(), sites, &[&source], fixture.budget.clone())
                .map_err(|_| "ch2_core_typed_invalid_observation_rejected")?;
        let occupied = fixture.budget.used();
        let result = stage_native_json_with_lifecycle(
            &observation,
            &fixture.lease,
            &fixture.binding,
            ProtocolPair::new(chat(), UpstreamProtocol::new(Protocol::Chat)),
            &fixture.attempt,
        );
        assert!(
            result.is_err(),
            "ch2_core_typed_invalid_or_immutable_arguments_accepted"
        );
        drop(result);
        assert!(
            fixture.budget.used() == occupied && fixture.counts()? == (0, 0, 0),
            "ch2_core_typed_failure_not_atomic"
        );
        drop(observation);
        assert!(
            fixture.budget.used() == baseline,
            "ch2_core_typed_failure_charge_retained"
        );
    }
    Ok(())
}

fn ch2_core_typed_sse_run(mixed_source: bool, explicit: bool) -> TestResult {
    use super::{
        native_test_support::{chat, NativeFixture, TEXT},
        response_restoration::finalize_native_sse,
        restoration_contract::{
            EditConstraint, ProtocolPair, SourceClass, TextSites, UpstreamProtocol,
        },
        restoration_core::{
            stage_native_sse_with_lifecycle, NativeSseFrameObservation, NativeSseObservation,
            NativeSseSnapshotSelection, NativeSseTextStream,
        },
    };
    use crate::proxy::protocol_bridge::Protocol;
    use futures::StreamExt;
    let fixture = NativeFixture::new(!mixed_source)?;
    let baseline = fixture.budget.used();
    let arguments = ch2_core_escaped_arguments(&fixture.reference);
    // Deliberately split inside the first JSON unicode escape, not at a token boundary.
    let split = arguments
        .find("\\u")
        .ok_or("ch2_core_typed_escape_missing")?
        + 3;
    let sources = [
        json!({"id":"synthetic-ch2-typed-sse","choices":[{"index":7,"delta":{"role":"assistant","tool_calls":[{"index":5,"id":"synthetic-call","type":"function","function":{"name":"synthetic-tool","arguments":&arguments[..split]}}]},"finish_reason":null}]}),
        json!({"id":"synthetic-ch2-typed-sse","choices":[{"index":7,"delta":{"tool_calls":[{"index":5,"function":{"arguments":&arguments[split..]}}]},"finish_reason":"tool_calls"}]}),
    ];
    let wire = ch2_core_sse_wire(&sources, true);
    let proof =
        ch2_core_sse_proof(&fixture, &wire).map_err(|_| "ch2_core_typed_sse_proof_rejected")?;
    let nodes = [
        &sources[0]["choices"][0]["delta"]["tool_calls"][0]["function"]["arguments"],
        &sources[1]["choices"][0]["delta"]["tool_calls"][0]["function"]["arguments"],
    ];
    let mut frames = Vec::new();
    let mut offset = 0;
    for (index, source) in sources.iter().enumerate() {
        let end = offset + format!("data: {source}\n\n").len();
        let mut sites = TextSites::new(fixture.budget.clone())?;
        sites.observe(
            nodes[index],
            if mixed_source && index == 1 {
                SourceClass::HistoricalText
            } else {
                SourceClass::ToolArguments
            },
            EditConstraint::Editable,
        )?;
        frames.push(
            NativeSseFrameObservation::new(
                &wire[offset..end],
                Some(source),
                sites,
                fixture.budget.clone(),
            )
            .map_err(|_| "ch2_core_typed_sse_frame_rejected")?,
        );
        offset = end;
    }
    frames.push(
        NativeSseFrameObservation::new(
            &wire[offset..],
            None,
            TextSites::new(fixture.budget.clone())?,
            fixture.budget.clone(),
        )
        .map_err(|_| "ch2_core_typed_sse_done_rejected")?,
    );
    let refs: Vec<_> = frames.iter().collect();
    let observed = NativeSseObservation::new(
        wire.as_bytes(),
        &refs,
        chat(),
        &fixture.lease,
        &fixture.binding,
        fixture.budget.clone(),
    )
    .map_err(|_| "ch2_core_typed_sse_observation_rejected")?;
    let selections = if explicit {
        [
            NativeSseSnapshotSelection::chat_choice(0, &sources[0]["choices"][0]),
            NativeSseSnapshotSelection::chat_choice(1, &sources[1]["choices"][0]),
        ]
    } else {
        [
            NativeSseSnapshotSelection::new(0, &sources[0]["choices"][0]["delta"]),
            NativeSseSnapshotSelection::new(1, &sources[1]["choices"][0]["delta"]),
        ]
    };
    let streams = [if explicit {
        NativeSseTextStream::with_policy(
            &nodes,
            super::restoration_contract::TextPolicy::ToolArguments,
        )
    } else {
        NativeSseTextStream::new(&nodes)
    }];
    let occupied = fixture.budget.used();
    let staged = stage_native_sse_with_lifecycle(
        &observed,
        &selections,
        &streams,
        &fixture.lease,
        &fixture.binding,
        fixture.budget.clone(),
        ProtocolPair::new(chat(), UpstreamProtocol::new(Protocol::Chat)),
        &fixture.attempt,
    );
    if mixed_source {
        assert!(
            staged.is_err(),
            "ch2_core_typed_sse_mixed_source_stream_accepted"
        );
        drop(staged);
        assert!(
            fixture.budget.used() == occupied && fixture.counts()? == (0, 0, 0),
            "ch2_core_typed_sse_mixed_source_not_atomic"
        );
        drop(proof);
    } else {
        let staged = staged.map_err(|_| "ch2_core_typed_sse_stage_rejected")?;
        assert!(
            staged.restored_occurrences() == 2 && staged.output_tokens().len() == 1,
            "ch2_core_typed_sse_decoded_ledger_missing"
        );
        assert!(
            fixture.counts()? == (0, 0, 0),
            "ch2_core_typed_sse_staging_published"
        );
        let finalized =
            finalize_native_sse(staged, &fixture.lease, proof, &fixture.attempt, 1024 * 1024)
                .map_err(|_| "ch2_core_typed_sse_finalization_rejected")?;
        let mut delivery = finalized.into_delivery_stream();
        let mut restored = String::new();
        for index in 0..3 {
            let chunk = futures::executor::block_on(delivery.next())
                .ok_or("ch2_core_typed_sse_missing_frame")??;
            assert!(
                fixture.counts()? == (0, 1, 1),
                "ch2_core_typed_sse_published_before_continuation"
            );
            if index < 2 {
                let data = std::str::from_utf8(&chunk)?
                    .lines()
                    .find_map(|line| line.strip_prefix("data: "))
                    .ok_or("ch2_core_typed_sse_data_missing")?;
                let value: Value = serde_json::from_str(data)?;
                restored.push_str(
                    value["choices"][0]["delta"]["tool_calls"][0]["function"]["arguments"]
                        .as_str()
                        .ok_or("ch2_core_typed_sse_arguments_missing")?,
                );
            }
        }
        let decoded: Value = serde_json::from_str(&restored)?;
        assert!(
            decoded["value"].as_str() == Some(TEXT)
                && decoded["nested"]["again"].as_str() == Some(TEXT),
            "ch2_core_typed_sse_decoded_leaf_not_restored"
        );
        assert!(
            futures::executor::block_on(delivery.next()).is_none(),
            "ch2_core_typed_sse_extra_frame"
        );
        assert!(
            fixture.counts()? == (1, 0, 0),
            "ch2_core_typed_sse_continuation_not_activated"
        );
        drop(delivery);
    }
    drop(observed);
    drop(frames);
    assert!(
        fixture.budget.used() == baseline,
        "ch2_core_typed_sse_charge_retained"
    );
    Ok(())
}

#[test]
fn ch2_core_typed_sse_fragmented_escaped_arguments_use_one_ledger() -> TestResult {
    ch2_core_typed_sse_run(false, false)
}

#[test]
fn ch2_core_typed_sse_mixed_source_stream_is_rejected_before_restoration() -> TestResult {
    ch2_core_typed_sse_run(true, false)
}

fn ch2_core_snapshot_run(selection_mode: usize, cancel: bool) -> TestResult {
    use super::{
        native_test_support::{chat, NativeFixture, TEXT},
        response_restoration::finalize_native_sse,
        restoration_contract::{
            EditConstraint, ProtocolPair, SourceClass, TextSites, UpstreamProtocol,
        },
        restoration_core::{
            stage_native_sse_with_lifecycle, NativeSseFrameObservation, NativeSseObservation,
            NativeSseSnapshotSelection, NativeSseTextStream,
        },
    };
    use crate::proxy::protocol_bridge::Protocol;
    use futures::StreamExt;
    let fixture = NativeFixture::new(true)?;
    let baseline = fixture.budget.used();
    let args = ch2_core_escaped_arguments(&fixture.reference);
    let split = args.find("\\u").ok_or("ch2_core_snapshot_escape_missing")? + 4;
    let text_split = fixture.reference.len() / 2;
    let sources = [
        json!({"id":"synthetic-ch2-snapshot","choices":[
            {"index":9,"delta":{"role":"assistant","tool_calls":[{"index":5,"id":"synthetic-call","type":"function","function":{"name":"synthetic-tool","arguments":&args[..split]}}]},"finish_reason":null},
            {"index":2,"delta":{"role":"assistant","content":&fixture.reference[..text_split]},"finish_reason":null}]}),
        json!({"id":"synthetic-ch2-snapshot","choices":[{"index":2,"delta":{"content":&fixture.reference[text_split..]},"finish_reason":"stop"}]}),
        json!({"id":"synthetic-ch2-snapshot","choices":[{"index":9,"delta":{"tool_calls":[{"index":5,"function":{"arguments":&args[split..]}}]},"finish_reason":"tool_calls"}]}),
    ];
    let wire = ch2_core_sse_wire(&sources, true);
    let proof =
        ch2_core_sse_proof(&fixture, &wire).map_err(|_| "ch2_core_snapshot_http_proof_rejected")?;
    let arg_nodes = [
        &sources[0]["choices"][0]["delta"]["tool_calls"][0]["function"]["arguments"],
        &sources[2]["choices"][0]["delta"]["tool_calls"][0]["function"]["arguments"],
    ];
    let text_nodes = [
        &sources[0]["choices"][1]["delta"]["content"],
        &sources[1]["choices"][0]["delta"]["content"],
    ];
    let mut frames = Vec::new();
    let mut offset = 0;
    for (index, source) in sources.iter().enumerate() {
        let mut sites = TextSites::new(fixture.budget.clone())?;
        if index == 0 {
            sites.observe(
                arg_nodes[0],
                SourceClass::ToolArguments,
                EditConstraint::Editable,
            )?;
        }
        if index < 2 {
            sites.observe(
                text_nodes[index],
                SourceClass::HistoricalText,
                EditConstraint::Editable,
            )?;
        }
        if index == 2 {
            sites.observe(
                arg_nodes[1],
                SourceClass::ToolArguments,
                EditConstraint::Editable,
            )?;
        }
        let end = offset + format!("data: {source}\n\n").len();
        frames.push(
            NativeSseFrameObservation::new(
                &wire[offset..end],
                Some(source),
                sites,
                fixture.budget.clone(),
            )
            .map_err(|_| "ch2_core_snapshot_frame_rejected")?,
        );
        offset = end;
    }
    frames.push(
        NativeSseFrameObservation::new(
            &wire[offset..],
            None,
            TextSites::new(fixture.budget.clone())?,
            fixture.budget.clone(),
        )
        .map_err(|_| "ch2_core_snapshot_done_rejected")?,
    );
    let refs: Vec<_> = frames.iter().collect();
    let observation = NativeSseObservation::new(
        wire.as_bytes(),
        &refs,
        chat(),
        &fixture.lease,
        &fixture.binding,
        fixture.budget.clone(),
    )
    .map_err(|_| "ch2_core_snapshot_observation_rejected")?;
    let mut selections = Vec::new();
    for (index, source) in sources.iter().enumerate() {
        for choice in source["choices"]
            .as_array()
            .ok_or("ch2_core_snapshot_choices_missing")?
        {
            selections.push(NativeSseSnapshotSelection::chat_choice(index, choice));
        }
    }
    match selection_mode {
        1 => {
            selections.pop();
        }
        2 => selections.swap(0, 1),
        3 => selections.push(NativeSseSnapshotSelection::chat_choice(
            0,
            &sources[0]["choices"][0],
        )),
        4 => {
            selections[0] =
                NativeSseSnapshotSelection::chat_choice(0, &sources[0]["choices"][0]["delta"])
        }
        _ => {}
    }
    let streams = [
        NativeSseTextStream::new(&arg_nodes),
        NativeSseTextStream::new(&text_nodes),
    ];
    let occupied = fixture.budget.used();
    let staged = stage_native_sse_with_lifecycle(
        &observation,
        &selections,
        &streams,
        &fixture.lease,
        &fixture.binding,
        fixture.budget.clone(),
        ProtocolPair::new(chat(), UpstreamProtocol::new(Protocol::Chat)),
        &fixture.attempt,
    );
    if selection_mode != 0 {
        assert!(
            staged.is_err(),
            "ch2_core_snapshot_inexact_or_incomplete_composition_accepted"
        );
        drop(staged);
        drop(proof);
        assert!(
            fixture.budget.used() == occupied && fixture.counts()? == (0, 0, 0),
            "ch2_core_snapshot_invalid_selection_not_atomic"
        );
    } else {
        let staged = staged.map_err(|_| "ch2_core_snapshot_stage_rejected")?;
        let call = json!({"id":"synthetic-call","type":"function","function":{"name":"synthetic-tool","arguments":serde_json::to_string(&json!({"value":TEXT,"types":[true,null,7],"nested":{"again":TEXT}}))?}});
        let expected = json!([{"role":"assistant","tool_calls":[call]},call,{"role":"assistant","content":TEXT}]);
        assert!(
            staged.delivered() == &expected,
            "ch2_core_snapshot_full_ordered_messages_and_calls_missing"
        );
        assert!(
            staged.restored_occurrences() == 3 && staged.output_tokens().len() == 1,
            "ch2_core_snapshot_single_ledger_lost"
        );
        assert!(
            fixture
                .lease
                .verify_native_delivered(&fixture.binding, chat(), &expected)
                .is_err(),
            "ch2_core_snapshot_stage_authorized_history"
        );
        let finalized =
            finalize_native_sse(staged, &fixture.lease, proof, &fixture.attempt, 1024 * 1024)
                .map_err(|_| "ch2_core_snapshot_finalization_rejected")?;
        let mut delivery = finalized.into_delivery_stream();
        for _ in 0..4 {
            let chunk = futures::executor::block_on(delivery.next())
                .ok_or("ch2_core_snapshot_frame_missing")??;
            drop(chunk);
            assert!(
                fixture.counts()? == (0, 1, 1),
                "ch2_core_snapshot_published_before_continuation"
            );
        }
        if cancel {
            drop(delivery);
            assert!(
                fixture.counts()? == (0, 0, 0)
                    && fixture
                        .lease
                        .verify_native_delivered(&fixture.binding, chat(), &expected)
                        .is_err(),
                "ch2_core_snapshot_cancel_published"
            );
        } else {
            assert!(
                futures::executor::block_on(delivery.next()).is_none(),
                "ch2_core_snapshot_extra_frame"
            );
            drop(delivery);
            assert!(
                fixture
                    .lease
                    .verify_native_delivered(&fixture.binding, chat(), &expected)
                    .is_ok(),
                "ch2_core_snapshot_full_history_missing"
            );
            let mut reordered = expected.clone();
            reordered
                .as_array_mut()
                .ok_or("ch2_core_snapshot_expected_missing")?
                .swap(0, 2);
            assert!(
                fixture
                    .lease
                    .verify_native_delivered(&fixture.binding, chat(), &reordered)
                    .is_err(),
                "ch2_core_snapshot_reordered_history_authorized"
            );
        }
    }
    drop(observation);
    drop(frames);
    assert!(
        fixture.budget.used() == baseline,
        "ch2_core_snapshot_charge_retained"
    );
    Ok(())
}

#[test]
fn ch2_core_snapshot_composes_complete_ordered_assistants_and_calls() -> TestResult {
    ch2_core_snapshot_run(0, false)
}

#[test]
fn ch2_core_snapshot_rejects_missing_reordered_duplicate_or_nonchoice_nodes() -> TestResult {
    for mode in 1..=4 {
        ch2_core_snapshot_run(mode, false)?;
    }
    Ok(())
}

#[test]
fn ch2_core_snapshot_cancel_after_done_never_publishes_full_history() -> TestResult {
    ch2_core_snapshot_run(0, true)
}

#[test]
fn ch2_core_carrier_no_data_frames_preserve_bytes_through_sealed_delivery() -> TestResult {
    use super::{
        native_test_support::{chat, NativeFixture, TEXT},
        response_restoration::finalize_native_sse,
        restoration_contract::{
            EditConstraint, ProtocolPair, SourceClass, TextSites, UpstreamProtocol,
        },
        restoration_core::{
            stage_native_sse_with_lifecycle, NativeSseFrameObservation, NativeSseObservation,
            NativeSseSnapshotSelection, NativeSseTextStream,
        },
    };
    use crate::proxy::protocol_bridge::Protocol;
    use futures::StreamExt;
    let fixture = NativeFixture::new(true)?;
    let baseline = fixture.budget.used();
    let source = json!({"id":"synthetic-ch2-keepalive","choices":[{"index":7,"delta":{"role":"assistant","content":fixture.reference},"finish_reason":"stop"}]});
    let raw = [
        "\n\n".to_owned(),
        ": synthetic keepalive\r\n\r\n".to_owned(),
        "event: ping\nid: synthetic-no-authority\nretry: 7\n\n".to_owned(),
        format!("data: {source}\n\n"),
        "data: [DONE]\n\n".to_owned(),
        ": synthetic tail\n\n".to_owned(),
    ];
    let wire = raw.concat();
    let proof = ch2_core_sse_proof(&fixture, &wire)
        .map_err(|_| "ch2_core_keepalive_http_proof_rejected")?;
    let mut frames = Vec::new();
    let mut offset = 0;
    for (index, raw) in raw.iter().enumerate() {
        let mut sites = TextSites::new(fixture.budget.clone())?;
        if index == 3 {
            sites.observe(
                &source["choices"][0]["delta"]["content"],
                SourceClass::HistoricalText,
                EditConstraint::Editable,
            )?;
        }
        let frame = NativeSseFrameObservation::new(
            &wire[offset..offset + raw.len()],
            if index == 3 { Some(&source) } else { None },
            sites,
            fixture.budget.clone(),
        );
        assert!(
            frame.is_ok(),
            "ch2_core_native_no_data_observation_rejected"
        );
        frames.push(frame.map_err(|_| "ch2_core_keepalive_frame_rejected")?);
        offset += raw.len();
    }
    let refs: Vec<_> = frames.iter().collect();
    let observed = NativeSseObservation::new(
        wire.as_bytes(),
        &refs,
        chat(),
        &fixture.lease,
        &fixture.binding,
        fixture.budget.clone(),
    )
    .map_err(|_| "ch2_core_keepalive_observation_rejected")?;
    let nodes = [&source["choices"][0]["delta"]["content"]];
    let staged = stage_native_sse_with_lifecycle(
        &observed,
        &[NativeSseSnapshotSelection::chat_choice(
            3,
            &source["choices"][0],
        )],
        &[NativeSseTextStream::new(&nodes)],
        &fixture.lease,
        &fixture.binding,
        fixture.budget.clone(),
        ProtocolPair::new(chat(), UpstreamProtocol::new(Protocol::Chat)),
        &fixture.attempt,
    )
    .map_err(|_| "ch2_core_keepalive_stage_rejected")?;
    let finalized =
        finalize_native_sse(staged, &fixture.lease, proof, &fixture.attempt, 1024 * 1024)
            .map_err(|_| "ch2_core_keepalive_finalization_rejected")?;
    let mut delivery = finalized.into_delivery_stream();
    for (index, original) in raw.iter().enumerate() {
        let chunk = futures::executor::block_on(delivery.next())
            .ok_or("ch2_core_keepalive_missing_frame")??;
        if index != 3 {
            assert!(
                chunk.as_ref() == original.as_bytes(),
                "ch2_core_keepalive_wire_changed"
            );
        } else {
            let encoded = std::str::from_utf8(&chunk)?
                .lines()
                .find_map(|line| line.strip_prefix("data: "))
                .ok_or("ch2_core_keepalive_data_missing")?;
            let actual: Value = serde_json::from_str(encoded)?;
            assert!(
                actual["choices"][0]["delta"]["content"].as_str() == Some(TEXT),
                "ch2_core_keepalive_text_not_restored"
            );
        }
        assert!(
            fixture.counts()? == (0, 1, 1),
            "ch2_core_keepalive_early_publication"
        );
    }
    assert!(
        futures::executor::block_on(delivery.next()).is_none(),
        "ch2_core_keepalive_extra_frame"
    );
    drop(delivery);
    assert!(
        fixture
            .lease
            .verify_native_delivered(
                &fixture.binding,
                chat(),
                &json!([{"role":"assistant","content":TEXT}])
            )
            .is_ok(),
        "ch2_core_keepalive_history_missing"
    );
    drop(observed);
    drop(frames);
    assert!(
        fixture.budget.used() == baseline,
        "ch2_core_keepalive_charge_retained"
    );
    Ok(())
}

#[test]
fn ch2_core_carrier_keepalive_never_supplies_completion_or_source_authority() -> TestResult {
    use super::{
        native_test_support::NativeFixture, restoration_contract::TextSites,
        restoration_core::NativeSseFrameObservation,
    };
    let fixture = NativeFixture::new(false)?;
    for wire in [
        ": synthetic\n\n",
        "event: ping\nid: synthetic\n\ndata: [DONE]\n\n",
        "data:\n\n",
    ] {
        assert!(
            ch2_core_sse_proof(&fixture, wire).is_err(),
            "ch2_core_keepalive_fabricated_completion"
        );
    }
    let foreign = json!({"choices":[]});
    assert!(
        NativeSseFrameObservation::new(
            ": synthetic\n\n",
            Some(&foreign),
            TextSites::new(fixture.budget.clone())?,
            fixture.budget.clone()
        )
        .is_err(),
        "ch2_core_no_data_foreign_source_accepted"
    );
    assert!(
        fixture.counts()? == (0, 0, 0),
        "ch2_core_keepalive_control_published"
    );
    Ok(())
}

#[test]
fn ch2_core_carrier_native_json_extensions_remain_opaque_and_conversion_closed() -> TestResult {
    use super::{
        completion_json::{JsonCompletion, JsonCompletionProtocol},
        native_test_support::NativeFixture,
    };
    let fixture = NativeFixture::new(false)?;
    for case in 0..4 {
        let mut source = fixture.json_source();
        match case {
            0 => {
                source["choices"][0]["message"]["synthetic_extension"] =
                    json!({"text":"opaque","types":[true,null,7]})
            }
            1 => {
                source["choices"][0]["finish_reason"] = json!("tool_calls");
                source["choices"][0]["message"]["tool_calls"] = json!([{"id":"synthetic-call","type":"function","synthetic_extension":{"opaque":true},"function":{"name":"synthetic-tool","arguments":"{}","synthetic_extension":[1,null]}}]);
            }
            2 => {
                source["choices"][0]["message"]["content"] = json!([{"type":"text","text":"Synthetic text","synthetic_extension":{"opaque":true}},{"type":"synthetic_opaque","payload":{"text":"opaque"}}])
            }
            _ => {
                source["choices"][0]["message"]["reasoning_details"] =
                    json!({"synthetic_extension":{"opaque":true}})
            }
        }
        assert!(
            ch2_core_json_proof(&fixture, &source).is_ok(),
            "ch2_core_native_json_extension_rejected"
        );
        let mut conversion = JsonCompletion::new(JsonCompletionProtocol::ChatCompletions);
        conversion.observe_raw_json(&serde_json::to_string(&source)?, &fixture.budget);
        assert!(
            !conversion.permits_completion(),
            "ch2_core_conversion_native_carrier_relaxed"
        );
    }
    assert!(
        fixture.counts()? == (0, 0, 0),
        "ch2_core_native_extension_granted_publication"
    );
    Ok(())
}

#[test]
fn ch2_core_carrier_native_sse_refusal_and_tool_extensions_keep_strict_types() -> TestResult {
    use super::{
        completion_evidence::{CompletionObservation, SseCompletion},
        native_test_support::{chat, NativeFixture},
    };
    let fixture = NativeFixture::new(false)?;
    for tools in [false, true] {
        let mut chunks = ch2_core_sse_chunks(tools);
        if tools {
            chunks[0]["choices"][0]["delta"]["tool_calls"][0]["synthetic_extension"] =
                json!({"opaque":true});
            chunks[0]["choices"][0]["delta"]["tool_calls"][0]["function"]["synthetic_extension"] =
                json!([true, null]);
        } else {
            chunks[0]["choices"][0]["delta"]["refusal"] = json!("Synthetic refusal");
            chunks[0]["choices"][0]["delta"]["synthetic_extension"] = json!({"opaque":true});
        }
        let wire = ch2_core_sse_wire(&chunks, true);
        let proof = ch2_core_sse_proof(&fixture, &wire);
        assert!(
            proof.is_ok(),
            "ch2_core_native_sse_refusal_or_extension_rejected"
        );
        assert!(
            proof
                .map_err(|_| "ch2_core_carrier_sse_proof_rejected")?
                .validate_restored(
                    wire.as_bytes(),
                    &fixture.binding,
                    &fixture.attempt,
                    chat(),
                    std::io::Cursor::new(wire.as_bytes()),
                    &fixture.budget
                )
                .is_ok(),
            "ch2_core_carrier_final_strict_check_rejected"
        );
        let mut invalid = chunks.clone();
        if tools {
            invalid[0]["choices"][0]["delta"]["tool_calls"][0]["function"]["arguments"] = json!(7);
        } else {
            invalid[0]["choices"][0]["delta"]["refusal"] = json!({"text":"not-a-string"});
        }
        assert!(
            ch2_core_sse_proof(&fixture, &ch2_core_sse_wire(&invalid, true)).is_err(),
            "ch2_core_native_carrier_type_weakened"
        );
        let mut conversion = SseCompletion::chat();
        for chunk in &chunks {
            conversion.observe(
                &CompletionObservation::RawFrame(&format!("data: {chunk}\n\n")),
                &fixture.budget,
            );
            conversion.observe(&CompletionObservation::ChatChunk(chunk), &fixture.budget);
        }
        conversion.observe(
            &CompletionObservation::RawFrame("data: [DONE]\n\n"),
            &fixture.budget,
        );
        conversion.observe(&CompletionObservation::ChatDone, &fixture.budget);
        assert!(
            !conversion.permits_completion(),
            "ch2_core_conversion_sse_carrier_relaxed"
        );
    }
    Ok(())
}

// Post-carrier characterization: an edited native envelope must retain opaque siblings.
// This is not a behavioral RED and does not establish explicit reasoning-policy selection.
#[test]
fn ch2_core_carrier_json_refusal_edits_preserve_extensions_and_sealed_history() -> TestResult {
    use super::{
        native_test_support::{chat, record_delivery, NativeFixture, MAX_ENTITY, TEXT},
        response_restoration::finalize_native_json,
        restoration_contract::{
            EditConstraint, ProtocolPair, SourceClass, TextSites, UpstreamProtocol,
        },
        restoration_core::{stage_native_json_with_lifecycle, NativeJsonObservation},
    };
    use crate::proxy::protocol_bridge::Protocol;
    use bytes::Bytes;
    for mapped in [false, true] {
        let fixture = NativeFixture::new(mapped)?;
        let baseline = fixture.budget.used();
        let source = json!({"id":"synthetic-carrier-edits","choices":[{"index":5,"message":{
            "role":"assistant","refusal":fixture.reference,
            "content":[{"type":"text","text":fixture.reference,"synthetic_extension":{"text":fixture.reference}},
                {"type":"refusal","refusal":fixture.reference,"synthetic_extension":[true,null,7]},
                {"type":"synthetic_opaque","payload":{"text":fixture.reference}}],
            "reasoning_details":{"synthetic_opaque":fixture.reference},
            "synthetic_extension":{"text":fixture.reference,"types":[true,null,7]}
        },"finish_reason":"stop"}],"synthetic_extension":{"text":fixture.reference}});
        let original = source.clone();
        let wire = Bytes::from(serde_json::to_vec_pretty(&source)?);
        let proof = ch2_core_json_proof(&fixture, &source)
            .map_err(|_| "ch2_core_carrier_edits_proof_rejected")?;
        let mut sites = TextSites::new(fixture.budget.clone())?;
        for node in [
            &source["choices"][0]["message"]["refusal"],
            &source["choices"][0]["message"]["content"][0]["text"],
            &source["choices"][0]["message"]["content"][1]["refusal"],
        ] {
            sites.observe(node, SourceClass::HistoricalText, EditConstraint::Editable)?;
        }
        let observed = NativeJsonObservation::new(
            &source,
            chat(),
            sites,
            &[&source["choices"][0]["message"]],
            fixture.budget.clone(),
        )
        .map_err(|_| "ch2_core_carrier_edits_observation_rejected")?;
        let staged = stage_native_json_with_lifecycle(
            &observed,
            &fixture.lease,
            &fixture.binding,
            ProtocolPair::new(chat(), UpstreamProtocol::new(Protocol::Chat)),
            &fixture.attempt,
        )
        .map_err(|_| "ch2_core_carrier_edits_staging_rejected")?;
        assert!(
            staged.restored_occurrences() == 3 * usize::from(mapped)
                && staged.output_tokens().len() == usize::from(mapped),
            "ch2_core_carrier_edits_ledger_invalid"
        );
        assert!(
            fixture.counts()? == (0, 0, 0),
            "ch2_core_carrier_edits_staging_published"
        );
        let finalized = finalize_native_json(
            staged,
            &fixture.lease,
            proof,
            &fixture.attempt,
            wire.clone(),
            MAX_ENTITY,
        )
        .map_err(|_| "ch2_core_carrier_edits_finalization_rejected")?;
        let delivered =
            futures::executor::block_on(record_delivery(finalized.into_delivery_stream()))?;
        let actual: Value = serde_json::from_slice(&delivered[0])?;
        let mut expected = source.clone();
        if mapped {
            expected["choices"][0]["message"]["refusal"] = json!(TEXT);
            expected["choices"][0]["message"]["content"][0]["text"] = json!(TEXT);
            expected["choices"][0]["message"]["content"][1]["refusal"] = json!(TEXT);
        } else {
            assert!(
                delivered[0] == wire,
                "ch2_core_carrier_zero_map_wire_changed"
            );
        }
        assert!(
            actual == expected,
            "ch2_core_carrier_opaque_extension_or_semantics_changed"
        );
        assert!(source == original, "ch2_core_carrier_original_mutated");
        assert!(
            fixture
                .lease
                .verify_native_delivered(
                    &fixture.binding,
                    chat(),
                    &json!([actual["choices"][0]["message"]])
                )
                .is_ok(),
            "ch2_core_carrier_edits_history_missing"
        );
        let mut edited_history = json!([actual["choices"][0]["message"]]);
        edited_history[0]["synthetic_extension"]["text"] = json!("synthetic-changed");
        assert!(
            fixture
                .lease
                .verify_native_delivered(&fixture.binding, chat(), &edited_history)
                .is_err(),
            "ch2_core_carrier_opaque_history_edit_accepted"
        );
        drop(delivered);
        drop(observed);
        assert!(
            fixture.budget.used() == baseline,
            "ch2_core_carrier_edits_charge_retained"
        );
    }
    Ok(())
}

// Flux F1: strict SSE nulls do not erase an established function-call identity.
fn ch2_core_snapshot_null_identity_run(field: usize) -> TestResult {
    use super::{
        native_test_support::{chat, NativeFixture, TEXT},
        response_restoration::finalize_native_sse,
        restoration_contract::{
            EditConstraint, ProtocolPair, SourceClass, TextSites, UpstreamProtocol,
        },
        restoration_core::{
            stage_native_sse_with_lifecycle, NativeSseFrameObservation, NativeSseObservation,
            NativeSseSnapshotSelection, NativeSseTextStream,
        },
    };
    use crate::proxy::protocol_bridge::Protocol;
    use futures::StreamExt;
    let fixture = NativeFixture::new(true)?;
    let baseline = fixture.budget.used();
    let args = ch2_core_escaped_arguments(&fixture.reference);
    let split = args.len() / 2;
    let mut sources = [
        json!({"id":"synthetic-null-identity","choices":[{"index":9,"delta":{"role":"assistant","tool_calls":[{"index":5,"id":"synthetic-call","type":"function","function":{"name":"synthetic-tool","arguments":&args[..split]}}]},"finish_reason":null}]}),
        json!({"id":"synthetic-null-identity","choices":[{"index":9,"delta":{"tool_calls":[{"index":5,"function":{"arguments":&args[split..]}}]},"finish_reason":"tool_calls"}]}),
    ];
    match field {
        0 => sources[1]["choices"][0]["delta"]["tool_calls"][0]["id"] = Value::Null,
        1 => sources[1]["choices"][0]["delta"]["tool_calls"][0]["type"] = Value::Null,
        _ => sources[1]["choices"][0]["delta"]["tool_calls"][0]["function"]["name"] = Value::Null,
    }
    let wire = ch2_core_sse_wire(&sources, true);
    let proof = ch2_core_sse_proof(&fixture, &wire)
        .map_err(|_| "ch2_core_snapshot_null_valid_wire_rejected")?;
    let nodes = [
        &sources[0]["choices"][0]["delta"]["tool_calls"][0]["function"]["arguments"],
        &sources[1]["choices"][0]["delta"]["tool_calls"][0]["function"]["arguments"],
    ];
    let mut frames = Vec::new();
    let mut offset = 0;
    for (index, source) in sources.iter().enumerate() {
        let mut sites = TextSites::new(fixture.budget.clone())?;
        sites.observe(
            nodes[index],
            SourceClass::ToolArguments,
            EditConstraint::Editable,
        )?;
        let end = offset + format!("data: {source}\n\n").len();
        frames.push(
            NativeSseFrameObservation::new(
                &wire[offset..end],
                Some(source),
                sites,
                fixture.budget.clone(),
            )
            .map_err(|_| "ch2_core_snapshot_null_frame_rejected")?,
        );
        offset = end;
    }
    frames.push(
        NativeSseFrameObservation::new(
            &wire[offset..],
            None,
            TextSites::new(fixture.budget.clone())?,
            fixture.budget.clone(),
        )
        .map_err(|_| "ch2_core_snapshot_null_done_rejected")?,
    );
    let refs: Vec<_> = frames.iter().collect();
    let observation = NativeSseObservation::new(
        wire.as_bytes(),
        &refs,
        chat(),
        &fixture.lease,
        &fixture.binding,
        fixture.budget.clone(),
    )
    .map_err(|_| "ch2_core_snapshot_null_observation_rejected")?;
    let selections = [
        NativeSseSnapshotSelection::chat_choice(0, &sources[0]["choices"][0]),
        NativeSseSnapshotSelection::chat_choice(1, &sources[1]["choices"][0]),
    ];
    let streams = [NativeSseTextStream::new(&nodes)];
    let staged = stage_native_sse_with_lifecycle(
        &observation,
        &selections,
        &streams,
        &fixture.lease,
        &fixture.binding,
        fixture.budget.clone(),
        ProtocolPair::new(chat(), UpstreamProtocol::new(Protocol::Chat)),
        &fixture.attempt,
    )
    .map_err(|_| "ch2_core_snapshot_null_staging_rejected")?;
    let call = json!({"id":"synthetic-call","type":"function","function":{"name":"synthetic-tool","arguments":serde_json::to_string(&json!({"value":TEXT,"types":[true,null,7],"nested":{"again":TEXT}}))?}});
    let expected = json!([{"role":"assistant","tool_calls":[call]},call]);
    assert!(
        fixture
            .lease
            .verify_native_delivered(&fixture.binding, chat(), &expected)
            .is_err(),
        "ch2_core_snapshot_null_staging_authorized_history"
    );
    let finalized =
        finalize_native_sse(staged, &fixture.lease, proof, &fixture.attempt, 1024 * 1024)
            .map_err(|_| "ch2_core_snapshot_null_finalization_rejected")?;
    let mut delivery = finalized.into_delivery_stream();
    for _ in 0..3 {
        let chunk = futures::executor::block_on(delivery.next())
            .ok_or("ch2_core_snapshot_null_frame_missing")??;
        drop(chunk);
        assert!(
            fixture.counts()? == (0, 1, 1),
            "ch2_core_snapshot_null_early_publication"
        );
    }
    assert!(
        futures::executor::block_on(delivery.next()).is_none(),
        "ch2_core_snapshot_null_extra_frame"
    );
    drop(delivery);
    assert!(
        fixture
            .lease
            .verify_native_delivered(&fixture.binding, chat(), &expected)
            .is_ok(),
        "ch2_core_snapshot_null_identity_erased_in_published_history"
    );
    drop(observation);
    drop(frames);
    assert!(
        fixture.budget.used() == baseline,
        "ch2_core_snapshot_null_charge_retained"
    );
    Ok(())
}

#[test]
fn ch2_core_snapshot_null_tool_id_preserves_strict_identity() -> TestResult {
    ch2_core_snapshot_null_identity_run(0)
}
#[test]
fn ch2_core_snapshot_null_tool_type_preserves_strict_identity() -> TestResult {
    ch2_core_snapshot_null_identity_run(1)
}
#[test]
fn ch2_core_snapshot_null_function_name_preserves_strict_identity() -> TestResult {
    ch2_core_snapshot_null_identity_run(2)
}

#[test]
fn ch2_core_snapshot_assembled_calls_require_strict_complete_structure() -> TestResult {
    use super::{native_test_support::NativeFixture, restoration_core::compose_chat_snapshots};
    let fixture = NativeFixture::new(false)?;
    let baseline = fixture.budget.used();
    for mode in 0..6 {
        let mut choices = [
            json!({"index":9,"delta":{"role":"assistant","tool_calls":[{"index":5,"id":"synthetic-call","type":"function","function":{"name":"synthetic-tool","arguments":"{}"}}]},"finish_reason":"tool_calls"}),
        ];
        match mode {
            0 => choices[0]["delta"]["tool_calls"][0]["id"] = Value::Null,
            1 => choices[0]["delta"]["tool_calls"][0]["type"] = Value::Null,
            2 => choices[0]["delta"]["tool_calls"][0]["function"]["name"] = Value::Null,
            3 => choices[0]["delta"]["tool_calls"][0]["function"]["arguments"] = json!("[]"),
            4 => choices[0]["delta"]["tool_calls"][0]["function"]["arguments"] = json!("broken"),
            _ => {
                choices[0]["delta"]["tool_calls"] = json!([
                    {"index":5,"id":"synthetic-call","type":"function","function":{"name":"synthetic-tool","arguments":"{}"}},
                    {"index":7,"id":"synthetic-call","type":"function","function":{"name":"synthetic-other","arguments":"{}"}}
                ])
            }
        }
        let assembled = compose_chat_snapshots(&[&choices[0]], &fixture.budget);
        assert!(
            assembled.is_err(),
            "ch2_core_snapshot_malformed_assembled_call_accepted"
        );
        drop(assembled);
        assert!(
            fixture.budget.used() == baseline && fixture.counts()? == (0, 0, 0),
            "ch2_core_snapshot_malformed_assembly_not_atomic"
        );
    }
    Ok(())
}

// Typed policy safety: readable reasoning must not become ordinary output restoration.
fn ch2_core_policy_json_run(reasoning: bool) -> TestResult {
    use super::{
        native_test_support::{chat, record_delivery, NativeFixture, MAX_ENTITY, TEXT},
        response_restoration::finalize_native_json,
        restoration_contract::{
            EditConstraint, ProtocolPair, SourceClass, TextPolicy, TextSites, UpstreamProtocol,
        },
        restoration_core::{stage_native_json_with_lifecycle, NativeJsonObservation},
    };
    use crate::proxy::protocol_bridge::Protocol;
    for mapped in [true, false] {
        let fixture = NativeFixture::new(mapped)?;
        let baseline = fixture.budget.used();
        let message = if reasoning {
            json!({"role":"assistant","reasoning_content":fixture.reference,"opaque":{"value":fixture.reference}})
        } else {
            json!({"role":"assistant","content":fixture.reference,"refusal":fixture.reference})
        };
        let source = json!({"id":"synthetic-policy-json","choices":[{"index":0,"message":message,"finish_reason":"stop"}]});
        let before = source.clone();
        let proof =
            ch2_core_json_proof(&fixture, &source).map_err(|_| "policy_json_proof_rejected")?;
        let wire = bytes::Bytes::from(serde_json::to_vec_pretty(&source)?);
        let mut sites = TextSites::new(fixture.budget.clone())?;
        let fields: &[&str] = if reasoning {
            &["reasoning_content"]
        } else {
            &["content", "refusal"]
        };
        for field in fields {
            sites.observe(
                &source["choices"][0]["message"][field],
                SourceClass::HistoricalText,
                EditConstraint::Editable,
            )?;
        }
        let policies: &[TextPolicy] = if reasoning {
            &[TextPolicy::Reasoning]
        } else {
            &[TextPolicy::Output, TextPolicy::Refusal]
        };
        let observed = NativeJsonObservation::new_with_policies(
            &source,
            chat(),
            sites,
            &[&source["choices"][0]["message"]],
            policies,
            fixture.budget.clone(),
        )
        .map_err(|_| "policy_json_observation_rejected")?;
        let staged = stage_native_json_with_lifecycle(
            &observed,
            &fixture.lease,
            &fixture.binding,
            ProtocolPair::new(chat(), UpstreamProtocol::new(Protocol::Chat)),
            &fixture.attempt,
        )
        .map_err(|_| "policy_json_stage_rejected")?;
        let expected_restores = if reasoning {
            0
        } else {
            fields.len() * usize::from(mapped)
        };
        assert!(
            staged.restored_occurrences() == expected_restores
                && staged.output_tokens().len() == usize::from(!reasoning && mapped),
            "policy_json_reasoning_restored_or_dependencies_granted"
        );
        assert!(
            fixture.counts()? == (0, 0, 0),
            "policy_json_stage_published"
        );
        let finalized = finalize_native_json(
            staged,
            &fixture.lease,
            proof,
            &fixture.attempt,
            wire.clone(),
            MAX_ENTITY,
        )
        .map_err(|_| "policy_json_finalization_rejected")?;
        let delivered =
            futures::executor::block_on(record_delivery(finalized.into_delivery_stream()))?;
        let actual: Value = serde_json::from_slice(&delivered[0])?;
        for field in fields {
            let expected = if !reasoning && mapped {
                TEXT
            } else {
                &fixture.reference
            };
            assert!(
                actual["choices"][0]["message"][field].as_str() == Some(expected),
                "policy_json_delivered_text_changed"
            );
        }
        assert!(
            source == before
                && fixture
                    .lease
                    .verify_native_delivered(
                        &fixture.binding,
                        chat(),
                        &json!([actual["choices"][0]["message"]])
                    )
                    .is_ok(),
            "policy_json_source_or_history_changed"
        );
        if reasoning || !mapped {
            assert!(
                delivered[0] == wire,
                "policy_json_unchanged_bytes_reencoded"
            );
        }
        drop(delivered);
        drop(observed);
        assert!(
            fixture.budget.used() == baseline,
            "policy_json_retained_charge"
        );
    }
    Ok(())
}

#[test]
fn ch2_core_policy_json_reasoning_preserves_reference_without_dependencies() -> TestResult {
    ch2_core_policy_json_run(true)
}

#[test]
fn ch2_core_policy_json_output_and_refusal_restore_control() -> TestResult {
    ch2_core_policy_json_run(false)
}

fn ch2_core_policy_sse_run(reasoning: bool, cancel: bool) -> TestResult {
    use super::{
        native_test_support::{chat, NativeFixture, MAX_ENTITY, TEXT},
        response_restoration::finalize_native_sse,
        restoration_contract::{
            EditConstraint, ProtocolPair, SourceClass, TextPolicy, TextSites, UpstreamProtocol,
        },
        restoration_core::{
            stage_native_sse_with_lifecycle, NativeSseFrameObservation, NativeSseObservation,
            NativeSseSnapshotSelection, NativeSseTextStream,
        },
    };
    use crate::proxy::protocol_bridge::Protocol;
    use futures::StreamExt;
    for mapped in [true, false] {
        let fixture = NativeFixture::new(mapped)?;
        let baseline = fixture.budget.used();
        let split = fixture.reference.len() / 2;
        let field = if reasoning {
            "reasoning_content"
        } else {
            "refusal"
        };
        let mut sources = [
            json!({"id":"synthetic-policy-sse","choices":[{"index":0,"delta":{"role":"assistant"},"finish_reason":null}]}),
            json!({"id":"synthetic-policy-sse","choices":[{"index":0,"delta":{},"finish_reason":"stop"}]}),
        ];
        sources[0]["choices"][0]["delta"][field] = json!(&fixture.reference[..split]);
        sources[1]["choices"][0]["delta"][field] = json!(&fixture.reference[split..]);
        let before = sources.clone();
        let wire = ch2_core_sse_wire(&sources, true);
        let proof = ch2_core_sse_proof(&fixture, &wire).map_err(|_| "policy_sse_proof_rejected")?;
        let nodes = [
            &sources[0]["choices"][0]["delta"][field],
            &sources[1]["choices"][0]["delta"][field],
        ];
        let mut frames = Vec::new();
        let mut offset = 0;
        for (index, source) in sources.iter().enumerate() {
            let end = offset + format!("data: {source}\n\n").len();
            let mut sites = TextSites::new(fixture.budget.clone())?;
            sites.observe(
                nodes[index],
                SourceClass::HistoricalText,
                EditConstraint::Editable,
            )?;
            frames.push(
                NativeSseFrameObservation::new(
                    &wire[offset..end],
                    Some(source),
                    sites,
                    fixture.budget.clone(),
                )
                .map_err(|_| "policy_sse_frame_rejected")?,
            );
            offset = end;
        }
        frames.push(
            NativeSseFrameObservation::new(
                &wire[offset..],
                None,
                TextSites::new(fixture.budget.clone())?,
                fixture.budget.clone(),
            )
            .map_err(|_| "policy_sse_done_rejected")?,
        );
        let refs: Vec<_> = frames.iter().collect();
        let observed = NativeSseObservation::new(
            wire.as_bytes(),
            &refs,
            chat(),
            &fixture.lease,
            &fixture.binding,
            fixture.budget.clone(),
        )
        .map_err(|_| "policy_sse_observation_rejected")?;
        let selections = [
            NativeSseSnapshotSelection::chat_choice(0, &sources[0]["choices"][0]),
            NativeSseSnapshotSelection::chat_choice(1, &sources[1]["choices"][0]),
        ];
        let streams = [NativeSseTextStream::with_policy(
            &nodes,
            if reasoning {
                TextPolicy::Reasoning
            } else {
                TextPolicy::Refusal
            },
        )];
        let staged = stage_native_sse_with_lifecycle(
            &observed,
            &selections,
            &streams,
            &fixture.lease,
            &fixture.binding,
            fixture.budget.clone(),
            ProtocolPair::new(chat(), UpstreamProtocol::new(Protocol::Chat)),
            &fixture.attempt,
        )
        .map_err(|_| "policy_sse_stage_rejected")?;
        assert!(
            staged.restored_occurrences() == usize::from(!reasoning && mapped)
                && staged.output_tokens().len() == usize::from(!reasoning && mapped),
            "policy_sse_reasoning_restored_or_dependencies_granted"
        );
        assert!(fixture.counts()? == (0, 0, 0), "policy_sse_stage_published");
        let finalized =
            finalize_native_sse(staged, &fixture.lease, proof, &fixture.attempt, MAX_ENTITY)
                .map_err(|_| "policy_sse_finalization_rejected")?;
        let mut delivery = finalized.into_delivery_stream();
        let mut received = String::new();
        for _ in 0..3 {
            let chunk = futures::executor::block_on(delivery.next())
                .ok_or("policy_sse_frame_missing")??;
            received.push_str(std::str::from_utf8(&chunk)?);
            assert!(
                fixture.counts()? == (0, 1, 1),
                "policy_sse_published_before_last_resume"
            );
        }
        if cancel {
            drop(delivery);
            assert!(
                fixture.counts()? == (0, 0, 0),
                "policy_sse_cancel_published"
            );
        } else {
            assert!(
                futures::executor::block_on(delivery.next()).is_none(),
                "policy_sse_extra_frame"
            );
            drop(delivery);
            let expected = if !reasoning && mapped {
                TEXT
            } else {
                &fixture.reference
            };
            let history = json!([{"role":"assistant",field:expected}]);
            assert!(
                fixture
                    .lease
                    .verify_native_delivered(&fixture.binding, chat(), &history)
                    .is_ok(),
                "policy_sse_delivered_history_mismatch"
            );
            assert!(fixture.counts()? == (1, 0, 0), "policy_sse_not_published");
        }
        assert!(sources == before, "policy_sse_original_mutated");
        if reasoning || !mapped {
            assert!(received == wire, "policy_sse_unchanged_bytes_changed");
        }
        drop(observed);
        drop(frames);
        assert!(
            fixture.budget.used() == baseline,
            "policy_sse_retained_charge"
        );
    }
    Ok(())
}

#[test]
fn ch2_core_policy_sse_split_reasoning_preserves_reference_without_dependencies() -> TestResult {
    ch2_core_policy_sse_run(true, false)
}

#[test]
fn ch2_core_policy_sse_split_refusal_restores_control_and_cancel_refunds() -> TestResult {
    ch2_core_policy_sse_run(false, false)?;
    ch2_core_policy_sse_run(false, true)
}

fn ch2_core_policy_json_rejection_run(mode: usize) -> TestResult {
    use super::{
        native_test_support::{chat, NativeFixture},
        restoration_contract::{
            EditConstraint, ProtocolPair, SourceClass, TextPolicy, TextSites, UpstreamProtocol,
        },
        restoration_core::{
            stage_native_json, stage_native_json_with_lifecycle, NativeJsonObservation,
        },
    };
    use crate::proxy::protocol_bridge::Protocol;
    let fixture = NativeFixture::new(true)?;
    let baseline = fixture.budget.used();
    let source = json!({"text":ch2_core_escaped_arguments(&fixture.reference)});
    let mut sites = TextSites::new(fixture.budget.clone())?;
    sites.observe(
        &source["text"],
        if mode == 3 {
            SourceClass::ToolArguments
        } else {
            SourceClass::HistoricalText
        },
        EditConstraint::Editable,
    )?;
    let policies: &[TextPolicy] = match mode {
        0 => &[],
        1 => &[TextPolicy::Output, TextPolicy::Refusal],
        2 => &[TextPolicy::ToolArguments],
        3 => &[TextPolicy::Reasoning],
        _ => &[TextPolicy::Refusal],
    };
    let observed = NativeJsonObservation::new_with_policies(
        &source,
        chat(),
        sites,
        &[&source],
        policies,
        fixture.budget.clone(),
    );
    if mode < 4 {
        assert!(
            observed.is_err(),
            "policy_json_bad_cardinality_or_source_accepted"
        );
        drop(observed);
    } else {
        let observed = observed.map_err(|_| "policy_json_valid_explicit_policy_rejected")?;
        let occupied = fixture.budget.used();
        let staged = stage_native_json(&observed, &fixture.lease, &fixture.binding);
        assert!(
            staged.is_err(),
            "policy_json_legacy_staging_ignored_explicit_policy"
        );
        drop(staged);
        assert!(
            fixture.budget.used() == occupied,
            "policy_json_legacy_failure_retained_charge"
        );
        let staged = stage_native_json_with_lifecycle(
            &observed,
            &fixture.lease,
            &fixture.binding,
            ProtocolPair::new(chat(), UpstreamProtocol::new(Protocol::Chat)),
            &fixture.attempt,
        );
        assert!(
            staged.is_ok(),
            "policy_json_legacy_failure_poisoned_lifecycle_retry"
        );
        drop(staged);
        drop(observed);
    }
    assert!(
        fixture.counts()? == (0, 0, 0) && fixture.budget.used() == baseline,
        "policy_json_rejection_published_or_retained_charge"
    );
    Ok(())
}

#[test]
fn ch2_core_policy_json_missing_policy_is_rejected() -> TestResult {
    ch2_core_policy_json_rejection_run(0)
}
#[test]
fn ch2_core_policy_json_extra_policy_is_rejected() -> TestResult {
    ch2_core_policy_json_rejection_run(1)
}
#[test]
fn ch2_core_policy_json_text_cannot_claim_argument_policy() -> TestResult {
    ch2_core_policy_json_rejection_run(2)
}
#[test]
fn ch2_core_policy_json_arguments_cannot_claim_reasoning_policy() -> TestResult {
    ch2_core_policy_json_rejection_run(3)
}
#[test]
fn ch2_core_policy_json_legacy_staging_cannot_ignore_explicit_policy() -> TestResult {
    ch2_core_policy_json_rejection_run(4)
}

fn ch2_core_policy_sse_rejection_run(mode: usize) -> TestResult {
    use super::{
        native_test_support::{chat, NativeFixture},
        restoration_contract::{
            EditConstraint, ProtocolPair, SourceClass, TextPolicy, TextSites, UpstreamProtocol,
        },
        restoration_core::{
            stage_native_sse_with_lifecycle, NativeSseFrameObservation, NativeSseObservation,
            NativeSseSnapshotSelection, NativeSseTextStream,
        },
    };
    use crate::proxy::protocol_bridge::Protocol;
    // Each negative has a matching valid-carrier control differing only at its target guard.
    for reject in [true, false] {
        let fixture = NativeFixture::new(true)?;
        let baseline = fixture.budget.used();
        let arguments = mode < 2;
        let text = if arguments {
            ch2_core_escaped_arguments(&fixture.reference)
        } else {
            fixture.reference.clone()
        };
        let split = text.len() / 2;
        let sources: Vec<_> = [&text[..split], &text[split..]].iter().enumerate().map(|(index, piece)| {
            let mut delta = if arguments {
                json!({"tool_calls":[{"index":97,"function":{"arguments":piece}}]})
            } else { json!({"content":piece}) };
            if index == 0 {
                delta["role"] = json!("assistant");
                if arguments {
                    delta["tool_calls"][0]["id"] = json!("synthetic-policy-call");
                    delta["tool_calls"][0]["type"] = json!("function");
                    delta["tool_calls"][0]["function"]["name"] = json!("synthetic-tool");
                }
            }
            json!({"id":"synthetic-policy","choices":[{"index":9,"delta":delta,
                "finish_reason":if index == 1 {Some(if arguments {"tool_calls"} else {"stop"})} else {None}}]})
        }).collect();
        let wire = ch2_core_sse_wire(&sources, true);
        let nodes: Vec<_> = sources
            .iter()
            .map(|source| {
                if arguments {
                    &source["choices"][0]["delta"]["tool_calls"][0]["function"]["arguments"]
                } else {
                    &source["choices"][0]["delta"]["content"]
                }
            })
            .collect();
        let mut frames = Vec::new();
        let mut offset = 0;
        for (index, source) in sources.iter().enumerate() {
            let end = offset + format!("data: {source}\n\n").len();
            let mut sites = TextSites::new(fixture.budget.clone())?;
            let classification = if arguments {
                SourceClass::ToolArguments
            } else if reject && mode == 3 && index == 1 {
                SourceClass::CurrentInput
            } else {
                SourceClass::HistoricalText
            };
            sites.observe(
                nodes[index],
                classification,
                if reject && mode == 4 {
                    EditConstraint::Immutable
                } else {
                    EditConstraint::Editable
                },
            )?;
            frames.push(
                NativeSseFrameObservation::new(
                    &wire[offset..end],
                    Some(source),
                    sites,
                    fixture.budget.clone(),
                )
                .map_err(|_| "policy_sse_rejection_frame_rejected")?,
            );
            offset = end;
        }
        frames.push(
            NativeSseFrameObservation::new(
                &wire[offset..],
                None,
                TextSites::new(fixture.budget.clone())?,
                fixture.budget.clone(),
            )
            .map_err(|_| "policy_sse_rejection_done_rejected")?,
        );
        let refs: Vec<_> = frames.iter().collect();
        let observed = NativeSseObservation::new(
            wire.as_bytes(),
            &refs,
            chat(),
            &fixture.lease,
            &fixture.binding,
            fixture.budget.clone(),
        )
        .map_err(|_| "policy_sse_rejection_observation_rejected")?;
        let selections = [
            NativeSseSnapshotSelection::chat_choice(0, &sources[0]["choices"][0]),
            NativeSseSnapshotSelection::chat_choice(1, &sources[1]["choices"][0]),
        ];
        let policy = if reject {
            match mode {
                0 => TextPolicy::Refusal,
                1 => TextPolicy::Reasoning,
                2 => TextPolicy::ToolArguments,
                _ => TextPolicy::Output,
            }
        } else if arguments {
            TextPolicy::ToolArguments
        } else {
            TextPolicy::Output
        };
        let streams = [NativeSseTextStream::with_policy(&nodes, policy)];
        let occupied = fixture.budget.used();
        let staged = stage_native_sse_with_lifecycle(
            &observed,
            &selections,
            &streams,
            &fixture.lease,
            &fixture.binding,
            fixture.budget.clone(),
            ProtocolPair::new(chat(), UpstreamProtocol::new(Protocol::Chat)),
            &fixture.attempt,
        );
        if reject {
            assert!(
                staged.is_err(),
                "policy_sse_mismatched_or_immutable_stream_accepted"
            );
        } else {
            let staged = staged
                .as_ref()
                .map_err(|_| "policy_sse_valid_carrier_control_rejected")?;
            assert!(
                staged.restored_occurrences() == if arguments { 2 } else { 1 }
                    && staged.output_tokens().len() == 1,
                "policy_sse_valid_carrier_control_not_restored"
            );
        }
        drop(staged);
        assert!(
            fixture.budget.used() == occupied && fixture.counts()? == (0, 0, 0),
            "policy_sse_failure_retained_charge_or_published"
        );
        drop(observed);
        drop(frames);
        assert!(
            fixture.budget.used() == baseline,
            "policy_sse_rejection_retained_charge"
        );
    }
    Ok(())
}

#[test]
fn ch2_core_policy_sse_arguments_cannot_claim_refusal_policy() -> TestResult {
    ch2_core_policy_sse_rejection_run(0)
}
#[test]
fn ch2_core_policy_sse_arguments_cannot_claim_reasoning_policy() -> TestResult {
    ch2_core_policy_sse_rejection_run(1)
}
#[test]
fn ch2_core_policy_sse_text_cannot_claim_argument_policy() -> TestResult {
    ch2_core_policy_sse_rejection_run(2)
}
#[test]
fn ch2_core_policy_sse_mixed_source_and_immutable_streams_remain_rejected() -> TestResult {
    ch2_core_policy_sse_rejection_run(3)?;
    ch2_core_policy_sse_rejection_run(4)
}

#[test]
fn ch2_core_policy_json_mixed_policies_share_ledger_and_metadata_refunds() -> TestResult {
    use super::{
        native_test_support::{chat, NativeFixture, TEXT},
        payload_metadata::test_support::{self, Action, Kind, Phase},
        restoration_contract::{
            EditConstraint, ProtocolPair, SourceClass, TextPolicy, TextSites, UpstreamProtocol,
        },
        restoration_core::{stage_native_json_with_lifecycle, NativeJsonObservation},
    };
    use crate::proxy::protocol_bridge::Protocol;
    let fixture = NativeFixture::new(true)?;
    let baseline = fixture.budget.used();
    let source = json!({"content":fixture.reference,"refusal":fixture.reference,"reasoning":fixture.reference,"arguments":ch2_core_escaped_arguments(&fixture.reference)});
    let policies = [
        TextPolicy::Output,
        TextPolicy::Refusal,
        TextPolicy::Reasoning,
        TextPolicy::ToolArguments,
    ];
    // The policy vector is a real charged owner; exercise both sides of allocation.
    for phase in [
        None,
        Some(Phase::BeforeAllocation),
        Some(Phase::AfterAllocation),
    ] {
        let mut sites = TextSites::new(fixture.budget.clone())?;
        for field in ["content", "refusal", "reasoning", "arguments"] {
            sites.observe(
                &source[field],
                if field == "arguments" {
                    SourceClass::ToolArguments
                } else {
                    SourceClass::HistoricalText
                },
                if field == "reasoning" {
                    EditConstraint::Immutable
                } else {
                    EditConstraint::Editable
                },
            )?;
        }
        let reached = std::rc::Rc::new(std::cell::Cell::new(false));
        let observed_fault = reached.clone();
        let hook = test_support::install(move |event| {
            if phase == Some(event.phase)
                && event.kind == Kind::Vector
                && event.bytes == 4 * std::mem::size_of::<TextPolicy>()
            {
                observed_fault.set(true);
                Action::Fail
            } else {
                Action::Continue
            }
        });
        let observed = NativeJsonObservation::new_with_policies(
            &source,
            chat(),
            sites,
            &[&source],
            &policies,
            fixture.budget.clone(),
        );
        drop(hook);
        if phase.is_some() {
            assert!(
                reached.get() && observed.is_err(),
                "policy_json_metadata_fault_not_reached"
            );
            drop(observed);
        } else {
            let observed = observed.map_err(|_| "policy_json_mixed_observation_rejected")?;
            let staged = stage_native_json_with_lifecycle(
                &observed,
                &fixture.lease,
                &fixture.binding,
                ProtocolPair::new(chat(), UpstreamProtocol::new(Protocol::Chat)),
                &fixture.attempt,
            )
            .map_err(|_| "policy_json_mixed_stage_rejected")?;
            assert!(
                staged.restored_occurrences() == 4 && staged.output_tokens().len() == 1,
                "policy_json_mixed_ledger_not_shared"
            );
            assert!(
                staged.as_value()["content"] == json!(TEXT)
                    && staged.as_value()["refusal"] == json!(TEXT)
                    && staged.as_value()["reasoning"] == source["reasoning"],
                "policy_json_mixed_policy_erased"
            );
            let decoded: Value = serde_json::from_str(
                staged.as_value()["arguments"]
                    .as_str()
                    .ok_or("policy_json_mixed_arguments_missing")?,
            )?;
            assert!(
                decoded["value"] == json!(TEXT) && decoded["nested"]["again"] == json!(TEXT),
                "policy_json_mixed_arguments_not_decoded"
            );
            drop(staged);
            drop(observed);
        }
        assert!(
            fixture.budget.used() == baseline && fixture.counts()? == (0, 0, 0),
            "policy_json_mixed_or_fault_retained_authority"
        );
    }
    Ok(())
}

// Opaque snapshot fixtures contain synthetic values only; diagnostics use fixed labels.
fn ch2_core_opaque_sources(target: usize, values: &[Option<Value>]) -> Vec<Value> {
    values
        .iter()
        .enumerate()
        .map(|(position, value)| {
            let mut delta = json!({"content":if position == 0 {"A"} else {"B"}});
            if matches!(target, 1 | 2) {
                delta["tool_calls"] = json!([{"index":5,"id":"synthetic-opaque-call","type":"function","function":{"name":"synthetic-tool","arguments":if position == 0 {"{}"} else {""}}}]);
            }
            if let Some(value) = value {
                match target {
                    1 => delta["tool_calls"][0]["extension"] = value.clone(),
                    2 => delta["tool_calls"][0]["function"]["extension"] = value.clone(),
                    3 => delta["reasoning"] = value.clone(),
                    _ => delta["extension"] = value.clone(),
                }
            }
            json!({"id":"synthetic-opaque-response","choices":[{"index":7,"delta":delta,"finish_reason":if position + 1 == values.len() {if matches!(target,1|2) {Some("tool_calls")} else {Some("stop")}} else {None}}]})
        })
        .collect()
}

fn ch2_core_opaque_expected(target: usize, value: &Value, content: &str) -> Value {
    let mut message = json!({"role":"assistant","content":content});
    if matches!(target, 1 | 2) {
        let mut call = json!({"id":"synthetic-opaque-call","type":"function","function":{"name":"synthetic-tool","arguments":"{}"}});
        if target == 1 {
            call["extension"] = value.clone();
        } else {
            call["function"]["extension"] = value.clone();
        }
        message["tool_calls"] = json!([call]);
        json!([message, call])
    } else {
        message[if target == 3 {
            "reasoning"
        } else {
            "extension"
        }] = value.clone();
        json!([message])
    }
}

fn ch2_core_opaque_stage_run(
    fixture: &super::native_test_support::NativeFixture,
    sources: &[Value],
    expected: Option<&Value>,
    cancellation: usize,
) -> TestResult {
    use super::{
        native_test_support::chat,
        response_restoration::{finalize_native_sse, serialize_json_with_budget},
        restoration_contract::{ProtocolPair, TextSites, UpstreamProtocol},
        restoration_core::{
            stage_native_sse_with_lifecycle, NativeSseFrameObservation, NativeSseObservation,
            NativeSseSnapshotSelection,
        },
    };
    use crate::proxy::protocol_bridge::Protocol;
    use futures::StreamExt;
    let baseline = fixture.budget.used();
    let wire = ch2_core_sse_wire(sources, true);
    let proof =
        ch2_core_sse_proof(fixture, &wire).map_err(|_| "opaque_original_http_proof_rejected")?;
    let mut frames = Vec::new();
    let mut offset = 0;
    for source in sources {
        let end = offset + format!("data: {source}\n\n").len();
        frames.push(
            NativeSseFrameObservation::new(
                &wire[offset..end],
                Some(source),
                TextSites::new(fixture.budget.clone())?,
                fixture.budget.clone(),
            )
            .map_err(|_| "opaque_frame_rejected")?,
        );
        offset = end;
    }
    frames.push(
        NativeSseFrameObservation::new(
            &wire[offset..],
            None,
            TextSites::new(fixture.budget.clone())?,
            fixture.budget.clone(),
        )
        .map_err(|_| "opaque_done_frame_rejected")?,
    );
    let refs: Vec<_> = frames.iter().collect();
    let observed = NativeSseObservation::new(
        wire.as_bytes(),
        &refs,
        chat(),
        &fixture.lease,
        &fixture.binding,
        fixture.budget.clone(),
    )
    .map_err(|_| "opaque_observation_rejected")?;
    let mut selections = Vec::new();
    for (frame, source) in sources.iter().enumerate() {
        for choice in source["choices"]
            .as_array()
            .ok_or("opaque_choices_missing")?
        {
            selections.push(NativeSseSnapshotSelection::chat_choice(frame, choice));
        }
    }
    let occupied = fixture.budget.used();
    let staged = stage_native_sse_with_lifecycle(
        &observed,
        &selections,
        &[],
        &fixture.lease,
        &fixture.binding,
        fixture.budget.clone(),
        ProtocolPair::new(chat(), UpstreamProtocol::new(Protocol::Chat)),
        &fixture.attempt,
    );
    if let Some(expected) = expected {
        let staged = staged.map_err(|_| "opaque_supported_stage_rejected")?;
        assert!(
            staged.delivered() == expected,
            "opaque_independent_history_mismatch"
        );
        let actual = serialize_json_with_budget(staged.delivered(), &fixture.budget, 1024 * 1024)
            .map_err(|_| "opaque_actual_encoding_rejected")?;
        let wanted = serialize_json_with_budget(expected, &fixture.budget, 1024 * 1024)
            .map_err(|_| "opaque_expected_encoding_rejected")?;
        assert!(
            actual.as_slice() == wanted.as_slice(),
            "opaque_first_value_or_history_order_changed"
        );
        drop((actual, wanted));
        assert!(
            staged.restored_occurrences() == 0 && staged.output_tokens().is_empty(),
            "opaque_granted_token_dependency"
        );
        assert!(
            fixture.counts()? == (0, 0, 0),
            "opaque_stage_published_history"
        );
        let finalized =
            finalize_native_sse(staged, &fixture.lease, proof, &fixture.attempt, 1024 * 1024)
                .map_err(|_| "opaque_finalization_rejected")?;
        let mut delivery = finalized.into_delivery_stream();
        let mut received = Vec::new();
        if cancellation != 1 {
            for _ in 0..frames.len() {
                let chunk = futures::executor::block_on(delivery.next())
                    .ok_or("opaque_frame_missing")??;
                received.extend_from_slice(chunk.as_ref());
                assert!(
                    fixture.counts()? == (0, 1, 1),
                    "opaque_published_before_last_continuation"
                );
            }
            assert!(received == wire.as_bytes(), "opaque_original_wire_changed");
        }
        if cancellation == 0 {
            assert!(
                futures::executor::block_on(delivery.next()).is_none(),
                "opaque_extra_frame"
            );
            assert!(
                fixture
                    .lease
                    .verify_native_delivered(&fixture.binding, chat(), expected)
                    .is_ok(),
                "opaque_delivered_history_not_authorized"
            );
        }
        drop(delivery);
        if cancellation != 0 {
            assert!(
                fixture.counts()? == (0, 0, 0)
                    && fixture
                        .lease
                        .verify_native_delivered(&fixture.binding, chat(), expected)
                        .is_err(),
                "opaque_cancel_published_history"
            );
        }
    } else {
        assert!(staged.is_err(), "opaque_conflicting_snapshot_accepted");
        drop(staged);
        drop(proof);
        assert!(
            fixture.budget.used() == occupied && fixture.counts()? == (0, 0, 0),
            "opaque_conflict_not_atomic"
        );
    }
    drop(observed);
    drop(frames);
    assert!(
        fixture.budget.used() == baseline,
        "opaque_snapshot_charge_retained"
    );
    Ok(())
}

fn ch2_core_opaque_conflict_run(target: usize, case: usize) -> TestResult {
    use super::native_test_support::NativeFixture;
    let fixture = NativeFixture::new(true)?;
    let (left, right) = match case {
        0 => (json!({"nested":{"value":1}}), json!({"nested":{"value":2}})),
        1 => (json!([1, 2]), json!([2, 1])),
        2 => (Value::Null, json!({"value":1})),
        3 => (json!({"value":1}), Value::Null),
        4 => (json!([1]), json!({"value":1})),
        _ => (json!("synthetic-first"), json!("synthetic-second")),
    };
    let sources = ch2_core_opaque_sources(target, &[Some(left), Some(right)]);
    ch2_core_opaque_stage_run(&fixture, &sources, None, 0)
}

macro_rules! ch2_opaque_conflict_test {
    ($name:ident, $target:expr, $case:expr) => {
        #[test]
        fn $name() -> TestResult {
            ch2_core_opaque_conflict_run($target, $case)
        }
    };
}
ch2_opaque_conflict_test!(ch2_core_opaque_conflict_message_object, 0, 0);
ch2_opaque_conflict_test!(ch2_core_opaque_conflict_tool_object, 1, 0);
ch2_opaque_conflict_test!(ch2_core_opaque_conflict_function_object, 2, 0);
ch2_opaque_conflict_test!(ch2_core_opaque_conflict_reasoning_object, 3, 0);
ch2_opaque_conflict_test!(ch2_core_opaque_conflict_message_array_order, 0, 1);
ch2_opaque_conflict_test!(ch2_core_opaque_conflict_tool_array_order, 1, 1);
ch2_opaque_conflict_test!(ch2_core_opaque_conflict_function_type_switch, 2, 4);
ch2_opaque_conflict_test!(ch2_core_opaque_conflict_reasoning_type_switch, 3, 4);
ch2_opaque_conflict_test!(ch2_core_opaque_conflict_message_null_value, 0, 2);
ch2_opaque_conflict_test!(ch2_core_opaque_conflict_message_value_null, 0, 3);
ch2_opaque_conflict_test!(ch2_core_opaque_conflict_tool_null_value, 1, 2);
ch2_opaque_conflict_test!(ch2_core_opaque_conflict_tool_value_null, 1, 3);
ch2_opaque_conflict_test!(ch2_core_opaque_conflict_function_null_value, 2, 2);
ch2_opaque_conflict_test!(ch2_core_opaque_conflict_function_value_null, 2, 3);
ch2_opaque_conflict_test!(ch2_core_opaque_conflict_reasoning_null_value, 3, 2);
ch2_opaque_conflict_test!(ch2_core_opaque_conflict_reasoning_value_null, 3, 3);
ch2_opaque_conflict_test!(ch2_core_opaque_conflict_message_unknown_string, 0, 5);
ch2_opaque_conflict_test!(ch2_core_opaque_conflict_tool_unknown_string, 1, 5);
ch2_opaque_conflict_test!(ch2_core_opaque_conflict_function_unknown_string, 2, 5);

#[test]
fn ch2_core_opaque_resource_aggregate_equal_repeats_exceed_shared_work() -> TestResult {
    use super::{payload_budget::PayloadBudget, restoration_core::compose_chat_snapshots};
    let budget = PayloadBudget::with_limit(16 * 1024 * 1024);
    let sources = ch2_core_opaque_sources(0, &vec![Some(json!("x".repeat(64 * 1024))); 80]);
    let choices: Vec<_> = sources.iter().map(|s| &s["choices"][0]).collect();
    let result = compose_chat_snapshots(&choices, &budget);
    assert!(result.is_err(), "opaque_aggregate_work_limit_missing");
    drop(result);
    assert!(budget.used() == 0, "opaque_work_rejection_charge_retained");
    Ok(())
}

#[test]
fn ch2_core_opaque_supported_single_omitted_and_equal_values() -> TestResult {
    use super::native_test_support::NativeFixture;
    for target in 0..4 {
        for value in [
            Value::Null,
            json!(true),
            json!(7),
            json!({"z":[1,null,{"a":false}],"a":"synthetic"}),
            json!([1, 2]),
        ] {
            let fixture = NativeFixture::new(true)?;
            let expected = ch2_core_opaque_expected(target, &value, "ABB");
            let sources =
                ch2_core_opaque_sources(target, &[Some(value.clone()), None, Some(value)]);
            ch2_core_opaque_stage_run(&fixture, &sources, Some(&expected), 0)?;
        }
    }
    Ok(())
}

#[test]
fn ch2_core_opaque_supported_reordered_object_preserves_first_order() -> TestResult {
    use super::native_test_support::NativeFixture;
    for target in 0..4 {
        let fixture = NativeFixture::new(false)?;
        let first = json!({"z":1,"a":2});
        let second = json!({"a":2,"z":1});
        let expected = ch2_core_opaque_expected(target, &first, "AB");
        let sources = ch2_core_opaque_sources(target, &[Some(first), Some(second)]);
        ch2_core_opaque_stage_run(&fixture, &sources, Some(&expected), 0)?;
    }
    Ok(())
}

#[test]
fn ch2_core_opaque_supported_nested_reference_is_not_inspected() -> TestResult {
    use super::native_test_support::NativeFixture;
    for target in 0..4 {
        let fixture = NativeFixture::new(true)?;
        let value = json!({"opaque":[fixture.reference]});
        let expected = ch2_core_opaque_expected(target, &value, "AB");
        let sources = ch2_core_opaque_sources(target, &[Some(value.clone()), Some(value)]);
        ch2_core_opaque_stage_run(&fixture, &sources, Some(&expected), 0)?;
    }
    Ok(())
}

#[test]
fn ch2_core_opaque_final_wire_and_candidate_contradictions_reject() -> TestResult {
    use super::native_test_support::{chat, NativeFixture};
    for target in 0..4 {
        for mode in 0..5 {
            let fixture = NativeFixture::new(true)?;
            let baseline = fixture.budget.used();
            let value = json!({"opaque":[1,2]});
            let sources =
                ch2_core_opaque_sources(target, &[Some(value.clone()), Some(value.clone())]);
            let original = ch2_core_sse_wire(&sources, true);
            let proof = ch2_core_sse_proof(&fixture, &original)
                .map_err(|_| "opaque_final_original_proof_rejected")?;
            let mut final_sources = sources.clone();
            let mut expected = ch2_core_opaque_expected(target, &value, "AB");
            if mode == 1 {
                final_sources = ch2_core_opaque_sources(
                    target,
                    &[Some(json!({"opaque":[2,1]})), Some(json!({"opaque":[2,1]}))],
                );
            } else if mode == 2 {
                // A shared last-value-wins composer would hide this earlier contradiction.
                final_sources = ch2_core_opaque_sources(
                    target,
                    &[Some(json!({"opaque":[2,1]})), Some(value.clone())],
                );
            } else if mode == 3 {
                expected[0]["content"] = json!("synthetic-contradiction");
            } else if mode == 4 {
                if matches!(target, 1 | 2) {
                    expected
                        .as_array_mut()
                        .ok_or("opaque_expected_array_missing")?
                        .swap(0, 1);
                } else {
                    expected[0][if target == 3 {
                        "reasoning"
                    } else {
                        "extension"
                    }] = json!({"opaque":[2,1]});
                }
            }
            let final_wire = ch2_core_sse_wire(&final_sources, true);
            let result = proof.validate_restored_chat_snapshots(
                original.as_bytes(),
                &fixture.binding,
                &fixture.attempt,
                chat(),
                std::io::Cursor::new(final_wire.as_bytes()),
                &fixture.budget,
                &expected,
            );
            assert!(
                result.is_ok() == (mode == 0),
                "opaque_final_contradiction_or_control_wrong"
            );
            assert!(
                fixture.counts()? == (0, 0, 0) && fixture.budget.used() == baseline,
                "opaque_final_check_retained_or_published"
            );
        }
    }
    Ok(())
}

#[test]
fn ch2_core_opaque_delivery_unpolled_and_last_yield_cancel_refund() -> TestResult {
    use super::native_test_support::NativeFixture;
    for target in 0..4 {
        for cancellation in 1..=2 {
            let fixture = NativeFixture::new(true)?;
            let value = json!({"opaque":[fixture.reference]});
            let sources =
                ch2_core_opaque_sources(target, &[Some(value.clone()), Some(value.clone())]);
            let expected = ch2_core_opaque_expected(target, &value, "AB");
            ch2_core_opaque_stage_run(&fixture, &sources, Some(&expected), cancellation)?;
        }
    }
    Ok(())
}

#[test]
fn ch2_core_opaque_resource_work_shared_across_fields_choices_and_call_emission() -> TestResult {
    use super::{payload_budget::PayloadBudget, restoration_core::compose_chat_snapshots};
    for mode in 0..3 {
        let budget = PayloadBudget::with_limit(16 * 1024 * 1024);
        let value = json!("x".repeat(64 * 1024));
        let mut sources = ch2_core_opaque_sources(
            if mode == 2 { 1 } else { 0 },
            &vec![Some(value.clone()); 40],
        );
        if mode == 0 {
            for source in &mut sources {
                source["choices"][0]["delta"]["second"] = value.clone();
            }
        } else if mode == 1 {
            let others = ch2_core_opaque_sources(0, &vec![Some(value); 40]);
            for (source, mut other) in sources.iter_mut().zip(others) {
                other["choices"][0]["index"] = json!(2);
                source["choices"]
                    .as_array_mut()
                    .ok_or("opaque_choices_missing")?
                    .push(other["choices"][0].clone());
            }
        }
        let choices: Vec<_> = sources
            .iter()
            .flat_map(|s| s["choices"].as_array().into_iter().flatten())
            .collect();
        let result = compose_chat_snapshots(&choices, &budget);
        assert!(result.is_err(), "opaque_work_was_reset_between_owners");
        drop(result);
        assert!(budget.used() == 0, "opaque_shared_work_retained_charge");
    }
    Ok(())
}

#[test]
fn ch2_core_opaque_resource_first_value_depth_size_and_low_budget_refund() -> TestResult {
    use super::{payload_budget::PayloadBudget, restoration_core::compose_chat_snapshots};
    for case in 0..4 {
        let budget = PayloadBudget::with_limit(if case == 3 { 128 } else { 16 * 1024 * 1024 });
        let mut value = json!("synthetic");
        if case < 2 {
            for _ in 0..70 {
                value = json!([value]);
            }
        }
        if case == 2 {
            value = json!("x".repeat(1024 * 1024));
        }
        let values = if case == 1 {
            vec![Some(value.clone()), Some(value)]
        } else {
            vec![Some(value)]
        };
        let sources = ch2_core_opaque_sources(0, &values);
        let choices: Vec<_> = sources.iter().map(|s| &s["choices"][0]).collect();
        let result = compose_chat_snapshots(&choices, &budget);
        assert!(result.is_err(), "opaque_first_value_bound_bypassed");
        drop(result);
        assert!(
            budget.used() == 0,
            "opaque_first_value_failure_charge_retained"
        );
    }
    Ok(())
}

#[test]
fn ch2_core_opaque_resource_metadata_and_buffer_faults_refund() -> TestResult {
    use super::{
        payload_budget::PayloadBudget,
        payload_metadata::test_support::{self, Action, Phase},
        restoration_core::compose_chat_snapshots,
    };
    for phase in [Phase::BeforeAllocation, Phase::AfterAllocation] {
        let budget = PayloadBudget::with_limit(16 * 1024 * 1024);
        let sources =
            ch2_core_opaque_sources(2, &[Some(json!({"a":[1,2]})), Some(json!({"a":[1,2]}))]);
        let choices: Vec<_> = sources.iter().map(|s| &s["choices"][0]).collect();
        let reached = std::rc::Rc::new(std::cell::Cell::new(false));
        let seen = reached.clone();
        let hook = test_support::install(move |event| {
            if event.phase == phase {
                seen.set(true);
                Action::Fail
            } else {
                Action::Continue
            }
        });
        let result = compose_chat_snapshots(&choices, &budget);
        drop(hook);
        assert!(
            reached.get() && result.is_err(),
            "opaque_metadata_fault_not_exercised"
        );
        drop(result);
        assert!(budget.used() == 0, "opaque_metadata_fault_charge_retained");
    }
    Ok(())
}

#[test]
fn ch2_core_opaque_supported_known_text_nulls_preserve_joined_values() -> TestResult {
    use super::native_test_support::NativeFixture;
    let fixture = NativeFixture::new(true)?;
    let values = [Some(json!({"stable":1})), None, Some(json!({"stable":1}))];
    let mut sources = ch2_core_opaque_sources(0, &values);
    sources[0]["choices"][0]["delta"]["refusal"] = json!("R");
    sources[0]["choices"][0]["delta"]["reasoning"] = json!("Q");
    sources[0]["choices"][0]["delta"]["reasoning_content"] = json!("T");
    for field in ["content", "refusal", "reasoning", "reasoning_content"] {
        sources[1]["choices"][0]["delta"][field] = Value::Null;
        sources[2]["choices"][0]["delta"][field] = json!("");
    }
    // Build the expected field order independently from the fixture's explicit order.
    let expected = json!([{"role":"assistant","content":"A","extension":{"stable":1},"refusal":"R","reasoning":"Q","reasoning_content":"T"}]);
    ch2_core_opaque_stage_run(&fixture, &sources, Some(&expected), 0)
}

#[test]
fn ch2_core_policy_sse_explicit_tool_arguments_restore_with_one_ledger() -> TestResult {
    ch2_core_typed_sse_run(false, true)
}

#[test]
fn ch2_core_opaque_resource_buffer_allocation_and_copy_faults_refund() -> TestResult {
    use super::{
        payload_budget::PayloadBudget,
        payload_buffer::test_support::{self, Action, Phase},
        restoration_core::compose_chat_snapshots,
    };
    for phase in [Phase::BeforeAllocation, Phase::BeforeCopy] {
        let budget = PayloadBudget::with_limit(16 * 1024 * 1024);
        let sources =
            ch2_core_opaque_sources(1, &[Some(json!({"a":[1,2]})), Some(json!({"a":[1,2]}))]);
        let choices: Vec<_> = sources.iter().map(|s| &s["choices"][0]).collect();
        let reached = std::rc::Rc::new(std::cell::Cell::new(false));
        let seen = reached.clone();
        let hook = test_support::install(move |event| {
            if event.phase == phase {
                seen.set(true);
                Action::FailAllocation
            } else {
                Action::Continue
            }
        });
        let result = compose_chat_snapshots(&choices, &budget);
        drop(hook);
        assert!(
            reached.get() && result.is_err(),
            "opaque_buffer_fault_not_exercised"
        );
        drop(result);
        assert!(budget.used() == 0, "opaque_buffer_fault_charge_retained");
    }
    Ok(())
}

#[test]
fn ch2_core_opaque_final_immutable_restoration_conflict_never_releases() -> TestResult {
    use super::{
        native_test_support::{chat, NativeFixture},
        restoration_contract::{
            EditConstraint, ProtocolPair, SourceClass, TextSites, UpstreamProtocol,
        },
        restoration_core::{
            stage_native_sse_with_lifecycle, NativeSseFrameObservation, NativeSseObservation,
            NativeSseSnapshotSelection, NativeSseTextStream,
        },
    };
    use crate::proxy::protocol_bridge::Protocol;
    for immutable in [false, true] {
        let fixture = NativeFixture::new(true)?;
        let baseline = fixture.budget.used();
        let opaque = json!({"stable":[fixture.reference]});
        let mut sources = ch2_core_opaque_sources(0, &[Some(opaque.clone()), Some(opaque)]);
        sources[0]["choices"][0]["delta"]["content"] = json!(fixture.reference);
        sources[1]["choices"][0]["delta"]["content"] = Value::Null;
        let wire = ch2_core_sse_wire(&sources, true);
        let proof = ch2_core_sse_proof(&fixture, &wire)
            .map_err(|_| "opaque_immutable_original_proof_rejected")?;
        let node = &sources[0]["choices"][0]["delta"]["content"];
        let mut frames = Vec::new();
        let mut offset = 0;
        for (position, source) in sources.iter().enumerate() {
            let end = offset + format!("data: {source}\n\n").len();
            let mut sites = TextSites::new(fixture.budget.clone())?;
            if position == 0 {
                sites.observe(
                    node,
                    SourceClass::HistoricalText,
                    if immutable {
                        EditConstraint::Immutable
                    } else {
                        EditConstraint::Editable
                    },
                )?;
            }
            frames.push(
                NativeSseFrameObservation::new(
                    &wire[offset..end],
                    Some(source),
                    sites,
                    fixture.budget.clone(),
                )
                .map_err(|_| "opaque_immutable_frame_rejected")?,
            );
            offset = end;
        }
        frames.push(
            NativeSseFrameObservation::new(
                &wire[offset..],
                None,
                TextSites::new(fixture.budget.clone())?,
                fixture.budget.clone(),
            )
            .map_err(|_| "opaque_immutable_done_rejected")?,
        );
        let refs: Vec<_> = frames.iter().collect();
        let observed = NativeSseObservation::new(
            wire.as_bytes(),
            &refs,
            chat(),
            &fixture.lease,
            &fixture.binding,
            fixture.budget.clone(),
        )
        .map_err(|_| "opaque_immutable_observation_rejected")?;
        let selections = [
            NativeSseSnapshotSelection::chat_choice(0, &sources[0]["choices"][0]),
            NativeSseSnapshotSelection::chat_choice(1, &sources[1]["choices"][0]),
        ];
        let fragments = [node];
        let streams = [NativeSseTextStream::new(&fragments)];
        let occupied = fixture.budget.used();
        let staged = stage_native_sse_with_lifecycle(
            &observed,
            &selections,
            &streams,
            &fixture.lease,
            &fixture.binding,
            fixture.budget.clone(),
            ProtocolPair::new(chat(), UpstreamProtocol::new(Protocol::Chat)),
            &fixture.attempt,
        );
        assert!(
            staged.is_ok() != immutable,
            "opaque_immutable_restore_boundary_wrong"
        );
        drop(staged);
        drop(proof);
        assert!(
            fixture.counts()? == (0, 0, 0) && fixture.budget.used() == occupied,
            "opaque_immutable_released_or_retained"
        );
        drop(observed);
        drop(frames);
        assert!(
            fixture.budget.used() == baseline,
            "opaque_immutable_charge_retained"
        );
    }
    Ok(())
}

#[test]
fn ch2_core_opaque_supported_single_values_and_unknown_equal_strings() -> TestResult {
    use super::native_test_support::NativeFixture;
    for target in 0..4 {
        for value in [
            Value::Null,
            json!(false),
            json!(0),
            json!("synthetic-opaque"),
            json!([]),
            json!({"z":[],"a":null}),
        ] {
            let fixture = NativeFixture::new(false)?;
            let sources = ch2_core_opaque_sources(target, &[Some(value.clone())]);
            let expected = ch2_core_opaque_expected(target, &value, "A");
            ch2_core_opaque_stage_run(&fixture, &sources, Some(&expected), 0)?;
            if target != 3 {
                let fixture = NativeFixture::new(false)?;
                let sources =
                    ch2_core_opaque_sources(target, &[Some(value.clone()), Some(value.clone())]);
                let expected = ch2_core_opaque_expected(target, &value, "AB");
                ch2_core_opaque_stage_run(&fixture, &sources, Some(&expected), 0)?;
            }
        }
    }
    Ok(())
}

#[test]
fn ch2_core_opaque_conflict_scalar_and_reasoning_text_structure_switches() -> TestResult {
    use super::native_test_support::NativeFixture;
    for target in 0..4 {
        for (left, right) in [
            (json!(true), json!(false)),
            (json!(1), json!(2)),
            (json!("synthetic"), json!({"a":1})),
            (json!({"a":1}), json!("synthetic")),
        ] {
            let fixture = NativeFixture::new(false)?;
            let sources = ch2_core_opaque_sources(target, &[Some(left), Some(right)]);
            ch2_core_opaque_stage_run(&fixture, &sources, None, 0)?;
        }
    }
    Ok(())
}

#[test]
fn ch2_core_opaque_supported_interleaved_choices_and_sparse_tools_keep_complete_order() -> TestResult
{
    use super::native_test_support::NativeFixture;
    let fixture = NativeFixture::new(true)?;
    let opaque = json!({"z":[fixture.reference],"a":true});
    let mut sources = ch2_core_opaque_sources(1, &[Some(opaque.clone()), Some(opaque.clone())]);
    let other = ch2_core_opaque_sources(0, &[Some(opaque.clone()), Some(opaque.clone())]);
    for (position, source) in sources.iter_mut().enumerate() {
        let mut choice = other[position]["choices"][0].clone();
        choice["index"] = json!(2);
        let mut tool = source["choices"][0]["delta"]["tool_calls"][0].clone();
        tool["index"] = json!(97);
        tool["id"] = json!("synthetic-second-call");
        source["choices"][0]["delta"]["tool_calls"]
            .as_array_mut()
            .ok_or("opaque_tools_missing")?
            .push(tool);
        source["choices"]
            .as_array_mut()
            .ok_or("opaque_choices_missing")?
            .push(choice);
    }
    // The second frame reverses choice interleaving, not first-observation history order.
    sources[1]["choices"]
        .as_array_mut()
        .ok_or("opaque_choices_missing")?
        .swap(0, 1);
    let first = json!({"id":"synthetic-opaque-call","type":"function","function":{"name":"synthetic-tool","arguments":"{}"},"extension":opaque});
    let second = json!({"id":"synthetic-second-call","type":"function","function":{"name":"synthetic-tool","arguments":"{}"},"extension":opaque});
    let expected = json!([{"role":"assistant","content":"AB","tool_calls":[first,second]},first,second,{"role":"assistant","content":"AB","extension":opaque}]);
    ch2_core_opaque_stage_run(&fixture, &sources, Some(&expected), 0)
}

// Same-class stream claims must not merge parallel choices or native carriers.
fn ch2_core_isolation_run(mode: usize, explicit: bool, composed: bool) -> TestResult {
    use super::{
        native_test_support::{chat, NativeFixture, TEXT},
        response_restoration::finalize_native_sse,
        restoration_contract::{
            EditConstraint, ProtocolPair, SourceClass, TextPolicy, TextSites, UpstreamProtocol,
        },
        restoration_core::{
            stage_native_sse_with_lifecycle, NativeSseFrameObservation, NativeSseObservation,
            NativeSseSnapshotSelection, NativeSseTextStream,
        },
    };
    use crate::proxy::protocol_bridge::Protocol;
    use futures::StreamExt;
    let fixture = NativeFixture::new(true)?;
    let baseline = fixture.budget.used();
    let split = fixture.reference.len() / 2;
    let first = &fixture.reference[..split];
    let last = &fixture.reference[split..];
    // Mode 0 mixes choices; mode 1 mixes content/refusal in one choice;
    // Mode 2 swaps choice-local calls; modes 3/4 keep choices/calls isolated.
    let arguments = mode == 2 || mode == 4;
    let sources = if arguments {
        [
            json!({"id":"synthetic-isolation","choices":[{"index":9,"delta":{"role":"assistant","tool_calls":[
                {"index":5,"id":"synthetic-a","type":"function","function":{"name":"synthetic-tool","arguments":format!("{{\"value\":\"{first}")}},
                {"index":11,"id":"synthetic-b","type":"function","function":{"name":"synthetic-tool","arguments":format!("{{\"value\":\"{first}")}}
            ]},"finish_reason":null}]}),
            json!({"id":"synthetic-isolation","choices":[{"index":9,"delta":{"tool_calls":[
                {"index":11,"function":{"arguments":format!("{last}\"}}")}},
                {"index":5,"function":{"arguments":format!("{last}\"}}")}}
            ]},"finish_reason":"tool_calls"}]}),
        ]
    } else if mode == 1 {
        [
            json!({"id":"synthetic-isolation","choices":[{"index":9,"delta":{"role":"assistant","content":first},"finish_reason":null}]}),
            json!({"id":"synthetic-isolation","choices":[{"index":9,"delta":{"refusal":last},"finish_reason":"stop"}]}),
        ]
    } else {
        [
            json!({"id":"synthetic-isolation","choices":[
                {"index":9,"delta":{"role":"assistant","content":first},"finish_reason":null},
                {"index":2,"delta":{"role":"assistant","content":first},"finish_reason":null}]}),
            json!({"id":"synthetic-isolation","choices":[
                {"index":2,"delta":{"content":last},"finish_reason":"stop"},
                {"index":9,"delta":{"content":last},"finish_reason":"stop"}]}),
        ]
    };
    // Keep the inputs owned and stable through all borrowed observations.
    let original = sources.clone();
    let wire = ch2_core_sse_wire(&sources, true);
    let proof = ch2_core_sse_proof(&fixture, &wire)
        .map_err(|_| "isolation_original_http_proof_rejected")?;
    let nodes: Vec<&Value> = if arguments {
        sources
            .iter()
            .flat_map(|source| {
                source["choices"][0]["delta"]["tool_calls"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(|tool| &tool["function"]["arguments"])
            })
            .collect()
    } else if mode == 1 {
        vec![
            &sources[0]["choices"][0]["delta"]["content"],
            &sources[1]["choices"][0]["delta"]["refusal"],
        ]
    } else {
        sources
            .iter()
            .flat_map(|source| {
                source["choices"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(|choice| &choice["delta"]["content"])
            })
            .collect()
    };
    let mut frames = Vec::new();
    let mut offset = 0;
    let per_frame = nodes.len() / 2;
    for (index, source) in sources.iter().enumerate() {
        let end = offset + format!("data: {source}\n\n").len();
        let mut sites = TextSites::new(fixture.budget.clone())?;
        for node in &nodes[index * per_frame..(index + 1) * per_frame] {
            sites.observe(
                node,
                if arguments {
                    SourceClass::ToolArguments
                } else {
                    SourceClass::HistoricalText
                },
                EditConstraint::Editable,
            )?;
        }
        frames.push(
            NativeSseFrameObservation::new(
                &wire[offset..end],
                Some(source),
                sites,
                fixture.budget.clone(),
            )
            .map_err(|_| "isolation_frame_rejected")?,
        );
        offset = end;
    }
    frames.push(
        NativeSseFrameObservation::new(
            &wire[offset..],
            None,
            TextSites::new(fixture.budget.clone())?,
            fixture.budget.clone(),
        )
        .map_err(|_| "isolation_done_rejected")?,
    );
    let refs: Vec<_> = frames.iter().collect();
    let observed = NativeSseObservation::new(
        wire.as_bytes(),
        &refs,
        chat(),
        &fixture.lease,
        &fixture.binding,
        fixture.budget.clone(),
    )
    .map_err(|_| "isolation_observation_rejected")?;
    let selections: Vec<_> = sources
        .iter()
        .enumerate()
        .flat_map(|(index, source)| {
            source["choices"]
                .as_array()
                .into_iter()
                .flatten()
                .map(move |choice| {
                    if composed {
                        NativeSseSnapshotSelection::chat_choice(index, choice)
                    } else {
                        NativeSseSnapshotSelection::new(index, &choice["delta"])
                    }
                })
        })
        .collect();
    let groups: Vec<Vec<&Value>> = if per_frame == 1 {
        vec![nodes.clone()]
    } else if mode >= 3 {
        vec![vec![nodes[0], nodes[3]], vec![nodes[1], nodes[2]]]
    } else {
        vec![vec![nodes[0], nodes[2]], vec![nodes[1], nodes[3]]]
    };
    let streams: Vec<_> = groups
        .iter()
        .map(|group| {
            if explicit {
                NativeSseTextStream::with_policy(
                    group,
                    if arguments {
                        TextPolicy::ToolArguments
                    } else {
                        TextPolicy::Output
                    },
                )
            } else {
                NativeSseTextStream::new(group)
            }
        })
        .collect();
    let occupied = fixture.budget.used();
    let staged = stage_native_sse_with_lifecycle(
        &observed,
        &selections,
        &streams,
        &fixture.lease,
        &fixture.binding,
        fixture.budget.clone(),
        ProtocolPair::new(chat(), UpstreamProtocol::new(Protocol::Chat)),
        &fixture.attempt,
    );
    if mode < 3 {
        let rejected = staged.is_err();
        drop(staged);
        assert!(
            fixture.counts()? == (0, 0, 0) && fixture.budget.used() == occupied,
            "isolation_rejection_not_atomic"
        );
        drop(proof);
        drop(observed);
        drop(frames);
        assert!(
            fixture.budget.used() == baseline,
            "isolation_rejection_charge_retained"
        );
        assert!(
            rejected,
            "isolation_cross_choice_call_or_carrier_stream_accepted"
        );
    } else {
        let staged = staged.map_err(|_| "isolation_control_stage_rejected")?;
        assert!(
            staged.restored_occurrences() == 2
                && staged.output_tokens().len() == 1
                && fixture.counts()? == (0, 0, 0),
            "isolation_control_ledger_or_authority_changed"
        );
        let finalized =
            finalize_native_sse(staged, &fixture.lease, proof, &fixture.attempt, 1024 * 1024)
                .map_err(|_| "isolation_control_finalization_rejected")?;
        let mut delivery = finalized.into_delivery_stream();
        for _ in 0..3 {
            let chunk = futures::executor::block_on(delivery.next())
                .ok_or("isolation_control_frame_missing")??;
            drop(chunk);
            assert!(
                fixture.counts()? == (0, 1, 1),
                "isolation_control_early_publication"
            );
        }
        assert!(
            futures::executor::block_on(delivery.next()).is_none(),
            "isolation_control_extra_frame"
        );
        drop(delivery);
        let expected = if arguments {
            let args = serde_json::to_string(&json!({"value":TEXT}))?;
            let first = json!({"id":"synthetic-a","type":"function","function":{"name":"synthetic-tool","arguments":args}});
            let second = json!({"id":"synthetic-b","type":"function","function":{"name":"synthetic-tool","arguments":args}});
            json!([{"role":"assistant","tool_calls":[first,second]},first,second])
        } else {
            json!([{"role":"assistant","content":TEXT},{"role":"assistant","content":TEXT}])
        };
        assert!(
            fixture
                .lease
                .verify_native_delivered(&fixture.binding, chat(), &expected)
                .is_ok(),
            "isolation_control_history_changed"
        );
        drop(observed);
        drop(frames);
        assert!(
            fixture.budget.used() == baseline,
            "isolation_control_charge_retained"
        );
    }
    assert!(sources == original, "isolation_original_mutated");
    Ok(())
}

#[test]
fn ch2_core_isolation_cross_choice_default_stream_rejects() -> TestResult {
    ch2_core_isolation_run(0, false, true)
}
#[test]
fn ch2_core_isolation_cross_choice_explicit_stream_rejects() -> TestResult {
    ch2_core_isolation_run(0, true, true)
}
#[test]
fn ch2_core_isolation_cross_choice_legacy_selection_rejects() -> TestResult {
    ch2_core_isolation_run(0, false, false)
}
#[test]
fn ch2_core_isolation_cross_carrier_same_choice_rejects() -> TestResult {
    ch2_core_isolation_run(1, false, true)
}
#[test]
fn ch2_core_isolation_cross_call_default_stream_rejects() -> TestResult {
    ch2_core_isolation_run(2, false, true)
}
#[test]
fn ch2_core_isolation_cross_call_explicit_stream_rejects() -> TestResult {
    ch2_core_isolation_run(2, true, true)
}
#[test]
fn ch2_core_isolation_reordered_choices_positive_control() -> TestResult {
    ch2_core_isolation_run(3, false, true)
}

#[test]
fn ch2_core_isolation_reordered_sparse_calls_positive_control() -> TestResult {
    ch2_core_isolation_run(4, true, true)
}

// A carrier's original sequence, not an adapter's partition, defines matching text.
fn ch2_core_continuity_run(mode: usize, explicit: bool, composed: bool) -> TestResult {
    use super::{
        native_test_support::{chat, NativeFixture},
        restoration_contract::{
            EditConstraint, ProtocolPair, SourceClass, TextPolicy, TextSites, UpstreamProtocol,
        },
        restoration_core::{
            stage_native_sse_with_lifecycle, NativeSseFrameObservation, NativeSseObservation,
            NativeSseSnapshotSelection, NativeSseTextStream,
        },
    };
    use crate::proxy::protocol_bridge::Protocol;
    let fixture = NativeFixture::new(true)?;
    let baseline = fixture.budget.used();
    // Split inside the reference header, before it can become a valid token prefix.
    let split = 2;
    let pieces = [
        &fixture.reference[..split],
        "!",
        &fixture.reference[split..],
    ];
    let arguments = mode == 4;
    let sources: Vec<_> = pieces
        .iter()
        .enumerate()
        .map(|(index, piece)| {
            let mut delta = if arguments {
                let text = match index {
                    0 => format!("{{\"value\":\"{piece}"),
                    2 => format!("{piece}\"}}"),
                    _ => piece.to_string(),
                };
                json!({"tool_calls":[{"index":97,"function":{"arguments":text}}]})
            } else {
                json!({"content":piece})
            };
            if index == 0 {
                delta["role"] = json!("assistant");
                if arguments {
                    delta["tool_calls"][0]["id"] = json!("synthetic-continuity-call");
                    delta["tool_calls"][0]["type"] = json!("function");
                    delta["tool_calls"][0]["function"]["name"] = json!("synthetic-tool");
                }
            }
            json!({"id":"synthetic-continuity","choices":[{"index":9,"delta":delta,
                "finish_reason":if index == 2 { Some(if arguments { "tool_calls" } else { "stop" }) } else { None }}]})
        })
        .collect();
    let original = sources.clone();
    let wire = ch2_core_sse_wire(&sources, true);
    let proof =
        ch2_core_sse_proof(&fixture, &wire).map_err(|_| "continuity_original_proof_rejected")?;
    let nodes: Vec<_> = sources
        .iter()
        .map(|source| {
            if arguments {
                &source["choices"][0]["delta"]["tool_calls"][0]["function"]["arguments"]
            } else {
                &source["choices"][0]["delta"]["content"]
            }
        })
        .collect();
    let mut frames = Vec::new();
    let mut offset = 0;
    for (index, source) in sources.iter().enumerate() {
        let end = offset + format!("data: {source}\n\n").len();
        let mut sites = TextSites::new(fixture.budget.clone())?;
        if mode != 2 || index != 1 {
            sites.observe(
                nodes[index],
                if arguments {
                    SourceClass::ToolArguments
                } else {
                    SourceClass::HistoricalText
                },
                EditConstraint::Editable,
            )?;
        }
        frames.push(
            NativeSseFrameObservation::new(
                &wire[offset..end],
                Some(source),
                sites,
                fixture.budget.clone(),
            )
            .map_err(|_| "continuity_frame_rejected")?,
        );
        offset = end;
    }
    frames.push(
        NativeSseFrameObservation::new(
            &wire[offset..],
            None,
            TextSites::new(fixture.budget.clone())?,
            fixture.budget.clone(),
        )
        .map_err(|_| "continuity_done_rejected")?,
    );
    let refs: Vec<_> = frames.iter().collect();
    let observed = NativeSseObservation::new(
        wire.as_bytes(),
        &refs,
        chat(),
        &fixture.lease,
        &fixture.binding,
        fixture.budget.clone(),
    )
    .map_err(|_| "continuity_observation_rejected")?;
    let selections: Vec<_> = sources
        .iter()
        .enumerate()
        .map(|(index, source)| {
            if composed {
                NativeSseSnapshotSelection::chat_choice(index, &source["choices"][0])
            } else {
                NativeSseSnapshotSelection::new(index, &source["choices"][0]["delta"])
            }
        })
        .collect();
    let groups = match mode {
        1 => vec![vec![nodes[0]], vec![nodes[1], nodes[2]]],
        2 => vec![vec![nodes[0], nodes[2]]],
        3 => vec![nodes.clone()],
        _ => vec![vec![nodes[0], nodes[2]], vec![nodes[1]]],
    };
    let streams: Vec<_> = groups
        .iter()
        .map(|group| {
            if explicit {
                NativeSseTextStream::with_policy(
                    group,
                    if arguments {
                        TextPolicy::ToolArguments
                    } else {
                        TextPolicy::Output
                    },
                )
            } else {
                NativeSseTextStream::new(group)
            }
        })
        .collect();
    let occupied = fixture.budget.used();
    let staged = stage_native_sse_with_lifecycle(
        &observed,
        &selections,
        &streams,
        &fixture.lease,
        &fixture.binding,
        fixture.budget.clone(),
        ProtocolPair::new(chat(), UpstreamProtocol::new(Protocol::Chat)),
        &fixture.attempt,
    );
    let rejected = staged.is_err();
    if mode == 3 {
        let staged = staged
            .as_ref()
            .map_err(|_| "continuity_complete_control_rejected")?;
        assert!(
            staged.restored_occurrences() == 0 && staged.output_tokens().is_empty(),
            "continuity_original_sequence_invented_dependency"
        );
    } else if let Ok(staged) = &staged {
        if mode != 1 {
            assert!(
                staged.restored_occurrences() == 1 && staged.output_tokens().len() == 1,
                "continuity_counterexample_did_not_restore"
            );
        }
    }
    drop(staged);
    assert!(
        fixture.counts()? == (0, 0, 0) && fixture.budget.used() == occupied,
        "continuity_rejection_not_atomic"
    );
    drop(proof);
    drop(observed);
    drop(frames);
    assert!(
        fixture.budget.used() == baseline && sources == original,
        "continuity_original_or_refund_changed"
    );
    assert!(
        mode == 3 || rejected,
        "continuity_partition_or_omission_accepted"
    );
    Ok(())
}

#[test]
fn ch2_core_continuity_hopping_default_composed_rejects() -> TestResult {
    ch2_core_continuity_run(0, false, true)
}
#[test]
fn ch2_core_continuity_hopping_explicit_composed_rejects() -> TestResult {
    ch2_core_continuity_run(0, true, true)
}
#[test]
fn ch2_core_continuity_hopping_default_legacy_rejects() -> TestResult {
    ch2_core_continuity_run(0, false, false)
}
#[test]
fn ch2_core_continuity_hopping_explicit_legacy_rejects() -> TestResult {
    ch2_core_continuity_run(0, true, false)
}
#[test]
fn ch2_core_continuity_contiguous_partition_rejects() -> TestResult {
    ch2_core_continuity_run(1, false, true)
}
#[test]
fn ch2_core_continuity_unobserved_middle_rejects() -> TestResult {
    ch2_core_continuity_run(2, false, true)
}
#[test]
fn ch2_core_continuity_sparse_arguments_hopping_rejects() -> TestResult {
    ch2_core_continuity_run(4, true, true)
}
#[test]
fn ch2_core_continuity_complete_original_sequence_control() -> TestResult {
    ch2_core_continuity_run(3, false, true)
}
