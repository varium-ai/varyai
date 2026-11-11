//! Synthetic placeholder compatibility without network or private configuration.
use super::{
    http_restoration::{
        admit_http_request, validate_unleased_enriched_body, RestorationAdmission,
        RestorationGateway,
    },
    payload_budget::PayloadBudget,
    request_plan::PlanField,
    request_preparation::prepare_codex_request_with_budget,
    request_transaction::prepare_fields_with_budget,
    runtime::{self, SecurityRuntimeConfig},
    tokenization::{DlpTokenStore, TokenCategory, TokenError, TokenFragment},
};
use crate::app_config::AppType;
use serde_json::{json, Value};
type TestResult = Result<(), Box<dyn std::error::Error>>;

fn literals() -> Vec<String> {
    vec![
        ["rredact-", "custom-"].concat(),
        ["rredact-", "custom-invalid"].concat(),
        format!("rredact-custom-{}-ai", "a".repeat(20)),
        ["vary_", "cu_"].concat(),
        format!("vary_cu_{}_ai", "b".repeat(20)),
    ]
}
struct Reset(SecurityRuntimeConfig);
impl Drop for Reset {
    fn drop(&mut self) {
        runtime::set_security_runtime_for_test(self.0.clone());
    }
}

#[test]
fn placeholder_passthrough_request_transaction_preserves_literals() -> TestResult {
    for text in literals() {
        let store = DlpTokenStore::new();
        let mut lease = store.admit("codex", Some("synthetic-literal"), 1)?;
        let fields = [PlanField {
            text: &text,
            writable: true,
            matches: &[],
        }];
        let prepared = prepare_fields_with_budget(
            &mut lease,
            &fields,
            &[],
            PayloadBudget::default(),
            |_, proof| {
                assert!(proof.is_empty(), "literal_must_not_grant_authority");
                Ok(())
            },
        )
        .map_err(|_| "literal_transaction_rejected")?;
        assert!(
            prepared.fields.as_slice()[0].as_str() == text,
            "literal_bytes_changed"
        );
        assert!(lease.is_empty(), "literal_granted_authority");
        assert!(store.stats()?.entries == 0, "literal_created_mapping");
    }
    Ok(())
}

#[test]
fn placeholder_passthrough_request_preparation_and_encoded_arguments() -> TestResult {
    for text in literals() {
        let store = DlpTokenStore::new();
        let mut lease = store.admit("codex", Some("synthetic-literal"), 1)?;
        let arguments =
            serde_json::to_string(&json!({"value":text}))?.replace("rredact", "\\u0072redact");
        let body = json!({"instructions":text,"input":[
            {"type":"message","role":"user","content":[{"type":"input_text","text":text}]},
            {"type":"function_call","call_id":"synthetic-call","name":"synthetic-tool","arguments":arguments}
        ]});
        let prepared = prepare_codex_request_with_budget(
            &mut lease,
            &body,
            &SecurityRuntimeConfig::default(),
            &[],
            &[],
            PayloadBudget::default(),
            |candidate, encoded, proof| {
                assert!(proof.is_empty(), "literal_proof_not_empty");
                assert!(
                    serde_json::from_slice::<Value>(encoded).ok().as_ref() == Some(candidate),
                    "serialization_changed"
                );
                Ok(())
            },
        )
        .map_err(|_| "literal_preparation_rejected")?;
        assert!(prepared.body.as_value() == &body, "literal_tree_changed");
    }
    Ok(())
}

#[test]
fn encoded_argument_business_fields_are_not_protocol_dependencies() -> TestResult {
    for field in ["encrypted_content", "encrypted_reasoning"] {
        let arguments = serde_json::to_string(&json!({
            "fixture": [{field: "Synthetic business value"}],
            "source": "Synthetic source\nrredact-custom-"
        }))?;
        let body = json!({"input":[
            {"type":"function_call", "call_id":"synthetic-business-call", "name":"synthetic-tool", "arguments":arguments},
            {"type":"function_call_output", "call_id":"synthetic-business-call", "output":"Synthetic result"}
        ]});
        assert!(
            super::request_preparation::is_fresh_readable_history(
                &body,
                &PayloadBudget::default()
            )?,
            "business_field_became_protocol_dependency"
        );
    }
    Ok(())
}

#[test]
#[serial_test::serial]
fn encoded_argument_business_fields_http_preserve_literal_continuation() -> TestResult {
    let _reset = Reset((*runtime::get_security_runtime()).clone());
    runtime::set_security_runtime_for_test(Default::default());
    let gateway = RestorationGateway::with_test_rules(&["chinese_phone_number"], 64 * 1024 * 1024);
    let mut headers = http::HeaderMap::new();
    headers.insert("session-id", "synthetic-business-session".parse()?);
    for field in ["encrypted_content", "encrypted_reasoning"] {
        for escaped in [false, true] {
            let arguments = serde_json::to_string(&json!({
                "fixture": [{field: "Synthetic business value"}],
                "source": "Synthetic source\nrredact-custom-"
            }))?;
            let arguments = if escaped {
                arguments.replace("encrypted_", "\\u0065ncrypted_")
            } else {
                arguments
            };
            let mut body = json!({"input":[
                {"type":"function_call", "call_id":"synthetic-business-call", "name":"synthetic-tool", "arguments":arguments},
                {"type":"function_call_output", "call_id":"synthetic-business-call", "output":"Synthetic result"},
                {"role":"user", "content":"Synthetic continue"}
            ]});
            let original = body.clone();
            let request = match admit_http_request(
                &gateway,
                &AppType::Codex,
                true,
                false,
                false,
                &mut body,
                &headers,
            )? {
                RestorationAdmission::Admitted(request) => request,
                _ => return Err("business_field_admission_rejected".into()),
            };
            let transformed = std::cell::Cell::new(false);
            let attempt = request
                .prepare_attempt(
                    "synthetic-provider",
                    "openai_responses",
                    "https://synthetic.invalid/v1/responses",
                    Some("synthetic-principal"),
                    &mut body,
                    |candidate| {
                        transformed.set(true);
                        assert!(candidate == &original, "business_arguments_changed");
                        Ok(candidate.clone())
                    },
                )
                .map_err(|_| "business_field_continuation_rejected")?;
            assert!(transformed.get(), "business_field_not_prepared");
            assert!(
                !attempt.has_active_tokens(),
                "business_field_granted_authority"
            );
            assert!(body == original, "business_source_changed");
        }
    }
    Ok(())
}

#[test]
#[serial_test::serial]
fn placeholder_passthrough_request_tool_history_and_final_validation() -> TestResult {
    let _reset = Reset((*runtime::get_security_runtime()).clone());
    runtime::set_security_runtime_for_test(Default::default());
    for text in literals() {
        let gateway =
            RestorationGateway::with_test_rules(&["chinese_phone_number"], 64 * 1024 * 1024);
        let mut body = json!({"input":[
            {"role":"user","content":"Synthetic task"},
            {"type":"function_call","call_id":"synthetic-call","name":"synthetic-tool","arguments":"{}"},
            {"type":"function_call_output","call_id":"synthetic-call","output":text},
            {"role":"user","content":"Synthetic continue"}
        ]});
        let original = body.clone();
        let request = match admit_http_request(
            &gateway,
            &AppType::Codex,
            true,
            false,
            false,
            &mut body,
            &Default::default(),
        )? {
            RestorationAdmission::Admitted(request) => request,
            _ => return Err("literal_admission_rejected".into()),
        };
        let attempt = request
            .prepare_attempt(
                "synthetic-provider",
                "openai_responses",
                "https://synthetic.invalid/v1/responses",
                Some("synthetic-principal"),
                &mut body,
                |candidate| Ok(candidate.clone()),
            )
            .map_err(|_| "literal_final_preparation_rejected")?;
        assert!(body == original, "literal_history_changed");
        assert!(
            !attempt.has_active_tokens(),
            "literal_history_granted_authority"
        );
    }
    Ok(())
}

#[test]
fn placeholder_passthrough_request_unleased_literals_are_not_dependencies() -> TestResult {
    for text in literals() {
        validate_unleased_enriched_body(&json!({"input":text}))
            .map_err(|_| "literal_unleased_rejected")?;
    }
    Ok(())
}

#[test]
fn placeholder_passthrough_request_mixed_pii_still_redacts() -> TestResult {
    let store = DlpTokenStore::new();
    let mut lease = store.admit("codex", Some("synthetic-literal"), 1)?;
    let text = literals().remove(0);
    let phone = ["139", "00000000"].concat();
    let body = json!({"input":format!("{text} synthetic contact {phone}")});
    let prepared = prepare_codex_request_with_budget(
        &mut lease,
        &body,
        &Default::default(),
        &["chinese_phone_number"],
        &[],
        PayloadBudget::default(),
        |_, _, proof| {
            assert!(proof.len() == 1, "pii_proof_missing");
            Ok(())
        },
    )
    .map_err(|_| "mixed_literal_rejected")?;
    let output = prepared.body.as_value()["input"]
        .as_str()
        .ok_or("mixed_output_missing")?;
    assert!(
        output.contains(&text) && !output.contains(&phone),
        "mixed_dlp_contract_changed"
    );
    Ok(())
}

#[test]
fn placeholder_passthrough_request_foreign_reference_does_not_resolve() -> TestResult {
    let store = DlpTokenStore::new();
    let mut issuer = store.admit("codex", Some("synthetic-issuer"), 1)?;
    let references = issuer.prepare(
        &[TokenFragment {
            raw: "synthetic-private",
            whole_span_categories: &[TokenCategory::Custom],
        }],
        &[],
        |references| Ok(references.to_vec()),
    )?;
    let mut receiver = store.admit("codex", Some("synthetic-receiver"), 1)?;
    let body = json!({"input":references[0]});
    let prepared = prepare_codex_request_with_budget(
        &mut receiver,
        &body,
        &Default::default(),
        &[],
        &[],
        PayloadBudget::default(),
        |_, _, proof| {
            assert!(proof.is_empty(), "foreign_reference_granted_authority");
            Ok(())
        },
    )
    .map_err(|_| "foreign_literal_rejected")?;
    assert!(prepared.body.as_value() == &body, "foreign_literal_changed");
    assert!(
        matches!(
            receiver.resolve(&references[0]),
            Err(TokenError::ContextUnavailable)
        ),
        "foreign_secret_resolved"
    );
    Ok(())
}

#[test]
fn placeholder_passthrough_request_block_budget_and_literal_collision_stay_closed() -> TestResult {
    use super::{
        request_plan::{EffectiveAction, PlanError, SpanMatch},
        runtime::CompiledCustomRule,
        types::{AuditAction, RiskLevel},
    };
    let store = DlpTokenStore::new();
    let mut lease = store.admit("codex", None, 1)?;
    let runtime = SecurityRuntimeConfig {
        custom_rules: vec![CompiledCustomRule {
            id: "synthetic-block".into(),
            name: "Synthetic".into(),
            rule_type: "regex".into(),
            pattern: "synthetic-stop".into(),
            regex: Some(regex::Regex::new("synthetic-stop")?),
            risk_level: RiskLevel::Medium,
            action: AuditAction::Block,
            enabled: true,
        }],
        ..Default::default()
    };
    let body = json!({"input":format!("{} synthetic-stop",literals()[0])});
    let result = prepare_codex_request_with_budget(
        &mut lease,
        &body,
        &runtime,
        &[],
        &[],
        PayloadBudget::default(),
        |_, _, _| Ok(()),
    );
    assert!(
        matches!(result, Err(PlanError::Blocked)),
        "literal_bypassed_block"
    );
    let budget = PayloadBudget::with_limit(1);
    assert!(
        prepare_codex_request_with_budget(
            &mut lease,
            &body,
            &Default::default(),
            &[],
            &[],
            budget.clone(),
            |_, _, _| Ok(())
        )
        .is_err(),
        "request_budget_bypassed"
    );
    assert!(budget.used() == 0, "request_budget_retained");
    let key = [7; 32];
    let collision_store = DlpTokenStore::with_test_key(key);
    let mut collision_lease = collision_store.admit("codex", None, 1)?;
    let raw = "synthetic-collision-plain";
    let literal = super::tokenization::make_token(&key, raw, TokenCategory::Custom)?;
    let matches = [SpanMatch {
        range: 0..raw.len(),
        action: EffectiveAction::Restore,
        category: TokenCategory::Custom,
    }];
    let fields = [
        PlanField {
            text: raw,
            writable: true,
            matches: &matches,
        },
        PlanField {
            text: &literal,
            writable: true,
            matches: &[],
        },
    ];
    let result = prepare_fields_with_budget(
        &mut collision_lease,
        &fields,
        &[],
        PayloadBudget::default(),
        |_, _| Ok(()),
    );
    assert!(
        matches!(result, Err(PlanError::Token(TokenError::TokenCollision))),
        "literal_collision_accepted"
    );
    assert!(
        collision_store.stats()?.entries == 0,
        "collision_partially_committed"
    );
    Ok(())
}

#[test]
#[serial_test::serial]
fn placeholder_passthrough_request_previous_response_degrades_without_completion() -> TestResult {
    let _reset = Reset((*runtime::get_security_runtime()).clone());
    runtime::set_security_runtime_for_test(Default::default());
    let gateway = RestorationGateway::with_test_rules(&["chinese_phone_number"], 64 * 1024 * 1024);
    let mut body =
        json!({"previous_response_id":"synthetic-missing-response","input":literals()[0]});
    let original = body.clone();
    let request = match admit_http_request(
        &gateway,
        &AppType::Codex,
        true,
        false,
        false,
        &mut body,
        &Default::default(),
    )? {
        RestorationAdmission::Admitted(request) => request,
        _ => return Err("missing_completion_admission_failed".into()),
    };
    let attempt = request
        .prepare_attempt(
            "synthetic-provider",
            "openai_responses",
            "https://synthetic.invalid/v1/responses",
            Some("synthetic-principal"),
            &mut body,
            |candidate| {
                assert_eq!(candidate, &original, "missing_completion_changed_literal");
                Ok(candidate.clone())
            },
        )
        .map_err(|_| "missing_completion_blocked_request")?;
    assert!(
        !attempt.has_active_tokens(),
        "missing_completion_granted_authority"
    );
    assert_eq!(body, original, "missing_completion_changed_source");
    Ok(())
}

#[test]
#[serial_test::serial]
fn placeholder_passthrough_request_disabled_runtime_keeps_unknown_complete_literal() -> TestResult {
    let _reset = Reset((*runtime::get_security_runtime()).clone());
    runtime::set_security_runtime_for_test(SecurityRuntimeConfig {
        enabled: false,
        ..Default::default()
    });
    let gateway = RestorationGateway::with_test_rules(&["chinese_phone_number"], 64 * 1024 * 1024);
    let mut body = json!({"input":literals()[2]});
    let original = body.clone();
    assert!(
        matches!(
            admit_http_request(
                &gateway,
                &AppType::Codex,
                true,
                false,
                false,
                &mut body,
                &Default::default()
            )?,
            RestorationAdmission::Passthrough
        ),
        "disabled_literal_rejected"
    );
    assert!(body == original, "disabled_literal_changed");
    Ok(())
}

#[test]
fn placeholder_passthrough_request_completed_matching_keeps_binding_boundary() -> TestResult {
    use super::{
        completed_context::CompletionWitness,
        response_restoration::restore_and_publish_json_response,
    };
    let store = DlpTokenStore::new();
    let mut issuer = store.admit("codex", Some("synthetic-binding"), 1)?;
    let reference = issuer.prepare(
        &[TokenFragment {
            raw: "synthetic-binding-secret",
            whole_span_categories: &[TokenCategory::Custom],
        }],
        &[],
        |refs| Ok(refs[0].clone()),
    )?;
    let binding = issuer.bind_actual_attempt(
        "synthetic-provider",
        "openai_responses",
        "https://synthetic.invalid/v1/responses",
        Some("synthetic-principal"),
    )?;
    let raw = json!({"id":"synthetic-binding-response","object":"response","status":"completed","output":[{"id":"synthetic-message","type":"message","role":"assistant","status":"completed","content":[{"type":"output_text","text":reference}]}]});
    let witness = CompletionWitness::from_verified_completion(true)?;
    restore_and_publish_json_response(
        &raw,
        false,
        &issuer,
        &binding,
        &witness,
        &PayloadBudget::default(),
    )
    .map_err(|_| "synthetic_publish_failed")?;
    for (session, provider, expected) in [
        ("synthetic-binding", "synthetic-provider", true),
        ("synthetic-binding", "synthetic-other-provider", false),
        ("synthetic-other-session", "synthetic-provider", false),
    ] {
        let mut receiver = store.admit("codex", Some(session), 1)?;
        let actual = receiver.bind_actual_attempt(
            provider,
            "openai_responses",
            "https://synthetic.invalid/v1/responses",
            Some("synthetic-principal"),
        )?;
        let unknown = &literals()[2];
        receiver.resume_matching_completed_tokens(&actual, &[unknown, &reference])?;
        assert!(
            receiver.resolve(&reference).is_ok() == expected,
            "completed_binding_boundary_changed"
        );
        assert!(
            receiver.resolve(unknown).is_err(),
            "unknown_literal_authorized"
        );
    }
    Ok(())
}

#[test]
#[serial_test::serial]
fn placeholder_passthrough_request_readable_mixed_history_degrades_without_tool_witness(
) -> TestResult {
    use super::{
        completed_context::CompletionWitness,
        response_restoration::restore_and_publish_json_response,
    };
    let _reset = Reset((*runtime::get_security_runtime()).clone());
    runtime::set_security_runtime_for_test(Default::default());
    let revision = runtime::admit_security_runtime()?.restoration_policy_revision;
    let store = DlpTokenStore::new();
    let mut issuer =
        store.admit_published_policy("codex", Some("synthetic-mixed-history"), revision)?;
    let reference = issuer.prepare(
        &[TokenFragment {
            raw: "synthetic-mixed-private",
            whole_span_categories: &[TokenCategory::Custom],
        }],
        &[],
        |refs| Ok(refs[0].clone()),
    )?;
    let binding = issuer.bind_actual_attempt(
        "synthetic-provider",
        "openai_responses",
        "https://synthetic.invalid/v1/responses",
        Some("synthetic-principal"),
    )?;
    let response = json!({"id":"synthetic-mixed-response","object":"response","status":"completed","output":[{"id":"synthetic-message","type":"message","role":"assistant","status":"completed","content":[{"type":"output_text","text":reference}]}]});
    let witness = CompletionWitness::from_verified_completion(true)?;
    restore_and_publish_json_response(
        &response,
        false,
        &issuer,
        &binding,
        &witness,
        &PayloadBudget::default(),
    )
    .map_err(|_| "mixed_history_publish_failed")?;
    let gateway = RestorationGateway::with_test_store_and_rules(
        store,
        &["chinese_phone_number"],
        PayloadBudget::default(),
    );
    let mut headers = http::HeaderMap::new();
    headers.insert("session-id", "synthetic-mixed-history".parse()?);
    for includes_reference in [false, true] {
        let literal = &literals()[2];
        let text = if includes_reference {
            format!("{literal} {reference}")
        } else {
            literal.clone()
        };
        let mut body = json!({"input":[
            {"role":"user","content":"Synthetic task"},
            {"type":"function_call","call_id":"synthetic-unwitnessed-call","name":"synthetic-tool","arguments":"{}"},
            {"type":"function_call_output","call_id":"synthetic-unwitnessed-call","output":text}
        ]});
        assert!(
            super::request_preparation::is_fresh_readable_history(
                &body,
                &PayloadBudget::default()
            )?,
            "mixed_history_not_readable"
        );
        let original = body.clone();
        let request = match admit_http_request(
            &gateway,
            &AppType::Codex,
            true,
            false,
            false,
            &mut body,
            &headers,
        )? {
            RestorationAdmission::Admitted(request) => request,
            _ => return Err("mixed_history_admission_failed".into()),
        };
        let transformed = std::cell::Cell::new(false);
        let attempt = request
            .prepare_attempt(
                "synthetic-provider",
                "openai_responses",
                "https://synthetic.invalid/v1/responses",
                Some("synthetic-principal"),
                &mut body,
                |candidate| {
                    transformed.set(true);
                    assert_eq!(
                        candidate, &original,
                        "unwitnessed_reference_changed; includes_reference={includes_reference}"
                    );
                    Ok(candidate.clone())
                },
            )
            .map_err(|_| "unwitnessed_history_blocked")?;
        assert!(transformed.get(), "unwitnessed_history_not_prepared");
        assert_eq!(
            attempt.has_active_tokens(),
            includes_reference,
            "known_completion_authority_changed; includes_reference={includes_reference}"
        );
        assert_eq!(
            attempt.lease.resolve(&reference).is_ok(),
            includes_reference,
            "known_reference_completion_boundary_changed"
        );
        assert!(
            attempt.lease.resolve(literal).is_err(),
            "unknown_literal_granted_authority"
        );
        assert_eq!(body, original, "mixed_history_bytes_changed");
    }
    Ok(())
}

fn message_projection_fixture() -> Result<
    (
        DlpTokenStore,
        super::tokenization::DlpRequestLease,
        super::completed_context::ActualAttemptBinding,
        Value,
        String,
    ),
    Box<dyn std::error::Error>,
> {
    let revision = runtime::admit_security_runtime()?.restoration_policy_revision;
    let store = DlpTokenStore::new();
    let mut lease =
        store.admit_published_policy("codex", Some("synthetic-message-projection"), revision)?;
    let reference = lease.prepare(
        &[TokenFragment {
            raw: "synthetic-projection-private",
            whole_span_categories: &[TokenCategory::Custom],
        }],
        &[],
        |refs| Ok(refs[0].clone()),
    )?;
    let binding = lease.bind_actual_attempt(
        "synthetic-provider",
        "openai_responses",
        "https://synthetic.invalid/v1/responses",
        Some("synthetic-principal"),
    )?;
    let response = json!({"id":"synthetic-projection-response", "object":"response", "status":"completed", "output":[{"id":"synthetic-projection-message", "type":"message", "role":"assistant", "status":"completed", "content":[{"type":"output_text", "text":"Synthetic delivered message"}]}]});
    let restored = super::response_restoration::restore_and_publish_json_response(
        &response,
        false,
        &lease,
        &binding,
        &super::completed_context::CompletionWitness::from_verified_completion(true)?,
        &PayloadBudget::default(),
    )
    .map_err(|_| "projection_publish_failed")?;
    let pending = lease
        .capture_delivered_history(&binding, restored.as_value())
        .ok_or("projection_capture_failed")?;
    assert!(
        lease.publish_delivered_history(&binding, pending),
        "projection_delivery_failed"
    );
    Ok((
        store,
        lease,
        binding,
        response["output"][0].clone(),
        reference,
    ))
}

#[test]
#[serial_test::serial]
fn message_projection_ordinary_tool_history_stays_independent() -> TestResult {
    let _reset = Reset((*runtime::get_security_runtime()).clone());
    runtime::set_security_runtime_for_test(Default::default());
    let (_, lease, _, message, _) = message_projection_fixture()?;
    let body = json!({"input":[message,
        {"type":"function_call", "call_id":"synthetic-readable-call", "name":"synthetic-tool", "arguments":"{}"},
        {"type":"function_call_output", "call_id":"synthetic-readable-call", "output":literals()[2]}
    ]});
    let budget = PayloadBudget::default();
    assert!(
        super::request_preparation::is_fresh_readable_history(&body, &budget)?,
        "readable_control_failed"
    );
    let projection = lease
        .history_admission_candidate_projection(&body)
        .ok_or("message_projection_missing")?;
    assert!(
        super::request_preparation::is_fresh_readable_history_with_history(
            &body,
            &budget,
            Some(&projection)
        )?,
        "message_projection_upgraded_tool_dependency"
    );
    Ok(())
}

#[test]
#[serial_test::serial]
fn message_projection_http_tool_history_retains_dependency_boundaries() -> TestResult {
    let _reset = Reset((*runtime::get_security_runtime()).clone());
    runtime::set_security_runtime_for_test(Default::default());
    let (store, lease, _, message, reference) = message_projection_fixture()?;
    let gateway = RestorationGateway::with_test_store_and_rules(
        store,
        &["chinese_phone_number"],
        PayloadBudget::default(),
    );
    let mut headers = http::HeaderMap::new();
    headers.insert("session-id", "synthetic-message-projection".parse()?);
    for case in 0..5 {
        let mut body = json!({"input":[message,
            {"type":"function_call", "call_id":"synthetic-readable-call", "name":"synthetic-tool", "arguments":"{}"},
            {"type":"function_call_output", "call_id":"synthetic-readable-call", "output":literals()[2]}
        ]});
        match case {
            1 => body["input"][2]["output"] = json!(reference),
            2 => body["previous_response_id"] = json!("synthetic-missing-completion"),
            3 => body["input"][1]["encrypted_reasoning"] = json!("synthetic-unknown-opaque"),
            4 => {
                body["input"].as_array_mut().ok_or("missing_input")?.pop();
            }
            _ => {}
        }
        let projection = lease
            .history_admission_candidate_projection(&body)
            .ok_or("message_projection_missing")?;
        drop(projection);
        let original = body.clone();
        let admission = admit_http_request(
            &gateway,
            &AppType::Codex,
            true,
            false,
            false,
            &mut body,
            &headers,
        );
        let request = match admission {
            Ok(RestorationAdmission::Admitted(request)) => request,
            _ if case != 0 => continue,
            _ => return Err("message_projection_http_admission_failed".into()),
        };
        let attempt = request
            .prepare_attempt(
                "synthetic-provider",
                "openai_responses",
                "https://synthetic.invalid/v1/responses",
                Some("synthetic-principal"),
                &mut body,
                |candidate| {
                    assert_eq!(
                        candidate, &original,
                        "projection_changed_unproven_history; case={case}"
                    );
                    Ok(candidate.clone())
                },
            )
            .map_err(|_| "message_projection_missing_context_blocked")?;
        assert_eq!(
            attempt.has_active_tokens(),
            case == 1,
            "projection_completion_authority_changed; case={case}"
        );
        assert_eq!(
            attempt.lease.resolve(&reference).is_ok(),
            case == 1,
            "projection_reference_boundary_changed; case={case}"
        );
        assert!(
            attempt.lease.resolve(&literals()[2]).is_err(),
            "projection_unknown_literal_granted_authority; case={case}"
        );
        assert_eq!(body, original, "projection_changed_source");
    }
    Ok(())
}

#[test]
#[serial_test::serial]
fn message_projection_recorded_call_identity_keeps_strict_resume() -> TestResult {
    let _reset = Reset((*runtime::get_security_runtime()).clone());
    runtime::set_security_runtime_for_test(Default::default());
    let (_, lease, binding, message, _) = message_projection_fixture()?;
    let call = json!({"type":"function_call", "call_id":"synthetic-recorded-call", "name":"synthetic-tool", "arguments":"{}", "reasoning_content":"Synthetic reasoning"});
    let response = json!({"output":[message, call]});
    let pending = lease
        .capture_delivered_history(&binding, &response)
        .ok_or("recorded_capture_failed")?;
    assert!(
        lease.publish_delivered_history(&binding, pending),
        "recorded_delivery_failed"
    );
    for change in 0..4 {
        let mut body = json!({"input":[message, call,
            {"type":"function_call_output", "call_id":"synthetic-recorded-call", "output":"Synthetic result"}
        ]});
        match change {
            1 => body["input"][1]["arguments"] = json!("{\"changed\":true}"),
            2 => body["input"][1]["name"] = json!("synthetic-other-tool"),
            3 => body["input"][1]["reasoning_content"] = json!("Synthetic changed reasoning"),
            _ => {}
        }
        let projection = lease
            .history_admission_candidate_projection(&body)
            .ok_or("recorded_projection_missing")?;
        assert!(
            !super::request_preparation::is_fresh_readable_history_with_history(
                &body,
                &PayloadBudget::default(),
                Some(&projection)
            )?,
            "recorded_identity_lost_completion_dependency"
        );
    }
    Ok(())
}

#[test]
#[serial_test::serial]
fn message_projection_recorded_identity_without_matching_sibling_stays_strict() -> TestResult {
    let _reset = Reset((*runtime::get_security_runtime()).clone());
    runtime::set_security_runtime_for_test(Default::default());
    let (_, lease, binding, _, _) = message_projection_fixture()?;
    let call = json!({"id":"synthetic-recorded-item", "type":"function_call", "call_id":"synthetic-recorded-call", "name":"synthetic-tool", "arguments":"{}", "reasoning_content":"Synthetic reasoning"});
    let pending = lease
        .capture_delivered_history(&binding, &json!({"output":[call]}))
        .ok_or("recorded_capture_failed")?;
    assert!(
        lease.publish_delivered_history(&binding, pending),
        "recorded_delivery_failed"
    );
    for change in 0..7 {
        let mut body = json!({"input":[call, {"type":"function_call_output", "call_id":"synthetic-recorded-call", "output":"Synthetic result"}]});
        match change {
            1 => body["input"][0]["arguments"] = json!("{\"changed\":true}"),
            2 => body["input"][0]["name"] = json!("synthetic-other-tool"),
            3 => body["input"][0]["reasoning_content"] = json!("Synthetic changed reasoning"),
            4 => {
                body["input"][0]["call_id"] = json!("synthetic-other-call");
                body["input"][1]["call_id"] = json!("synthetic-other-call");
            }
            5 => body["input"][0]["id"] = json!("synthetic-other-item"),
            6 => {
                body["input"][0]["type"] = json!("custom_tool_call");
                body["input"][0]["input"] = json!("Synthetic custom input");
                body["input"][0]
                    .as_object_mut()
                    .ok_or("missing_call")?
                    .remove("arguments");
                body["input"][1]["type"] = json!("custom_tool_call_output");
            }
            _ => {}
        }
        let projection = lease.history_admission_candidate_projection(&body);
        assert!(
            projection.as_ref().is_some_and(|projection| projection
                .source_for_responses_leaf(
                    0,
                    super::history_provenance::DlpHistoryLeafField::CallReasoning,
                    0,
                )
                .is_some()
                == (change == 0)),
            "identity_candidate_granted_changed_leaf_proof"
        );
        let foreign = lease.bind_actual_attempt(
            "synthetic-other-provider",
            "openai_responses",
            "https://synthetic.invalid/v1/responses",
            Some("synthetic-principal"),
        )?;
        assert!(
            lease.history_projection(&foreign, &body).is_none(),
            "identity_candidate_crossed_binding"
        );
        assert!(
            !super::request_preparation::is_fresh_readable_history_with_history(
                &body,
                &PayloadBudget::default(),
                projection.as_ref()
            )?,
            "recorded_identity_without_sibling_lost_dependency"
        );
    }
    Ok(())
}

#[test]
#[serial_test::serial]
fn message_projection_recorded_call_http_degrades_unavailable_binding_and_completion() -> TestResult
{
    let _reset = Reset((*runtime::get_security_runtime()).clone());
    runtime::set_security_runtime_for_test(Default::default());
    let (store, lease, binding, message, _) = message_projection_fixture()?;
    let call = json!({"id":"synthetic-bound-item", "type":"function_call", "call_id":"synthetic-bound-call", "name":"synthetic-tool", "arguments":"{}"});
    let response = json!({"id":"synthetic-bound-response", "object":"response", "status":"completed", "output":[message, call]});
    let restored = super::response_restoration::restore_and_publish_json_response(
        &response,
        false,
        &lease,
        &binding,
        &super::completed_context::CompletionWitness::from_verified_completion(true)?,
        &PayloadBudget::default(),
    )
    .map_err(|_| "bound_publish_failed")?;
    let pending = lease
        .capture_delivered_history(&binding, restored.as_value())
        .ok_or("bound_capture_failed")?;
    assert!(
        lease.publish_delivered_history(&binding, pending),
        "bound_delivery_failed"
    );
    let gateway = RestorationGateway::with_test_store_and_rules(
        store,
        &["chinese_phone_number"],
        PayloadBudget::default(),
    );
    let mut headers = http::HeaderMap::new();
    headers.insert("session-id", "synthetic-message-projection".parse()?);
    for case in 0..8 {
        let variant = case % 4;
        let mut body = json!({"input":[message, call,
            {"type":"function_call_output", "call_id":"synthetic-bound-call", "output":"Synthetic result"}
        ]});
        if variant == 2 {
            body["input"][1]["arguments"] = json!("{\"changed\":true}");
        }
        if variant == 3 {
            body["input"][1]["call_id"] = json!("synthetic-undelivered-call");
            body["input"][2]["call_id"] = json!("synthetic-undelivered-call");
        }
        if case >= 4 {
            body["input"]
                .as_array_mut()
                .ok_or("missing_bound_input")?
                .remove(0);
        }
        let original = body.clone();
        let request = match admit_http_request(
            &gateway,
            &AppType::Codex,
            true,
            false,
            false,
            &mut body,
            &headers,
        )? {
            RestorationAdmission::Admitted(request) => request,
            _ => return Err("bound_admission_failed".into()),
        };
        let transformed = std::cell::Cell::new(false);
        let attempt = request
            .prepare_attempt(
                if variant == 1 {
                    "synthetic-other-provider"
                } else {
                    "synthetic-provider"
                },
                "openai_responses",
                "https://synthetic.invalid/v1/responses",
                Some("synthetic-principal"),
                &mut body,
                |candidate| {
                    transformed.set(true);
                    assert_eq!(
                        candidate, &original,
                        "unavailable_binding_changed_history; case={case}"
                    );
                    Ok(candidate.clone())
                },
            )
            .map_err(|_| "unavailable_bound_history_blocked")?;
        assert!(transformed.get(), "bound_history_not_prepared; case={case}");
        assert_eq!(
            attempt.has_active_tokens(),
            variant == 0,
            "bound_completion_authority_changed; case={case}"
        );
        assert!(
            attempt.lease.resolve(&literals()[2]).is_err(),
            "bound_unknown_literal_granted_authority; case={case}"
        );
        assert_eq!(body, original, "bound_source_changed");
    }
    Ok(())
}

#[test]
#[serial_test::serial]
fn mixed_readable_continuation_http_degrades_unavailable_per_call_completion() -> TestResult {
    let _reset = Reset((*runtime::get_security_runtime()).clone());
    runtime::set_security_runtime_for_test(Default::default());
    let (store, lease, binding, message, reference) = message_projection_fixture()?;
    let call = json!({"id":"synthetic-mixed-item", "type":"function_call", "call_id":"synthetic-mixed-call", "name":"synthetic-tool", "arguments":"{}", "reasoning_content":"Synthetic reasoning"});
    let response = json!({"id":"synthetic-mixed-completed", "object":"response", "status":"completed", "output":[message, call]});
    let restored = super::response_restoration::restore_and_publish_json_response(
        &response,
        false,
        &lease,
        &binding,
        &super::completed_context::CompletionWitness::from_verified_completion(true)?,
        &PayloadBudget::default(),
    )
    .map_err(|_| "mixed_completion_publish_failed")?;
    let pending = lease
        .capture_delivered_history(&binding, restored.as_value())
        .ok_or("mixed_capture_failed")?;
    assert!(
        lease.publish_delivered_history(&binding, pending),
        "mixed_delivery_failed"
    );
    let gateway = RestorationGateway::with_test_store_and_rules(
        store,
        &["chinese_phone_number"],
        PayloadBudget::default(),
    );
    let mut headers = http::HeaderMap::new();
    headers.insert("session-id", "synthetic-message-projection".parse()?);
    // Run strict controls before the positive case so RED also verifies the controls.
    let business_arguments = serde_json::to_string(&json!({
        "fixture": {"encrypted_content": "Synthetic value", "encrypted_reasoning": "Synthetic value"},
        "source": "Synthetic source\nrredact-custom-"
    }))?;
    for case in [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 14, 15, 16, 0, 13] {
        let must_admit = matches!(case, 0 | 5 | 13);
        let mut body = json!({"input":[
            {"type":"function_call", "call_id":"synthetic-readable-prior", "name":"synthetic-prior-tool", "arguments":"{}"},
            {"type":"function_call_output", "call_id":"synthetic-readable-prior", "output":"Synthetic prior result"},
            call,
            {"type":"function_call_output", "call_id":"synthetic-mixed-call", "output":"Synthetic current result"}
        ]});
        if matches!(case, 13..=16) {
            body["input"][0]["arguments"] = json!(business_arguments);
        }
        match case {
            1 | 14 => body["input"][2]["arguments"] = json!("{\"changed\":true}"),
            15 => body["input"][0]["encrypted_reasoning"] = json!("synthetic-opaque"),
            16 => body["input"][1]["output"] = json!(reference),
            2 => body["input"][2]["name"] = json!("synthetic-changed-tool"),
            3 => body["input"][2]["reasoning_content"] = json!("Synthetic changed reasoning"),
            4 => body["input"][1]["output"] = json!(reference),
            5 => body["previous_response_id"] = json!("synthetic-mixed-completed"),
            6 => body["input"][2]["encrypted_reasoning"] = json!("synthetic-opaque"),
            7 => {
                body["input"]
                    .as_array_mut()
                    .ok_or("missing_mixed_input")?
                    .pop();
            }
            8 => body["input"][0]["arguments"] = json!("{"),
            11 => {
                body["input"][2]["call_id"] = json!("synthetic-changed-call");
                body["input"][3]["call_id"] = json!("synthetic-changed-call");
            }
            12 => {
                body["input"][2]["type"] = json!("custom_tool_call");
                body["input"][2]["input"] = json!("Synthetic custom input");
                body["input"][2]
                    .as_object_mut()
                    .ok_or("missing_mixed_call")?
                    .remove("arguments");
                body["input"][3]["type"] = json!("custom_tool_call_output");
            }
            _ => {}
        }
        let original = body.clone();
        let admission = admit_http_request(
            &gateway,
            &AppType::Codex,
            true,
            false,
            false,
            &mut body,
            &headers,
        );
        let request = match admission {
            Ok(RestorationAdmission::Admitted(request)) => request,
            _ if !must_admit => continue,
            _ => return Err("mixed_readable_admission_rejected".into()),
        };
        if case == 10 {
            body["input"][1]["output"] = json!("Synthetic enriched result");
        }
        let attempt_input = body.clone();
        let transformed = std::cell::Cell::new(false);
        let attempt = request
            .prepare_attempt(
                if case == 9 {
                    "synthetic-other-provider"
                } else {
                    "synthetic-provider"
                },
                "openai_responses",
                "https://synthetic.invalid/v1/responses",
                Some("synthetic-principal"),
                &mut body,
                |candidate| {
                    transformed.set(true);
                    assert_eq!(
                        candidate, &attempt_input,
                        "mixed_unproven_history_changed; case={case}"
                    );
                    Ok(candidate.clone())
                },
            )
            .map_err(|_| "mixed_readable_missing_context_blocked")?;
        assert!(transformed.get(), "mixed_readable_history_not_prepared");
        let expects_completion_authority = matches!(case, 0 | 4 | 5 | 13 | 16);
        assert_eq!(
            attempt.has_active_tokens(),
            expects_completion_authority,
            "mixed_completion_authority_changed; case={case}"
        );
        assert_eq!(
            attempt.lease.resolve(&reference).is_ok(),
            expects_completion_authority,
            "mixed_reference_boundary_changed; case={case}"
        );
        assert!(
            attempt.lease.resolve(&literals()[2]).is_err(),
            "mixed_unknown_literal_granted_authority; case={case}"
        );
        assert_eq!(body, attempt_input, "mixed_readable_source_changed");
        if case != 10 {
            assert_eq!(body, original, "mixed_readable_admission_changed");
        }
    }
    Ok(())
}

#[test]
#[serial_test::serial]
fn mixed_readable_continuation_cursor_degrades_unavailable_per_call_completion() -> TestResult {
    let _reset = Reset((*runtime::get_security_runtime()).clone());
    runtime::set_security_runtime_for_test(Default::default());
    let (store, lease, binding, message, reference) = message_projection_fixture()?;
    let first = json!({"id":"synthetic-cursor-first-item", "type":"function_call", "call_id":"synthetic-cursor-first-call", "name":"synthetic-first-tool", "arguments":"{}"});
    let second = json!({"id":"synthetic-cursor-second-item", "type":"function_call", "call_id":"synthetic-cursor-second-call", "name":"synthetic-second-tool", "arguments":"{}"});
    let response = json!({"id":"synthetic-cursor-completed", "object":"response", "status":"completed", "output":[message, first, second]});
    let restored = super::response_restoration::restore_and_publish_json_response(
        &response,
        false,
        &lease,
        &binding,
        &super::completed_context::CompletionWitness::from_verified_completion(true)?,
        &PayloadBudget::default(),
    )
    .map_err(|_| "cursor_completion_publish_failed")?;
    let pending = lease
        .capture_delivered_history(&binding, restored.as_value())
        .ok_or("cursor_capture_failed")?;
    assert!(
        lease.publish_delivered_history(&binding, pending),
        "cursor_delivery_failed"
    );
    let gateway = RestorationGateway::with_test_store_and_rules(
        store,
        &["chinese_phone_number"],
        PayloadBudget::default(),
    );
    let mut headers = http::HeaderMap::new();
    headers.insert("session-id", "synthetic-message-projection".parse()?);
    for case in 0..9 {
        let mut body = json!({"input":[
            {"type":"function_call", "call_id":"synthetic-cursor-prior", "name":"synthetic-prior-tool", "arguments":"{}"},
            {"type":"function_call_output", "call_id":"synthetic-cursor-prior", "output":"Synthetic prior result"},
            response["output"][1].clone(),
            {"type":"function_call_output", "call_id":"synthetic-cursor-first-call", "output":"Synthetic first result"},
            response["output"][2].clone(),
            {"type":"function_call_output", "call_id":"synthetic-cursor-second-call", "output":"Synthetic second result"},
            {"role":"user", "content":"Synthetic continue"}
        ]});
        if case >= 7 {
            // A live message projection remains, but per-call selection produces no calls.
            body["input"] = json!([
                response["output"][0].clone(),
                {"type":"function_call", "call_id":"synthetic-cursor-prior", "name":"synthetic-prior-tool", "arguments":"{}"},
                {"type":"function_call_output", "call_id":"synthetic-cursor-prior", "output":"Synthetic prior result"},
                {"role":"user", "content":"Synthetic continue"}
            ]);
        }
        if case != 0 && case != 3 && case != 4 {
            body["previous_response_id"] = json!("synthetic-cursor-completed");
        }
        match case {
            2 | 7 => body["previous_response_id"] = json!("synthetic-forged-cursor"),
            3 => body["input"][1]["output"] = json!(reference),
            4 => body["input"][0]["encrypted_reasoning"] = json!("synthetic-unknown-opaque"),
            5 => body["input"][2]["arguments"] = json!("{\"changed\":true}"),
            _ => {}
        }
        let original = body.clone();
        let request = match admit_http_request(
            &gateway,
            &AppType::Codex,
            true,
            false,
            false,
            &mut body,
            &headers,
        )? {
            RestorationAdmission::Admitted(request) => request,
            _ => return Err("cursor_mixed_admission_failed".into()),
        };
        let transformed = std::cell::Cell::new(false);
        let attempt = request
            .prepare_attempt(
                if matches!(case, 6 | 8) {
                    "synthetic-other-provider"
                } else {
                    "synthetic-provider"
                },
                "openai_responses",
                "https://synthetic.invalid/v1/responses",
                Some("synthetic-principal"),
                &mut body,
                |candidate| {
                    transformed.set(true);
                    assert_eq!(
                        candidate, &original,
                        "cursor_unproven_history_changed; case={case}"
                    );
                    Ok(candidate.clone())
                },
            )
            .map_err(|_| "cursor_mixed_missing_context_blocked")?;
        assert!(transformed.get(), "cursor_mixed_history_not_prepared");
        let expects_completion_authority = matches!(case, 0 | 1 | 3);
        assert_eq!(
            attempt.has_active_tokens(),
            expects_completion_authority,
            "cursor_completion_authority_changed; case={case}"
        );
        assert_eq!(
            attempt.lease.resolve(&reference).is_ok(),
            expects_completion_authority,
            "cursor_reference_boundary_changed; case={case}"
        );
        assert!(
            attempt.lease.resolve(&literals()[2]).is_err(),
            "cursor_unknown_literal_granted_authority; case={case}"
        );
        assert_eq!(body, original, "cursor_mixed_source_changed");
    }
    Ok(())
}

#[test]
#[serial_test::serial]
fn mixed_readable_continuation_selected_completion_survives_history_eviction() -> TestResult {
    let _reset = Reset((*runtime::get_security_runtime()).clone());
    runtime::set_security_runtime_for_test(Default::default());
    for case in 0..4 {
        let (store, lease, binding, _, _) = message_projection_fixture()?;
        let call = json!({"id":"synthetic-eviction-item", "type":"function_call", "call_id":"synthetic-eviction-call", "name":"synthetic-tool", "arguments":"{}"});
        if case >= 2 {
            let response = json!({"id":"synthetic-eviction-completed", "object":"response", "status":"completed", "output":[call]});
            super::response_restoration::restore_and_publish_json_response(
                &response,
                false,
                &lease,
                &binding,
                &super::completed_context::CompletionWitness::from_verified_completion(true)?,
                &PayloadBudget::default(),
            )
            .map_err(|_| "eviction_completion_publish_failed")?;
        }
        let pending = lease
            .capture_delivered_history(&binding, &json!({"output":[call]}))
            .ok_or("eviction_capture_failed")?;
        assert!(
            lease.publish_delivered_history(&binding, pending),
            "eviction_history_publish_failed"
        );
        let mut body = json!({"input":[
            {"type":"function_call", "call_id":"synthetic-eviction-prior", "name":"synthetic-prior-tool", "arguments":"{}"},
            {"type":"function_call_output", "call_id":"synthetic-eviction-prior", "output":"Synthetic prior result"},
            call, {"type":"function_call_output", "call_id":"synthetic-eviction-call", "output":"Synthetic result"}
        ]});
        let original = body.clone();
        let probe_body = body.clone();
        let gateway = RestorationGateway::with_test_store_and_rules(
            store,
            &["chinese_phone_number"],
            PayloadBudget::default(),
        );
        let mut headers = http::HeaderMap::new();
        headers.insert("session-id", "synthetic-message-projection".parse()?);
        let request = match admit_http_request(
            &gateway,
            &AppType::Codex,
            true,
            false,
            false,
            &mut body,
            &headers,
        )? {
            RestorationAdmission::Admitted(request) => request,
            _ => return Err("eviction_admission_failed".into()),
        };
        let evicted = std::rc::Rc::new(std::cell::Cell::new(false));
        let observed = evicted.clone();
        let _hook =
            super::http_restoration::completion_selection_test_support::install(move || {
                if case != 0 {
                    lease
                        .evict_delivered_history_for_test()
                        .expect("synthetic_eviction_failed");
                    observed.set(
                        lease
                            .history_admission_candidate_projection(&probe_body)
                            .is_none(),
                    );
                } else {
                    observed.set(
                        lease
                            .history_admission_candidate_projection(&probe_body)
                            .is_some(),
                    );
                }
            });
        let transformed = std::cell::Cell::new(false);
        let attempt = request
            .prepare_attempt(
                if case == 2 {
                    "synthetic-other-provider"
                } else {
                    "synthetic-provider"
                },
                "openai_responses",
                "https://synthetic.invalid/v1/responses",
                Some("synthetic-principal"),
                &mut body,
                |candidate| {
                    transformed.set(true);
                    assert_eq!(
                        candidate, &original,
                        "evicted_unproven_history_changed; case={case}"
                    );
                    Ok(candidate.clone())
                },
            )
            .map_err(|_| "evicted_missing_context_blocked")?;
        assert!(evicted.get(), "history_eviction_control_not_exercised");
        assert!(transformed.get(), "evicted_history_not_prepared");
        assert_eq!(
            attempt.has_active_tokens(),
            case == 3,
            "evicted_completion_authority_changed; case={case}"
        );
        assert!(
            attempt.lease.resolve(&literals()[2]).is_err(),
            "evicted_unknown_literal_granted_authority; case={case}"
        );
        assert_eq!(body, original, "evicted_source_changed");
    }
    Ok(())
}

#[test]
#[serial_test::serial]
fn mixed_media_history_degrades_missing_completion_without_granting_authority() -> TestResult {
    let _reset = Reset((*runtime::get_security_runtime()).clone());
    runtime::set_security_runtime_for_test(Default::default());
    let (store, lease, binding, _, _) = message_projection_fixture()?;
    let current = json!({"id":"synthetic-media-current-item", "type":"function_call", "call_id":"synthetic-media-current-call", "name":"synthetic-current-tool", "arguments":"{}"});
    let response = json!({"id":"synthetic-media-completed", "object":"response", "status":"completed", "output":[current.clone()]});
    let restored = super::response_restoration::restore_and_publish_json_response(
        &response,
        false,
        &lease,
        &binding,
        &super::completed_context::CompletionWitness::from_verified_completion(true)?,
        &PayloadBudget::default(),
    )
    .map_err(|_| "media_completion_publish_failed")?;
    let pending = lease
        .capture_delivered_history(&binding, restored.as_value())
        .ok_or("media_capture_failed")?;
    assert!(
        lease.publish_delivered_history(&binding, pending),
        "media_delivery_failed"
    );
    let gateway = RestorationGateway::with_test_store_and_rules(
        store,
        &["chinese_phone_number"],
        PayloadBudget::default(),
    );
    let mut headers = http::HeaderMap::new();
    headers.insert("session-id", "synthetic-message-projection".parse()?);
    let prior_literal = literals()[2].clone();
    let mut body = json!({"previous_response_id":"synthetic-media-completed", "input":[
        {"type":"function_call", "call_id":"synthetic-media-prior-call", "name":"synthetic-prior-tool", "arguments":"{}"},
        {"type":"function_call_output", "call_id":"synthetic-media-prior-call", "output":[
            {"type":"input_image", "image_url":"data:image/png;base64,synthetic-opaque", "detail":"auto"},
            {"type":"input_text", "text":prior_literal}
        ]},
        current,
        {"type":"function_call_output", "call_id":"synthetic-media-current-call", "output":"Synthetic current result"},
        {"role":"user", "content":"Synthetic continue"}
    ]});
    let original = body.clone();
    let request = match admit_http_request(
        &gateway,
        &AppType::Codex,
        true,
        false,
        false,
        &mut body,
        &headers,
    )? {
        RestorationAdmission::Admitted(request) => request,
        _ => return Err("mixed_media_history_admission_failed".into()),
    };
    let transformed = std::cell::Cell::new(false);
    let attempt = request
        .prepare_attempt(
            "synthetic-provider",
            "openai_responses",
            "https://synthetic.invalid/v1/responses",
            Some("synthetic-principal"),
            &mut body,
            |candidate| {
                transformed.set(true);
                assert_eq!(candidate, &original, "mixed_media_history_changed");
                Ok(candidate.clone())
            },
        )
        .map_err(|_| "mixed_media_history_missing_completion_blocked")?;
    assert!(transformed.get(), "mixed_media_history_not_prepared");
    assert!(
        !attempt.has_active_tokens(),
        "unknown_media_history_granted_restoration_authority"
    );
    assert_eq!(body, original, "mixed_media_source_changed");
    Ok(())
}
