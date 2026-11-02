//! Synthetic fixtures for final protocol-bound request inspection and refusal audits.
use super::*;
use crate::proxy::security::{runtime::CompiledCustomRule, AuditAction, RiskLevel};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn request(source: &Value) -> Result<DlpHttpRequest, TokenError> {
    request_in_store(source, &DlpTokenStore::new())
}

fn request_in_store(source: &Value, store: &DlpTokenStore) -> Result<DlpHttpRequest, TokenError> {
    let budget = PayloadBudget::with_limit(16 * 1024 * 1024);
    let mut runtime = SecurityRuntimeConfig::default();
    runtime.custom_rules.push(CompiledCustomRule {
        id: "final-fixture".into(),
        name: "Synthetic policy".into(),
        rule_type: "regex".into(),
        pattern: "private-a".into(),
        regex: regex::Regex::new("private-a").ok(),
        risk_level: RiskLevel::High,
        action: AuditAction::RedactRestore,
        enabled: true,
    });
    Ok(DlpHttpRequest {
        base_lease: store.admit("codex", Some("final-fixture-session"), 1)?,
        runtime,
        restore_rules: StoreVec::new(store.allocation_ledger()?, 4096),
        budget: budget.clone(),
        source_body: PayloadJson::try_from_value(source, budget.clone(), budget.limit())?,
        pretokenized: false,
        endpoint_is_compact: false,
        audit_state: std::sync::Arc::new(RequestAuditState::new(
            &AppType::Codex,
            Some("final-fixture-session"),
            budget.clone(),
        )?),
        initial_audit: std::sync::Arc::new(PendingRequestAudit::empty(budget)?),
    })
}

fn prepare(
    request: &DlpHttpRequest,
    protocol: &str,
    final_body: &Value,
    source: &mut Value,
) -> Result<DlpPreparedAttempt, crate::proxy::error::ProxyError> {
    request.prepare_attempt_for_proxy(
        "final-fixture-provider",
        protocol,
        "https://synthetic.invalid/v1/responses",
        Some("synthetic-account"),
        source,
        |_| Ok(final_body.clone()),
    )
}

fn chat(arguments: &str) -> Value {
    json!({"messages":[{"role":"assistant","content":null,"tool_calls":[{
        "id":"call_final","type":"function","function":{"name":"fixture","arguments":arguments}
    }]}]})
}

fn native(arguments: &str) -> Value {
    json!({"input":[
        {"type":"function_call","call_id":"call_safe","name":"fixture","arguments":"{\"value\":\"safe\"}"},
        {"type":"function_call","call_id":"call_final","name":"fixture","arguments":arguments}
    ]})
}

#[test]
fn final_validation_chat_decodes_escaped_nested_arguments_after_override() -> TestResult {
    let mut source = json!({"input":"safe"});
    let request = request(&source)?;
    let outbound = chat(r#"{"nested":[{"value":"\u0070rivate-a"}]}"#);
    let result = prepare(&request, "openai_chat", &outbound, &mut source);
    assert!(
        matches!(
            result,
            Err(crate::proxy::error::ProxyError::DlpForward(
                DlpForwardError::RedactionFailed
            ))
        ),
        "decoded protective findings must refuse the actual outbound body"
    );
    assert_eq!(
        source,
        json!({"input":"safe"}),
        "refusal must not publish the candidate"
    );
    Ok(())
}

#[test]
fn final_validation_native_malformed_second_arguments_fail_closed() -> TestResult {
    let mut source = json!({"input":"safe"});
    let request = request(&source)?;
    let result = prepare(
        &request,
        "openai_responses",
        &native("{broken"),
        &mut source,
    );
    assert!(
        matches!(
            result,
            Err(crate::proxy::error::ProxyError::DlpForward(
                DlpForwardError::RedactionFailed
            ))
        ),
        "a later malformed argument document cannot disable final inspection"
    );
    Ok(())
}

#[test]
fn final_validation_native_deep_second_arguments_fail_closed() -> TestResult {
    let mut source = json!({"input":"safe"});
    let request = request(&source)?;
    let deep = format!("{}\"safe\"{}", "[".repeat(80), "]".repeat(80));
    let result = prepare(&request, "openai_responses", &native(&deep), &mut source);
    assert!(
        matches!(
            result,
            Err(crate::proxy::error::ProxyError::DlpForward(
                DlpForwardError::CapacityExceeded
            ))
        ),
        "argument depth exhaustion must propagate rather than permit forwarding"
    );
    Ok(())
}

#[test]
fn final_validation_chat_malformed_arguments_fail_closed() -> TestResult {
    let mut source = json!({"input":"safe"});
    let request = request(&source)?;
    assert!(
        prepare(&request, "openai_chat", &chat("{broken"), &mut source).is_err(),
        "converted arguments must be parsed even without a raw-text finding"
    );
    Ok(())
}

#[test]
fn final_validation_chat_unknown_readonly_content_denies_protective_findings() -> TestResult {
    let mut source = json!({"input":"safe"});
    let request = request(&source)?;
    let outbound = json!({"messages":[{"role":"user","content":[{"type":"future_text","value":"private-a"}]}]});
    assert!(
        prepare(&request, "openai_chat", &outbound, &mut source).is_err(),
        "unknown readable carriers are not authorized to forward sensitive plaintext"
    );
    Ok(())
}

fn assert_final_refusal_audit(protocol: &str, outbound: Value) -> TestResult {
    let mut source = json!({"input":"safe"});
    let request = request(&source)?;
    let observe = request.audit_observer_for_test();
    assert!(
        matches!(
            prepare(&request, protocol, &outbound, &mut source),
            Err(crate::proxy::error::ProxyError::DlpForward(
                DlpForwardError::RedactionFailed
            ))
        ),
        "final protected plaintext must produce the fixed refusal"
    );
    assert_eq!(
        source,
        json!({"input":"safe"}),
        "refusal must leave the original body untouched"
    );
    let events = observe();
    assert_eq!(
        events.len(),
        1,
        "transform-injected finding needs one refusal audit"
    );
    assert_eq!(events[0].log.rule_id, "final-fixture");
    assert_eq!(events[0].log.action_taken, "block");
    assert_eq!(events[0].log.provider_id, "final-fixture-provider");
    let encoded = serde_json::to_string(&events[0].log)?;
    assert!(
        !encoded.contains("private-a"),
        "audit must not expose matched plaintext"
    );
    assert!(
        !encoded.contains("rredact-"),
        "audit must not expose token references"
    );
    Ok(())
}

#[test]
fn final_validation_chat_readonly_tool_metadata_denies_plaintext() -> TestResult {
    let safe = json!({"messages":[{"role":"assistant","content":null,
        "name":"fixture","tool_call_id":"call_final",
        "tool_calls":[{"id":"call_final","type":"function","function":{
            "name":"fixture","arguments":"{\"value\":\"safe\"}"
        }}],"function_call":{"name":"fixture","arguments":"{}"}
    }]});
    for pointer in [
        "/messages/0/name",
        "/messages/0/tool_call_id",
        "/messages/0/tool_calls/0/id",
        "/messages/0/tool_calls/0/function/name",
        "/messages/0/function_call/name",
    ] {
        let mut outbound = safe.clone();
        let field = outbound
            .pointer_mut(pointer)
            .ok_or("synthetic metadata fixture is missing")?;
        *field = json!("private-a");
        assert_final_refusal_audit("openai_chat", outbound)?;
    }
    Ok(())
}

#[test]
fn final_validation_anthropic_readonly_tool_metadata_denies_plaintext() -> TestResult {
    let safe = json!({"messages":[
        {"role":"assistant","content":[{"type":"tool_use","id":"call_final",
            "name":"fixture","input":{"value":"safe"}}]},
        {"role":"user","content":[{"type":"tool_result","tool_use_id":"call_final",
            "content":"safe"}]}
    ]});
    for pointer in [
        "/messages/0/content/0/id",
        "/messages/0/content/0/name",
        "/messages/1/content/0/tool_use_id",
    ] {
        let mut outbound = safe.clone();
        let field = outbound
            .pointer_mut(pointer)
            .ok_or("synthetic metadata fixture is missing")?;
        *field = json!("private-a");
        assert_final_refusal_audit("anthropic", outbound)?;
    }
    Ok(())
}

#[test]
fn final_validation_native_transform_refusal_is_audited() -> TestResult {
    assert_final_refusal_audit("openai_responses", native(r#"{"value":"\u0070rivate-a"}"#))
}

#[test]
fn final_validation_chat_transform_refusal_is_audited() -> TestResult {
    assert_final_refusal_audit("openai_chat", chat(r#"{"value":"private-a"}"#))
}

#[test]
fn final_validation_anthropic_transform_refusal_is_audited() -> TestResult {
    assert_final_refusal_audit(
        "anthropic",
        json!({"messages":[{"role":"assistant","content":[{
            "type":"tool_use","id":"tool_final","name":"fixture","input":{"nested":[{"value":"private-a"}]}
        }]}]}),
    )
}

fn assert_malformed_final_arguments_preserve_sibling_audit(
    protocol: &str,
    malformed_first: bool,
) -> TestResult {
    let sensitive_arguments = if protocol == "anthropic" {
        json!({"nested":[{"value":"private-a"}]})
    } else {
        json!(r#"{"nested":[{"value":"\u0070rivate-a"}]}"#)
    };
    let call = |id: &str, arguments: Value| match protocol {
        "openai_chat" => json!({"id":id,"type":"function","function":{
            "name":"fixture","arguments":arguments
        }}),
        "anthropic" => json!({"type":"tool_use","id":id,"name":"fixture","input":arguments}),
        _ => json!({"type":"function_call","call_id":id,"name":"fixture","arguments":arguments}),
    };
    let sensitive = call("call_sensitive", sensitive_arguments);
    let malformed = call("call_malformed", json!(r#"{"nested":[{"value":"safe"}]"#));
    let calls = if malformed_first {
        vec![malformed, sensitive]
    } else {
        vec![sensitive, malformed]
    };
    let outbound = match protocol {
        "openai_chat" => {
            json!({"messages":[{"role":"assistant","content":null,"tool_calls":calls}]})
        }
        "anthropic" => json!({"messages":[{"role":"assistant","content":calls}]}),
        _ => json!({"input":calls}),
    };
    assert_final_refusal_audit(protocol, outbound)
}

#[test]
fn final_validation_chat_malformed_first_arguments_preserve_sibling_audit() -> TestResult {
    assert_malformed_final_arguments_preserve_sibling_audit("openai_chat", true)
}

#[test]
fn final_validation_chat_malformed_last_arguments_preserve_sibling_audit() -> TestResult {
    assert_malformed_final_arguments_preserve_sibling_audit("openai_chat", false)
}

#[test]
fn final_validation_anthropic_malformed_first_arguments_preserve_sibling_audit() -> TestResult {
    assert_malformed_final_arguments_preserve_sibling_audit("anthropic", true)
}

#[test]
fn final_validation_anthropic_malformed_last_arguments_preserve_sibling_audit() -> TestResult {
    assert_malformed_final_arguments_preserve_sibling_audit("anthropic", false)
}

#[test]
fn final_validation_native_malformed_first_arguments_preserve_sibling_audit() -> TestResult {
    assert_malformed_final_arguments_preserve_sibling_audit("openai_responses", true)
}

#[test]
fn final_validation_native_malformed_last_arguments_preserve_sibling_audit() -> TestResult {
    assert_malformed_final_arguments_preserve_sibling_audit("openai_responses", false)
}

#[test]
fn final_validation_opaque_and_schema_fields_are_not_inspection_text() -> TestResult {
    for (protocol, outbound) in [
        (
            "openai_responses",
            json!({"input":[{"type":"compaction","encrypted_content":"private-a"}],
            "tools":[{"type":"function","name":"fixture","parameters":{"description":"private-a"}}]}),
        ),
        (
            "openai_chat",
            json!({"messages":[{"role":"user","content":[{"type":"image_url","image_url":{"url":"private-a"}}]}],
            "tools":[{"type":"function","function":{"name":"fixture","parameters":{"description":"private-a"}}}]}),
        ),
        (
            "anthropic",
            json!({"system":[{"type":"text","text":"safe"}],"messages":[{"role":"assistant","content":[
            {"type":"thinking","thinking":"safe","signature":"private-a"},
            {"type":"redacted_thinking","data":"private-a"},
            {"type":"image","source":{"type":"base64","data":"private-a"}}
        ]}],"tools":[{"name":"fixture","input_schema":{"description":"private-a"}}]}),
        ),
    ] {
        let mut source = json!({"input":"safe"});
        let request = request(&source)?;
        assert!(
            prepare(&request, protocol, &outbound, &mut source).is_ok(),
            "schemas and opaque protocol carriers must remain outside text inspection"
        );
        assert_eq!(
            source, outbound,
            "inspection must preserve outbound semantics"
        );
    }
    Ok(())
}

#[test]
fn final_validation_unknown_resolved_protocol_is_denied() -> TestResult {
    let mut source = json!({"input":"safe"});
    let request = request(&source)?;
    assert!(
        matches!(
            prepare(
                &request,
                "future_protocol",
                &json!({"input":"safe"}),
                &mut source
            ),
            Err(crate::proxy::error::ProxyError::DlpForward(
                DlpForwardError::RestoreUnsupported
            ))
        ),
        "final inspection must bind to an explicitly supported actual protocol"
    );
    Ok(())
}

#[test]
fn final_validation_valid_tokens_survive_protocol_conversion_without_plaintext() -> TestResult {
    for protocol in ["openai_responses", "openai_chat", "anthropic"] {
        let mut source = json!({"input":"private-a"});
        let request = request(&source)?;
        let attempt = request.prepare_attempt_for_proxy("final-fixture-provider", protocol,
            "https://synthetic.invalid/v1/responses", Some("synthetic-account"), &mut source, |prepared| {
                let text = prepared["input"].as_str().ok_or(crate::proxy::error::ProxyError::DlpForward(DlpForwardError::RedactionFailed))?;
                let arguments = json!({"nested":[{"value":text}]}).to_string();
                Ok(match protocol {
                    "openai_chat" => chat(&arguments),
                    "anthropic" => json!({"messages":[{"role":"assistant","content":[{"type":"tool_use","id":"tool_final","name":"fixture","input":{"nested":[{"value":text}]}}]}]}),
                    _ => native(&arguments),
                })
            });
        assert!(
            attempt.is_ok(),
            "authorized references remain valid in decoded inspection fields"
        );
        let encoded = serde_json::to_string(&source)?;
        assert!(
            !encoded.contains("private-a"),
            "conversion cannot reintroduce plaintext"
        );
        assert!(
            encoded.contains("rredact-"),
            "conversion must retain the authorized reference"
        );
    }
    Ok(())
}

#[test]
fn final_validation_actual_body_immutable_keys_refuse_authorized_references() -> TestResult {
    for protocol in ["openai_chat", "anthropic", "openai_responses"] {
        for placement in 0..3 {
            let mut source = json!({"input":"private-a"});
            let original = source.clone();
            let request = request(&source)?;
            let budget = request.budget.clone();
            let result = request.prepare_attempt_for_proxy(
                "final-fixture-provider", protocol,
                "https://synthetic.invalid/v1/responses", Some("synthetic-account"),
                &mut source, |prepared| {
                    let token = prepared["input"].as_str().ok_or(
                        crate::proxy::error::ProxyError::DlpForward(DlpForwardError::RedactionFailed)
                    )?;
                    let mut body = match protocol {
                        "openai_chat" => json!({"messages":[{"role":"user","content":[{"type":"text","text":"safe"}]}]}),
                        "anthropic" => json!({"messages":[{"role":"user","content":[{"type":"text","text":"safe"}]}]}),
                        _ => json!({"input":[{"role":"user","content":[{"type":"input_text","text":"safe"}]}]}),
                    };
                    match placement {
                        0 => body["metadata"] = json!({token:1}),
                        1 => {
                            let root = if protocol == "openai_responses" {"input"} else {"messages"};
                            body[root][0]["content"][0][token] = json!(1);
                        }
                        _ => body["tools"] = json!([{ "type":"function", "parameters":{"properties":{token:{"type":"string"}}}}]),
                    }
                    Ok(body)
                },
            );
            assert!(
                matches!(
                    result,
                    Err(crate::proxy::error::ProxyError::DlpForward(
                        DlpForwardError::ContextUnavailable
                    ))
                ),
                "authorized immutable key must be refused case={placement}"
            );
            assert!(source == original, "refusal cannot publish the candidate");
            drop(result);
            drop(request);
            assert!(
                budget.used() == 0,
                "refusal must release transient payload owners"
            );
        }
    }
    Ok(())
}

#[test]
fn final_validation_actual_body_unknown_key_literal_preserves_encoding() -> TestResult {
    let literal = format!("rredact-custom-{}-ai", "a".repeat(20));
    for protocol in ["openai_chat", "anthropic", "openai_responses"] {
        let mut source = json!({"input":"private-a"});
        let request = request(&source)?;
        let budget = request.budget.clone();
        let mut body = if protocol == "openai_responses" {
            json!({"input":"safe"})
        } else {
            json!({"messages":[{"role":"user","content":"safe"}]})
        };
        body["metadata"] = json!({&literal:1});
        let attempt = prepare(&request, protocol, &body, &mut source)?;
        assert!(
            source == body,
            "unknown immutable literal must remain unchanged"
        );
        assert!(
            attempt.lease.resolve_if_authorized(&literal)?.is_none(),
            "literal cannot gain authority"
        );
        let encoded = attempt
            .encode_outbound_body(&source)
            .map_err(|_| "synthetic body encoding refused")?;
        let bytes = encoded.as_ref().ok_or("synthetic body encoding absent")?;
        assert!(
            bytes.as_ref() == serde_json::to_vec(&body)?.as_slice(),
            "actual encoded body must preserve the literal"
        );
        drop(encoded);
        drop(attempt);
        drop(request);
        assert!(
            budget.used() == 0,
            "successful send encoding must release payload owners"
        );
    }
    Ok(())
}

#[test]
fn final_validation_password_context_is_preserved_in_refusal_audit() -> TestResult {
    let mut source = json!({"input":"safe"});
    let mut request = request(&source)?;
    request
        .runtime
        .builtin_action_overrides
        .insert("catalog:password-field".into(), AuditAction::RedactRestore);
    let observe = request.audit_observer_for_test();
    let result = prepare(
        &request,
        "openai_chat",
        &chat(r#"{"nested":{"password":"synthetic-only"}}"#),
        &mut source,
    );
    assert!(
        result.is_err(),
        "structured password policy must refuse final plaintext"
    );
    let events = observe();
    assert!(
        events
            .iter()
            .any(|event| event.log.rule_id == "catalog:password-field"
                && event.log.action_taken == "block"),
        "final audit must retain password-field inspection context"
    );
    Ok(())
}

#[test]
fn final_validation_exhausted_budget_is_propagated_without_publication() -> TestResult {
    let mut source = json!({"input":"safe"});
    let request = request(&source)?;
    let budget = request.budget.clone();
    let mut held = None;
    let result = request.prepare_attempt_for_proxy(
        "final-fixture-provider",
        "openai_chat",
        "https://synthetic.invalid/v1/responses",
        Some("synthetic-account"),
        &mut source,
        |_| {
            held = Some(
                budget
                    .reserve(budget.limit() - budget.used())
                    .map_err(|error| {
                        crate::proxy::error::ProxyError::DlpForward(
                            DlpForwardError::from_token_error(error),
                        )
                    })?,
            );
            Ok(chat(r#"{"value":"safe"}"#))
        },
    );
    assert!(
        matches!(
            result,
            Err(crate::proxy::error::ProxyError::DlpForward(
                DlpForwardError::CapacityExceeded
            ))
        ),
        "final inspection budget failure cannot permit forwarding"
    );
    assert_eq!(source, json!({"input":"safe"}));
    drop(held);
    Ok(())
}

fn final_binding_check(
    attempt: &DlpPreparedAttempt,
    provider: &str,
    protocol: &str,
    target: &str,
    principal: Option<&str>,
) -> Result<(), DlpForwardError> {
    attempt.validate_final_send_binding(provider, protocol, target, principal)
}

#[test]
fn final_send_binding_preserves_exact_actual_attempt() -> TestResult {
    let mut source = json!({"input":"private-a"});
    let request = request(&source)?;
    let attempt = request.prepare_attempt_for_proxy(
        "final-fixture-provider",
        "openai_responses",
        "https://synthetic.invalid/v1/responses",
        Some("synthetic-account"),
        &mut source,
        |prepared| Ok(prepared.clone()),
    )?;
    assert!(
        final_binding_check(
            &attempt,
            "final-fixture-provider",
            "openai_responses",
            "https://synthetic.invalid/v1/responses",
            Some("synthetic-account"),
        )
        .is_ok(),
        "an unchanged final binding must remain permitted"
    );
    Ok(())
}

#[test]
fn final_send_binding_rejects_changed_actual_attempt() -> TestResult {
    let mut source = json!({"input":"private-a"});
    let request = request(&source)?;
    let attempt = request.prepare_attempt_for_proxy(
        "final-fixture-provider",
        "openai_responses",
        "https://synthetic.invalid/v1/responses",
        Some("synthetic-account"),
        &mut source,
        |prepared| Ok(prepared.clone()),
    )?;
    for (provider, protocol, target, principal) in [
        (
            "changed-provider",
            "openai_responses",
            "https://synthetic.invalid/v1/responses",
            Some("synthetic-account"),
        ),
        (
            "final-fixture-provider",
            "openai_chat",
            "https://synthetic.invalid/v1/responses",
            Some("synthetic-account"),
        ),
        (
            "final-fixture-provider",
            "openai_responses",
            "https://synthetic.invalid/v1/responses/compact",
            Some("synthetic-account"),
        ),
        (
            "final-fixture-provider",
            "openai_responses",
            "https://synthetic.invalid/v1/responses",
            Some("changed-account"),
        ),
        (
            "final-fixture-provider",
            "openai_responses",
            "https://synthetic.invalid/v1/responses",
            None,
        ),
    ] {
        assert!(
            matches!(
                final_binding_check(&attempt, provider, protocol, target, principal),
                Err(DlpForwardError::ContextUnavailable)
            ),
            "a changed final binding must fail before transport"
        );
    }
    Ok(())
}

#[tokio::test]
async fn reasoning_tool_publication_preserves_literal_leaf_and_required_context() -> TestResult {
    use http_body_util::BodyExt;
    for literal in [
        format!("rredact-custom-{}-ai", "a".repeat(20)),
        "rredact-custom-".to_owned(),
    ] {
        let mut source = json!({"input":"private-a"});
        let request = request(&source)?;
        let attempt = request.prepare_attempt_for_proxy(
            "final-fixture-provider",
            "openai_responses",
            "https://synthetic.invalid/v1/responses",
            Some("synthetic-account"),
            &mut source,
            |prepared| Ok(prepared.clone()),
        )?;
        let raw = json!({"id":"resp_reasoning_publish","object":"response","status":"completed","output":[{
            "id":"fc_reasoning_publish","type":"function_call","status":"completed",
            "call_id":"call_reasoning_publish","name":"synthetic_tool","arguments":"{}",
            "reasoning_content":literal
        }]});
        let response = attempt
            .restore_client_http_response(
                Response::builder()
                    .header("content-type", "application/json")
                    .body(Body::from(raw.to_string()))?,
                ArgumentNormalizationMode::Exact,
                None,
            )
            .await;
        assert!(
            response.status() == StatusCode::OK,
            "literal tool reasoning must survive mandatory context publication"
        );
        let bytes = response.into_body().collect().await?.to_bytes();
        let delivered: Value = serde_json::from_slice(&bytes)?;
        assert!(
            delivered["output"][0]["reasoning_content"].as_str() == Some(literal.as_str()),
            "tool reasoning must remain byte-identical after publication"
        );
        let mut continuation = request.base_lease.fork_attempt()?;
        let binding = continuation.bind_actual_attempt(
            "final-fixture-provider",
            "openai_responses",
            "https://synthetic.invalid/v1/responses",
            Some("synthetic-account"),
        )?;
        assert!(
            continuation
                .resume_completed_context(&binding, Some("resp_reasoning_publish"), &[], &[])
                .is_ok(),
            "successful delivery must retain mandatory continuation context"
        );
    }
    Ok(())
}

#[tokio::test]
async fn reasoning_empty_lease_keeps_native_completion_validation() -> TestResult {
    let mut source = json!({"input":"safe"});
    let request = request(&source)?;
    let attempt = request.prepare_attempt_for_proxy(
        "final-fixture-provider",
        "openai_responses",
        "https://synthetic.invalid/v1/responses",
        Some("synthetic-account"),
        &mut source,
        |prepared| Ok(prepared.clone()),
    )?;
    let raw = json!({"id":"resp_reasoning_empty","object":"response","status":"incomplete","output":[{
        "id":"rs_reasoning_empty","type":"reasoning","summary":[{"type":"summary_text","text":"synthetic reasoning"}]
    }]});
    let response = attempt
        .restore_client_http_response(
            Response::builder()
                .header("content-type", "application/json")
                .body(Body::from(raw.to_string()))?,
            ArgumentNormalizationMode::Exact,
            None,
        )
        .await;
    assert!(
        response.status() == StatusCode::BAD_GATEWAY,
        "empty token authority must not skip native completion validation"
    );
    Ok(())
}
#[test]
fn reasoning_empty_lease_requires_converted_original_sse_evidence() -> TestResult {
    for protocol in ["openai_chat", "anthropic"] {
        let mut source = json!({"input":"synthetic safe input"});
        let request = request(&source)?;
        let outbound = json!({"model":"synthetic-model","messages":[{
            "role":"user","content":"synthetic safe input"
        }]});
        let attempt = prepare(&request, protocol, &outbound, &mut source)?;
        assert!(
            !attempt.has_active_tokens(),
            "synthetic lease must be empty"
        );
        assert!(
            attempt.upstream_sse_evidence_tracker().is_some(),
            "converted completion evidence must not depend on token occupancy"
        );
    }
    Ok(())
}

#[test]
fn reasoning_empty_lease_requires_xai_original_sse_evidence() -> TestResult {
    let mut source = json!({"input":"synthetic safe input"});
    let request = request(&source)?;
    let outbound = source.clone();
    let attempt = prepare(&request, "openai_responses", &outbound, &mut source)?;
    assert!(
        !attempt.has_active_tokens(),
        "synthetic lease must be empty"
    );
    assert!(
        attempt.xai_original_sse_evidence_tracker().is_some(),
        "original rewrite evidence must not depend on token occupancy"
    );
    Ok(())
}

#[tokio::test]
async fn reasoning_delivered_history_json_requires_final_success() -> TestResult {
    use crate::proxy::security::history_provenance::DlpHistoryLeafField;
    use http_body_util::BodyExt;
    for active_tokens in [false, true] {
        for reasoning in [false, true] {
            let mut source = json!({"input":if active_tokens { "private-a" } else { "safe" }});
            let request = request(&source)?;
            let attempt = request.prepare_attempt_for_proxy(
                "final-fixture-provider",
                "openai_responses",
                "https://synthetic.invalid/v1/responses",
                Some("synthetic-account"),
                &mut source,
                |prepared| Ok(prepared.clone()),
            )?;
            let binding = attempt.binding.clone();
            let literal = "synthetic literal rredact-custom-";
            let item = if reasoning {
                json!({"id":"synthetic-delivered-reasoning","type":"reasoning",
                    "summary":[{"type":"summary_text","text":literal}]})
            } else {
                json!({"id":"synthetic-delivered-message","type":"message","role":"assistant",
                    "status":"completed","content":[{"type":"output_text","text":literal,"annotations":[]}]})
            };
            let raw = json!({"id":"synthetic-delivered-response","object":"response",
                "status":"completed","output":[item]});
            let replay = json!({"input":raw["output"]});
            assert!(
                request
                    .base_lease
                    .history_projection(&binding, &replay)
                    .is_none(),
                "unpublished response must not issue delivered history authority"
            );
            let response = attempt
                .restore_client_http_response(
                    Response::builder()
                        .header("content-type", "application/json")
                        .body(Body::from(raw.to_string()))?,
                    ArgumentNormalizationMode::Exact,
                    None,
                )
                .await;
            assert!(
                response.status() == StatusCode::OK,
                "validated literal response must remain deliverable"
            );
            let bytes = response.into_body().collect().await?.to_bytes();
            let delivered: Value = serde_json::from_slice(&bytes)
                .map_err(|_| "synthetic delivered response must be valid JSON")?;
            let replay = json!({"input":delivered["output"]});
            let projection = request
                .base_lease
                .history_projection(&binding, &replay)
                .ok_or("successful final delivery must publish optional history proof")?;
            let field = if reasoning {
                DlpHistoryLeafField::ReasoningSummary
            } else {
                DlpHistoryLeafField::OutputText
            };
            assert!(
                projection.source_for_responses_leaf(0, field, 0).is_some(),
                "actual delivered typed text must retain exact history provenance"
            );
        }
    }
    Ok(())
}

#[tokio::test]
async fn reasoning_delivered_history_json_failed_completion_never_publishes() -> TestResult {
    for active_tokens in [false, true] {
        let mut source = json!({"input":if active_tokens { "private-a" } else { "safe" }});
        let request = request(&source)?;
        let attempt = request.prepare_attempt_for_proxy(
            "final-fixture-provider",
            "openai_responses",
            "https://synthetic.invalid/v1/responses",
            Some("synthetic-account"),
            &mut source,
            |prepared| Ok(prepared.clone()),
        )?;
        let binding = attempt.binding.clone();
        let raw = json!({"id":"synthetic-failed-history","object":"response","status":"incomplete",
            "output":[{"id":"synthetic-failed-reasoning","type":"reasoning",
                "summary":[{"type":"summary_text","text":"synthetic failed reasoning"}]}]});
        let replay = json!({"input":raw["output"]});
        let response = attempt
            .restore_client_http_response(
                Response::builder()
                    .header("content-type", "application/json")
                    .body(Body::from(raw.to_string()))?,
                ArgumentNormalizationMode::Exact,
                None,
            )
            .await;
        assert!(
            response.status() == StatusCode::BAD_GATEWAY,
            "failed completion must preserve failure semantics"
        );
        assert!(
            request
                .base_lease
                .history_projection(&binding, &replay)
                .is_none(),
            "failed completion must never publish optional history proof"
        );
    }
    Ok(())
}

fn synthetic_history_sse_response() -> Value {
    json!({"id":"synthetic_sse_history", "object":"response", "status":"completed", "output":[
        {"id":"synthetic_sse_reasoning", "type":"reasoning",
         "summary":[{"type":"summary_text","text":"Synthetic 思考 rredact-custom-"}],
         "content":[{"type":"reasoning_text","text":"Synthetic second 思考 rredact-custom-"}]},
        {"id":"synthetic_sse_message", "type":"message", "role":"assistant", "status":"completed",
         "content":[{"type":"output_text","text":"Synthetic output rredact-custom-","annotations":[]}]}
    ]})
}

fn synthetic_history_sse_frame(event: &str, data: &Value) -> Bytes {
    Bytes::from(format!("event: {event}\ndata: {data}\n\n"))
}

#[tokio::test]
async fn reasoning_delivered_history_sse_requires_terminal_release() -> TestResult {
    use crate::proxy::security::history_provenance::DlpHistoryLeafField as Field;
    use http_body_util::BodyExt;
    for active_tokens in [false, true] {
        let mut source = json!({"input":if active_tokens {"private-a"} else {"safe"}});
        let request = request(&source)?;
        let attempt = request.prepare_attempt_for_proxy(
            "final-fixture-provider",
            "openai_responses",
            "https://synthetic.invalid/v1/responses",
            Some("synthetic-account"),
            &mut source,
            |prepared| Ok(prepared.clone()),
        )?;
        let binding = attempt.binding.clone();
        let response = synthetic_history_sse_response();
        let replay = json!({"input":response["output"]});
        let terminal = synthetic_history_sse_frame(
            "response.completed",
            &json!({"type":"response.completed","response":response}),
        );
        let chunks: Vec<_> = terminal
            .chunks(7)
            .map(|chunk| Ok::<_, std::io::Error>(Bytes::copy_from_slice(chunk)))
            .collect();
        let restored = attempt
            .restore_client_http_response(
                Response::builder()
                    .header("content-type", "text/event-stream")
                    .body(Body::from_stream(futures::stream::iter(chunks)))?,
                ArgumentNormalizationMode::Exact,
                None,
            )
            .await;
        assert!(
            restored.status() == StatusCode::OK,
            "synthetic SSE wrapper must be accepted"
        );
        assert!(
            request
                .base_lease
                .history_projection(&binding, &replay)
                .is_none(),
            "unpolled SSE must not publish proof"
        );
        let bytes = restored
            .into_body()
            .collect()
            .await
            .map_err(|_| "synthetic SSE body collection failed")?
            .to_bytes();
        let text = std::str::from_utf8(&bytes).map_err(|_| "synthetic SSE must remain UTF-8")?;
        assert!(
            text.contains("event: response.completed") && !text.contains("event: response.failed"),
            "valid SSE must release the completion barrier"
        );
        let projection = request
            .base_lease
            .history_projection(&binding, &replay)
            .ok_or("released terminal SSE must publish optional history")?;
        for (index, field) in [
            (0, Field::ReasoningSummary),
            (0, Field::ReasoningContent),
            (1, Field::OutputText),
        ] {
            assert!(
                projection
                    .source_for_responses_leaf(index, field, 0)
                    .is_some(),
                "delivered supported SSE leaves must carry exact proof"
            );
        }
    }
    Ok(())
}

// Shared optional-ledger equality requires --test-threads=1.
#[tokio::test]
#[serial_test::serial]
async fn reasoning_delivered_history_sse_final_yield_requires_wrapper_resume() -> TestResult {
    use crate::proxy::security::history_provenance::DlpHistoryLeafField as Field;
    use http_body_util::BodyExt;

    for active_tokens in [false, true] {
        for finish_release in [false, true] {
            let store = DlpTokenStore::new();
            let (global_before, control_before, pinned_response_before) = {
                let warm = store.admit("codex", Some("synthetic-sse-ledger-warmup"), 1)?;
                let binding = warm.bind_actual_attempt(
                    "final-fixture-provider",
                    "openai_responses",
                    "https://synthetic.invalid/v1/responses",
                    Some("synthetic-account"),
                )?;
                let response = synthetic_history_sse_response();
                let replay = json!({"input":response["output"]});
                let pending = warm
                    .capture_delivered_history(&binding, &response)
                    .ok_or("synthetic SSE ledger warmup unavailable")?;
                assert!(
                    warm.history_projection(&binding, &replay).is_none(),
                    "pending warmup capture must not authorize undelivered history"
                );
                drop(pending);
                let control = store
                    .history_metadata_bytes_for_test()
                    .ok_or("synthetic SSE accounting unavailable")?;
                let pending = warm
                    .capture_delivered_history(&binding, &response)
                    .ok_or("synthetic SSE pinned warmup unavailable")?;
                assert!(
                    store.history_metadata_bytes_for_test() == Some(control + 256 * 1024),
                    "pending capture must retain the complete response charge"
                );
                drop(warm);
                store.expire_idle_at(
                    std::time::Instant::now() + std::time::Duration::from_secs(7200),
                )?;
                assert!(
                    store.stats()?.namespaces == 0,
                    "pending capture must not keep a namespace live"
                );
                let pinned_response = store
                    .history_metadata_bytes_for_test()
                    .ok_or("synthetic SSE pinned accounting unavailable")?;
                drop(pending);
                let global = store
                    .history_metadata_bytes_for_test()
                    .ok_or("synthetic SSE accounting unavailable")?;
                assert!(
                    control > global,
                    "warmup must establish controller-only charge"
                );
                assert!(
                    pinned_response > global + 256 * 1024,
                    "a pending owner must pin the response and namespace controller charge"
                );
                (global, control, pinned_response)
            };
            let mut source = json!({"input":if active_tokens {"private-a"} else {"safe"}});
            let request = request_in_store(&source, &store)?;
            let budget = request.budget.clone();
            let attempt = request.prepare_attempt_for_proxy(
                "final-fixture-provider",
                "openai_responses",
                "https://synthetic.invalid/v1/responses",
                Some("synthetic-account"),
                &mut source,
                |prepared| Ok(prepared.clone()),
            )?;
            assert!(
                attempt.has_restoration_lease() == active_tokens,
                "wrapper fixture must cover the requested token-authority branch"
            );
            let binding = attempt.binding.clone();
            let response = synthetic_history_sse_response();
            let replay = json!({"input":response["output"]});
            assert!(
                request
                    .base_lease
                    .history_projection(&binding, &replay)
                    .is_none(),
                "undelivered SSE fixture must start without history proof"
            );
            let terminal = synthetic_history_sse_frame(
                "response.completed",
                &json!({"type":"response.completed","response":response}),
            );
            let restored = attempt
                .restore_client_http_response(
                    Response::builder()
                        .header("content-type", "text/event-stream")
                        .body(Body::from_stream(futures::stream::iter([Ok::<
                            _,
                            std::io::Error,
                        >(
                            terminal
                        )])))?,
                    ArgumentNormalizationMode::Exact,
                    None,
                )
                .await;
            assert!(
                restored.status() == StatusCode::OK,
                "synthetic terminal wrapper must be accepted"
            );
            let mut body = restored.into_body();
            let mut terminal_yielded = false;
            for _ in 0..16 {
                let frame = body
                    .frame()
                    .await
                    .ok_or("synthetic terminal yield missing")?
                    .map_err(|_| "synthetic terminal yield failed")?;
                let data = frame.data_ref().ok_or("synthetic SSE data frame missing")?;
                let text = std::str::from_utf8(data)
                    .map_err(|_| "synthetic terminal yield must be UTF-8")?;
                assert!(
                    !text.contains("event: response.failed"),
                    "valid synthetic wrapper must not yield failure"
                );
                assert!(
                    request
                        .base_lease
                        .history_projection(&binding, &replay)
                        .is_none(),
                    "no yielded frame may publish proof before the wrapper resumes"
                );
                if text.contains("event: response.completed") {
                    terminal_yielded = true;
                    break;
                }
            }
            assert!(terminal_yielded, "synthetic final frame must be reached");
            assert!(
                store.history_metadata_bytes_for_test() == Some(control_before + 256 * 1024),
                "the final yield must retain the complete response proof charge without publication"
            );
            if finish_release {
                assert!(
                    body.frame().await.is_none(),
                    "resuming past the terminal yield must finish the wrapper"
                );
                let projection = request
                    .base_lease
                    .history_projection(&binding, &replay)
                    .ok_or("resumed terminal release must publish proof")?;
                for (index, field) in [
                    (0, Field::ReasoningSummary),
                    (0, Field::ReasoningContent),
                    (1, Field::OutputText),
                ] {
                    assert!(
                        projection
                            .source_for_responses_leaf(index, field, 0)
                            .is_some(),
                        "fully released wrapper must prove each supported delivered leaf"
                    );
                }
                drop(body);
                drop(request);
                assert!(
                    budget.used() == 0,
                    "completed wrapper must release transient owners"
                );
                store.expire_idle_at(
                    std::time::Instant::now() + std::time::Duration::from_secs(7200),
                )?;
                assert!(
                    store.stats()?.namespaces == 0,
                    "projection ownership must not keep an expired namespace live"
                );
                assert!(
                    store.history_metadata_bytes_for_test() == Some(pinned_response_before + 2048),
                    "expiry must retain the response, controller and exact projection permit charge"
                );
                drop(projection);
                assert!(
                    store.history_metadata_bytes_for_test() == Some(global_before),
                    "the last projection owner must release proof and controller charge"
                );
            } else {
                // Cancellation here is after the final yield, not before EOF/finalize.
                drop(body);
                assert!(
                    request
                        .base_lease
                        .history_projection(&binding, &replay)
                        .is_none(),
                    "cancellation at the final yield must discard the pending proof"
                );
                assert!(
                    store.history_metadata_bytes_for_test() == Some(control_before),
                    "cancelling the final yield must release every pending proof owner"
                );
                drop(request);
                assert!(
                    budget.used() == 0,
                    "cancelled wrapper must release transient owners"
                );
                store.expire_idle_at(
                    std::time::Instant::now() + std::time::Duration::from_secs(7200),
                )?;
                assert!(
                    store.stats()?.namespaces == 0,
                    "cancelled wrapper must not retain an active lease"
                );
                assert!(
                    store.history_metadata_bytes_for_test() == Some(global_before),
                    "namespace expiry must release the remaining optional controller charge"
                );
            }
        }
    }
    Ok(())
}

// Shared optional-ledger equality requires --test-threads=1.
#[tokio::test]
#[serial_test::serial]
async fn reasoning_delivered_history_sse_abort_or_failure_never_publishes() -> TestResult {
    use http_body_util::BodyExt;
    for active_tokens in [false, true] {
        for mode in 0..6 {
            let store = DlpTokenStore::new();
            let global_before = {
                let warm = store.admit("codex", Some("synthetic-sse-failure-warmup"), 1)?;
                let binding = warm.bind_actual_attempt(
                    "final-fixture-provider",
                    "openai_responses",
                    "https://synthetic.invalid/v1/responses",
                    Some("synthetic-account"),
                )?;
                let pending = warm
                    .capture_delivered_history(&binding, &synthetic_history_sse_response())
                    .ok_or("synthetic SSE failure global warmup unavailable")?;
                drop(pending);
                drop(warm);
                store.expire_idle_at(
                    std::time::Instant::now() + std::time::Duration::from_secs(7200),
                )?;
                assert!(
                    store.stats()?.namespaces == 0,
                    "global warmup must release its namespace"
                );
                store
                    .history_metadata_bytes_for_test()
                    .ok_or("synthetic SSE failure global accounting unavailable")?
            };
            let mut source = json!({"input":if active_tokens {"private-a"} else {"safe"}});
            let request = request_in_store(&source, &store)?;
            let budget = request.budget.clone();
            let attempt = request.prepare_attempt_for_proxy(
                "final-fixture-provider",
                "openai_responses",
                "https://synthetic.invalid/v1/responses",
                Some("synthetic-account"),
                &mut source,
                |prepared| Ok(prepared.clone()),
            )?;
            assert!(
                attempt.has_restoration_lease() == active_tokens,
                "failure fixture must cover the requested token-authority branch"
            );
            let binding = attempt.binding.clone();
            let response = synthetic_history_sse_response();
            let replay = json!({"input":response["output"]});
            let pending = request
                .base_lease
                .capture_delivered_history(&binding, &response)
                .ok_or("synthetic SSE failure accounting warmup unavailable")?;
            drop(pending);
            let history_before = store
                .history_metadata_bytes_for_test()
                .ok_or("synthetic SSE failure accounting unavailable")?;
            assert!(
                request
                    .base_lease
                    .history_projection(&binding, &replay)
                    .is_none(),
                "discarded warmup capture must not authorize undelivered SSE history"
            );
            let terminal = synthetic_history_sse_frame(
                "response.completed",
                &json!({"type":"response.completed","response":response}),
            );
            let mut chunks = Vec::new();
            if mode == 3 {
                let mut changed = response["output"][0].clone();
                changed["summary"][0]["text"] = json!("Synthetic mismatched thought");
                chunks.push(Ok(synthetic_history_sse_frame(
                    "response.output_item.done",
                    &json!({"type":"response.output_item.done","output_index":0,"item":changed}),
                )));
            }
            if mode == 5 {
                // A clean EOF without the original completion cannot publish history.
                chunks.push(Ok(synthetic_history_sse_frame(
                    "response.output_item.done",
                    &json!({"type":"response.output_item.done", "output_index":0,
                        "item":response["output"][0]}),
                )));
            } else {
                chunks.push(Ok(terminal));
            }
            if mode == 1 {
                chunks.push(Err(std::io::Error::other("Synthetic read failure")));
            } else if mode == 2 {
                chunks.push(Ok(Bytes::from_static(b"event: unfinished")));
            }
            let upstream = futures::stream::iter(chunks);
            let body = if mode == 0 {
                Body::from_stream(upstream.chain(futures::stream::pending()))
            } else {
                Body::from_stream(upstream)
            };
            let restored = attempt
                .restore_client_http_response(
                    Response::builder()
                        .header("content-type", "text/event-stream")
                        .body(body)?,
                    ArgumentNormalizationMode::Exact,
                    None,
                )
                .await;
            let mut body = restored.into_body();
            if mode == 0 {
                assert!(
                    tokio::time::timeout(std::time::Duration::from_millis(15), body.frame())
                        .await
                        .is_err(),
                    "terminal barrier must wait for clean EOF"
                );
                drop(body);
            } else if mode == 4 {
                let mut terminal_yielded = false;
                for _ in 0..16 {
                    let frame = body
                        .frame()
                        .await
                        .ok_or("synthetic terminal frame must exist")?
                        .map_err(|_| "synthetic terminal frame failed")?;
                    let data = frame
                        .data_ref()
                        .ok_or("synthetic terminal frame must be data")?;
                    let text = std::str::from_utf8(data)
                        .map_err(|_| "synthetic terminal frame must be UTF-8")?;
                    assert!(
                        !text.contains("event: response.failed"),
                        "terminal cancellation must reach successful local emission"
                    );
                    assert!(
                        request
                            .base_lease
                            .history_projection(&binding, &replay)
                            .is_none(),
                        "terminal yield alone must not publish optional proof"
                    );
                    if text.contains("event: response.completed") {
                        terminal_yielded = true;
                        break;
                    }
                }
                assert!(
                    terminal_yielded,
                    "cancellation must occur after the terminal yield"
                );
                assert!(
                    store.history_metadata_bytes_for_test() == Some(history_before + 256 * 1024),
                    "terminal cancellation must discard a fully charged pending response"
                );
                drop(body);
            } else {
                let bytes = body
                    .collect()
                    .await
                    .map_err(|_| "synthetic failure framing failed")?
                    .to_bytes();
                let text = std::str::from_utf8(&bytes)
                    .map_err(|_| "synthetic failure framing must be UTF-8")?;
                assert!(
                    text.contains("event: response.failed")
                        && !text.contains("event: response.completed"),
                    "failed SSE must withhold terminal completion"
                );
            }
            assert!(
                request
                    .base_lease
                    .history_projection(&binding, &replay)
                    .is_none(),
                "cancelled or failed SSE must not publish optional proof"
            );
            assert!(
                store.history_metadata_bytes_for_test() == Some(history_before),
                "cancelled or failed SSE must release every transient proof owner"
            );
            drop(request);
            assert!(
                budget.used() == 0,
                "cancelled or failed SSE must release every transient payload owner"
            );
            store
                .expire_idle_at(std::time::Instant::now() + std::time::Duration::from_secs(7200))?;
            assert!(
                store.stats()?.namespaces == 0,
                "cancelled or failed SSE must not retain an active lease"
            );
            assert!(
                store.history_metadata_bytes_for_test() == Some(global_before),
                "namespace expiry must release the entire failure fixture controller charge"
            );
        }
    }
    Ok(())
}

#[tokio::test]
#[serial_test::serial]
async fn reasoning_history_admission_candidate_preserves_mandatory_budget() -> TestResult {
    use super::super::{payload_json::test_support as json_probe, runtime};
    use http_body_util::BodyExt;
    use std::{cell::Cell, rc::Rc};

    let original = (*runtime::get_security_runtime()).clone();
    struct Reset(SecurityRuntimeConfig);
    impl Drop for Reset {
        fn drop(&mut self) {
            runtime::set_security_runtime_for_test(self.0.clone());
        }
    }
    let _reset = Reset(original);
    let mut policy = SecurityRuntimeConfig::default();
    policy.custom_rules.push(CompiledCustomRule {
        id: "synthetic-candidate-policy".into(),
        name: "Synthetic candidate policy".into(),
        rule_type: "regex".into(),
        pattern: "synthetic-protected".into(),
        regex: regex::Regex::new("synthetic-protected").ok(),
        risk_level: RiskLevel::High,
        action: AuditAction::RedactRestore,
        enabled: true,
    });
    runtime::set_security_runtime_for_test(policy);
    let revision = admit_security_runtime()?.restoration_policy_revision;
    let delivered = json!({"output":[{"id":"synthetic-budget-reasoning","type":"reasoning",
        "summary":[{"type":"summary_text","text":"s".repeat(64 * 1024)}]}]});
    let mut source = json!({"input":delivered["output"]});
    let measurement = PayloadBudget::with_limit(4 * 1024 * 1024);
    let measured = PayloadJson::try_from_value(&source, measurement.clone(), measurement.limit())?;
    let (measured_value, measured_owner) = measured.into_parts();
    let source_bytes = measured_owner.bytes();
    drop(measured_value);
    drop(measured_owner);
    assert_eq!(measurement.used(), 0);

    // Mandatory admission keeps the source while the audit carrier owns its copy.
    // A third concurrent tree belongs only to optional candidate discovery.
    let budget = PayloadBudget::with_limit(2 * source_bytes + 32 * 1024);
    let store = DlpTokenStore::new();
    let issuer =
        store.admit_published_policy("codex", Some("synthetic-candidate-budget"), revision)?;
    let binding = issuer.bind_actual_attempt(
        "synthetic-budget-provider",
        "openai_responses",
        "https://synthetic.invalid/v1/responses",
        Some("synthetic-budget-account"),
    )?;
    let pending = issuer
        .capture_delivered_history(&binding, &delivered)
        .ok_or("synthetic budget capture unavailable")?;
    assert!(issuer.publish_delivered_history(&binding, pending));
    let history_baseline = store.history_bytes_for_test()?;
    let gateway = RestorationGateway::with_test_store_and_rules(store.clone(), &[], budget.clone());
    let mut headers = HeaderMap::new();
    headers.insert(
        "session-id",
        HeaderValue::from_static("synthetic-candidate-budget"),
    );
    let builds = Rc::new(Cell::new(0usize));
    let observed = Rc::clone(&builds);
    let probe = json_probe::install(move |observation| {
        if observation.phase == json_probe::Phase::BeforeBuild {
            observed.set(observed.get() + 1);
        }
        json_probe::Action::Continue
    });
    let admission = admit_http_request(
        &gateway,
        &AppType::Codex,
        true,
        false,
        &mut source,
        &headers,
    )?;
    drop(probe);
    let admission = match admission {
        RestorationAdmission::Intercepted(response) => {
            let bytes = response
                .into_body()
                .collect()
                .await
                .map_err(|_| "synthetic refusal collection failed")?
                .to_bytes();
            let refusal: Value =
                serde_json::from_slice(&bytes).map_err(|_| "synthetic refusal decoding failed")?;
            let category = match refusal.pointer("/error/code").and_then(Value::as_str) {
                Some("dlp_capacity_exceeded") => "capacity",
                Some("dlp_context_unavailable") => "context",
                Some("dlp_redaction_failed") => "redaction",
                _ => "other",
            };
            panic!(
                "candidate admission refused: category={category}, builds={}",
                builds.get()
            );
        }
        admission => admission,
    };
    assert!(
        matches!(&admission, RestorationAdmission::Admitted(_)),
        "candidate discovery must preserve mandatory admission"
    );
    assert_eq!(
        builds.get(),
        3,
        "only refusal audit, retained source, and initial audit may build mandatory trees"
    );
    drop(admission);
    assert_eq!(
        budget.used(),
        0,
        "admission owners must release all mandatory charges"
    );
    assert_eq!(
        store.history_bytes_for_test()?,
        history_baseline,
        "candidate pins and permits must release their optional charges"
    );
    Ok(())
}

#[tokio::test]
#[serial_test::serial]
async fn reasoning_history_roundtrip_admission_and_native_attempt() -> TestResult {
    use super::super::runtime;
    use http_body_util::BodyExt;
    let original = (*runtime::get_security_runtime()).clone();
    struct Reset(SecurityRuntimeConfig);
    impl Drop for Reset {
        fn drop(&mut self) {
            runtime::set_security_runtime_for_test(self.0.clone());
        }
    }
    let _reset = Reset(original);
    let mut policy = SecurityRuntimeConfig::default();
    policy.custom_rules.push(CompiledCustomRule {
        id: "synthetic-history-policy".into(),
        name: "Synthetic history policy".into(),
        rule_type: "regex".into(),
        pattern: "synthetic-protected".into(),
        regex: regex::Regex::new("synthetic-protected").ok(),
        risk_level: RiskLevel::High,
        action: AuditAction::RedactRestore,
        enabled: true,
    });
    runtime::set_security_runtime_for_test(policy);
    let gateway = RestorationGateway::with_rules(&[], 32 * 1024 * 1024);
    let mut headers = HeaderMap::new();
    headers.insert(
        "session-id",
        HeaderValue::from_static("synthetic-history-roundtrip"),
    );
    for reasoning in [true, false] {
        let mut source = json!({"input":"synthetic-protected"});
        let admitted = match admit_http_request(
            &gateway,
            &AppType::Codex,
            true,
            false,
            &mut source,
            &headers,
        )? {
            RestorationAdmission::Admitted(request) => request,
            _ => panic!("synthetic initial request must be admitted"),
        };
        let attempt = admitted.prepare_attempt_for_proxy(
            "synthetic-history-provider",
            "openai_responses",
            "https://synthetic.invalid/v1/responses",
            Some("synthetic-history-principal"),
            &mut source,
            |body| Ok(body.clone()),
        )?;
        let issued = source["input"]
            .as_str()
            .ok_or("synthetic signed input missing")?;
        assert!(
            attempt.lease.resolve_if_authorized(issued)?.is_some(),
            "roundtrip fixture must issue a current-lease reference"
        );
        let literal_reference = format!("rredact-custom-{}-ai", "a".repeat(20));
        let item = if reasoning {
            json!({"id":"synthetic-history-reasoning", "type":"reasoning", "summary":[
                {"type":"summary_text", "text":"synthetic rredact-custom- literal"}
            ], "content":[{"type":"reasoning_text", "text":format!("synthetic reasoning {literal_reference} boundary")} ]})
        } else {
            json!({"id":"synthetic-history-message", "type":"message", "role":"assistant", "status":"completed",
                "content":[{"type":"output_text", "text":"synthetic rredact-custom- literal", "annotations":[]}]})
        };
        let raw = json!({"id":format!("synthetic-history-response-{}", usize::from(reasoning)), "object":"response", "status":"completed", "output":[item,
            {"id":"synthetic-history-restored", "type":"message", "role":"assistant", "status":"completed",
                "content":[{"type":"output_text", "text":issued, "annotations":[]}]}]});
        let response = attempt
            .restore_client_http_response(
                Response::builder()
                    .header("content-type", "application/json")
                    .body(Body::from(raw.to_string()))?,
                ArgumentNormalizationMode::Exact,
                None,
            )
            .await;
        assert!(
            response.status() == StatusCode::OK,
            "synthetic response delivery must succeed"
        );
        let bytes = response.into_body().collect().await?.to_bytes();
        let delivered: Value =
            serde_json::from_slice(&bytes).map_err(|_| "synthetic delivery must encode JSON")?;
        let mut replay =
            json!({"previous_response_id":delivered["id"], "input":delivered["output"]});
        let request = match admit_http_request(
            &gateway,
            &AppType::Codex,
            true,
            false,
            &mut replay,
            &headers,
        )? {
            RestorationAdmission::Admitted(request) => request,
            _ => panic!("exact delivered history must pass candidate admission"),
        };
        let result = request.prepare_attempt_for_proxy(
            "synthetic-history-provider",
            "openai_responses",
            "https://synthetic.invalid/v1/responses",
            Some("synthetic-history-principal"),
            &mut replay,
            |body| Ok(body.clone()),
        );
        assert!(
            result.is_ok(),
            "exact delivered history must pass native final inspection"
        );
        let attempt = result.map_err(|_| "synthetic exact history preparation must succeed")?;
        assert!(
            attempt.validate_final_send_history(&replay).is_ok(),
            "exact final tree must retain proof"
        );
        let mut changed = replay.clone();
        let pointer = if reasoning {
            "/input/0/summary/0/text"
        } else {
            "/input/0/content/0/text"
        };
        *changed
            .pointer_mut(pointer)
            .ok_or("synthetic history path missing")? =
            json!("changed synthetic-protected rredact-custom-");
        assert!(
            attempt.validate_final_send_history(&changed).is_err(),
            "late text mutation must lose proof"
        );
        let mut uncompleted = json!({"previous_response_id":"synthetic-uncompleted-response", "input":delivered["output"]});
        let uncompleted_request = match admit_http_request(
            &gateway,
            &AppType::Codex,
            true,
            false,
            &mut uncompleted,
            &headers,
        )? {
            RestorationAdmission::Admitted(request) => request,
            _ => return Err("completion dependency must defer its actual binding check".into()),
        };
        assert!(
            matches!(
                uncompleted_request.prepare_attempt_for_proxy(
                    "synthetic-history-provider",
                    "openai_responses",
                    "https://synthetic.invalid/v1/responses",
                    Some("synthetic-history-principal"),
                    &mut uncompleted,
                    |body| Ok(body.clone())
                ),
                Err(crate::proxy::error::ProxyError::DlpForward(
                    DlpForwardError::ContextUnavailable
                ))
            ),
            "exact delivered-history proof must not authorize an uncompleted response dependency"
        );
        for (provider, target, principal) in [
            (
                "synthetic-other-provider",
                "https://synthetic.invalid/v1/responses",
                "synthetic-history-principal",
            ),
            (
                "synthetic-history-provider",
                "https://synthetic.invalid/v1/responses?other=1",
                "synthetic-history-principal",
            ),
            (
                "synthetic-history-provider",
                "https://synthetic.invalid/v1/responses",
                "synthetic-other-principal",
            ),
        ] {
            let mut candidate =
                json!({"previous_response_id":delivered["id"], "input":delivered["output"]});
            let request = match admit_http_request(
                &gateway,
                &AppType::Codex,
                true,
                false,
                &mut candidate,
                &headers,
            )? {
                RestorationAdmission::Admitted(request) => request,
                _ => panic!("private candidate must defer actual-binding decision"),
            };
            assert!(
                request
                    .prepare_attempt_for_proxy(
                        provider,
                        "openai_responses",
                        target,
                        Some(principal),
                        &mut candidate,
                        |body| Ok(body.clone())
                    )
                    .is_err(),
                "different actual authority must refuse history"
            );
        }
        for mutation in 0..4 {
            let mut candidate =
                json!({"previous_response_id":delivered["id"], "input":delivered["output"]});
            let request = match admit_http_request(
                &gateway,
                &AppType::Codex,
                true,
                false,
                &mut candidate,
                &headers,
            )? {
                RestorationAdmission::Admitted(request) => request,
                _ => return Err("synthetic mutation control must be admitted".into()),
            };
            let result = request.prepare_attempt_for_proxy(
                "synthetic-history-provider", "openai_responses", "https://synthetic.invalid/v1/responses",
                Some("synthetic-history-principal"), &mut candidate, |body| {
                    let mut changed = body.clone();
                    match mutation {
                        0 => changed["input"][0][if reasoning { "summary" } else { "content" }][0]["text"] =
                            json!("changed synthetic-protected rredact-custom-"),
                        1 => changed["input"][0]["metadata"] =
                            json!({"synthetic":"synthetic-protected rredact-custom-"}),
                        2 => changed["input"].as_array_mut().ok_or(
                            crate::proxy::error::ProxyError::DlpForward(DlpForwardError::RedactionFailed))?
                            .push(json!({"type":"function_call", "call_id":"synthetic-strict-call",
                                "name":"synthetic", "arguments":"{\"value\":\"synthetic-protected rredact-custom-\"}"})),
                        _ => changed["input"][0]["rredact-custom-"] = json!("synthetic-protected"),
                    }
                    Ok(changed)
                });
            assert!(
                matches!(
                    result,
                    Err(crate::proxy::error::ProxyError::DlpForward(
                        DlpForwardError::RedactionFailed
                    ))
                ),
                "post-prepare protective plaintext must not bypass exact history inspection"
            );
        }
        let mut literal_sibling =
            json!({"previous_response_id":delivered["id"], "input":delivered["output"]});
        literal_sibling["input"]
            .as_array_mut()
            .ok_or("synthetic input missing")?
            .push(json!({"role":"user", "content":literal_reference}));
        let literal_request = match admit_http_request(
            &gateway,
            &AppType::Codex,
            true,
            false,
            &mut literal_sibling,
            &headers,
        )? {
            RestorationAdmission::Admitted(request) => request,
            _ => return Err("ordinary literal sibling must be admitted".into()),
        };
        let literal_attempt = literal_request.prepare_attempt_for_proxy(
            "synthetic-history-provider",
            "openai_responses",
            "https://synthetic.invalid/v1/responses",
            Some("synthetic-history-principal"),
            &mut literal_sibling,
            |body| Ok(body.clone()),
        )?;
        assert!(
            literal_sibling["input"]
                .as_array()
                .and_then(|items| items.last())
                .and_then(|item| item["content"].as_str())
                == Some(literal_reference.as_str()),
            "unproven literal siblings must retain every byte"
        );
        assert!(
            literal_attempt
                .lease
                .resolve_if_authorized(&literal_reference)?
                .is_none(),
            "unproven literal siblings must not acquire authority"
        );
    }
    Ok(())
}

#[tokio::test]
#[serial_test::serial]
async fn reasoning_history_http_output_session_isolation() -> TestResult {
    check_history_http_isolation(false, HistoryIsolationBoundary::Session).await
}

#[tokio::test]
#[serial_test::serial]
async fn reasoning_history_http_reasoning_session_isolation() -> TestResult {
    check_history_http_isolation(true, HistoryIsolationBoundary::Session).await
}

#[tokio::test]
#[serial_test::serial]
async fn reasoning_history_http_output_policy_admission_isolation() -> TestResult {
    check_history_http_isolation(false, HistoryIsolationBoundary::PolicyAdmission).await
}

#[tokio::test]
#[serial_test::serial]
async fn reasoning_history_http_reasoning_policy_admission_isolation() -> TestResult {
    check_history_http_isolation(true, HistoryIsolationBoundary::PolicyAdmission).await
}

#[tokio::test]
#[serial_test::serial]
async fn reasoning_history_http_output_prepared_policy_revocation() -> TestResult {
    check_history_http_isolation(false, HistoryIsolationBoundary::PreparedPolicy).await
}

#[tokio::test]
#[serial_test::serial]
async fn reasoning_history_http_reasoning_prepared_policy_revocation() -> TestResult {
    check_history_http_isolation(true, HistoryIsolationBoundary::PreparedPolicy).await
}

#[tokio::test]
#[serial_test::serial]
async fn reasoning_history_http_output_prepared_policy_gate_closed() -> TestResult {
    check_history_http_isolation(false, HistoryIsolationBoundary::PreparedPolicyGateClosed).await
}

#[tokio::test]
#[serial_test::serial]
async fn reasoning_history_http_reasoning_prepared_policy_gate_closed() -> TestResult {
    check_history_http_isolation(true, HistoryIsolationBoundary::PreparedPolicyGateClosed).await
}

enum HistoryIsolationBoundary {
    Session,
    PolicyAdmission,
    PreparedPolicy,
    PreparedPolicyGateClosed,
}

async fn check_history_http_isolation(
    reasoning: bool,
    boundary: HistoryIsolationBoundary,
) -> TestResult {
    use super::super::runtime;
    use http_body_util::BodyExt;

    struct Reset(SecurityRuntimeConfig);
    impl Drop for Reset {
        fn drop(&mut self) {
            runtime::set_security_runtime_for_test(self.0.clone());
        }
    }
    let _reset = Reset((*runtime::get_security_runtime()).clone());
    let mut policy = SecurityRuntimeConfig::default();
    policy.custom_rules.push(CompiledCustomRule {
        id: "synthetic-isolation-rule".into(),
        name: "Synthetic isolation policy".into(),
        rule_type: "regex".into(),
        pattern: "synthetic-isolation-protected".into(),
        regex: Some(regex::Regex::new("synthetic-isolation-protected")?),
        risk_level: RiskLevel::High,
        action: AuditAction::RedactRestore,
        enabled: true,
    });
    runtime::set_security_runtime_for_test(policy.clone());
    let revision = admit_security_runtime()?.restoration_policy_revision;
    let gateway = RestorationGateway::with_rules(&[], 32 * 1024 * 1024);
    let mut headers = HeaderMap::new();
    headers.insert(
        "session-id",
        HeaderValue::from_static("synthetic-history-isolation"),
    );
    let mut source = json!({"input": "synthetic-isolation-protected"});
    let admitted = match admit_http_request(
        &gateway,
        &AppType::Codex,
        true,
        false,
        &mut source,
        &headers,
    )? {
        RestorationAdmission::Admitted(request) => request,
        _ => return Err("synthetic initial isolation request must be admitted".into()),
    };
    let attempt = admitted.prepare_attempt_for_proxy(
        "synthetic-isolation-provider",
        "openai_responses",
        "https://synthetic.invalid/v1/responses",
        Some("synthetic-isolation-principal"),
        &mut source,
        |body| Ok(body.clone()),
    )?;
    let issued = source["input"]
        .as_str()
        .ok_or("synthetic isolation signed input missing")?;
    assert!(
        attempt.lease.resolve_if_authorized(issued)?.is_some(),
        "isolation fixture must issue a current-lease reference"
    );
    let item = if reasoning {
        json!({"id": "synthetic-isolation-reasoning", "type": "reasoning", "summary": [
            {"type": "summary_text", "text": "synthetic reasoning rredact-custom-"}
        ]})
    } else {
        json!({"id": "synthetic-isolation-output", "type": "message", "role": "assistant",
        "status": "completed", "content": [
            {"type": "output_text", "text": "synthetic output rredact-custom-", "annotations": []}
        ]})
    };
    let raw = json!({"id": "synthetic-isolation-response", "object": "response",
        "status": "completed", "output": [item,
            {"id":"synthetic-isolation-restored", "type":"message", "role":"assistant", "status":"completed",
                "content":[{"type":"output_text", "text":issued, "annotations":[]}]}]});
    let response = attempt
        .restore_client_http_response(
            Response::builder()
                .header("content-type", "application/json")
                .body(Body::from(raw.to_string()))?,
            ArgumentNormalizationMode::Exact,
            None,
        )
        .await;
    assert!(
        response.status() == StatusCode::OK,
        "synthetic isolation delivery must succeed"
    );
    let bytes = response.into_body().collect().await?.to_bytes();
    let delivered: Value = serde_json::from_slice(&bytes)
        .map_err(|_| "synthetic isolation delivery must encode JSON")?;
    let mut replay = json!({"previous_response_id":delivered["id"], "input": delivered["output"]});
    let replay_request = match admit_http_request(
        &gateway,
        &AppType::Codex,
        true,
        false,
        &mut replay,
        &headers,
    )? {
        RestorationAdmission::Admitted(request) => request,
        _ => return Err("same-session synthetic history must be admitted".into()),
    };
    let ready = replay_request.prepare_attempt_for_proxy(
        "synthetic-isolation-provider",
        "openai_responses",
        "https://synthetic.invalid/v1/responses",
        Some("synthetic-isolation-principal"),
        &mut replay,
        |body| Ok(body.clone()),
    )?;
    let target = super::super::history_provenance::DlpHistoryTarget::ResponsesLeaf {
        input_index: 0,
        field: if reasoning {
            super::super::history_provenance::DlpHistoryLeafField::ReasoningSummary
        } else {
            super::super::history_provenance::DlpHistoryLeafField::OutputText
        },
        part_index: 0,
    };
    assert!(
        ready.history_mapping.as_ref().is_some_and(|mapping| {
            mapping.validates_target(&ready.lease, &ready.binding, &replay, target)
        }),
        "synthetic isolation fixture must carry a live exact delivered-leaf mapping"
    );
    assert!(
        ready.validate_final_send_history(&replay).is_ok(),
        "same-policy exact history must pass the final proof check"
    );
    assert!(
        ready.encode_outbound_body(&replay).is_ok(),
        "same-policy exact history must pass outbound encoding"
    );

    if matches!(boundary, HistoryIsolationBoundary::Session) {
        let mut other_session = HeaderMap::new();
        other_session.insert(
            "session-id",
            HeaderValue::from_static("synthetic-other-history-session"),
        );
        let no_session = HeaderMap::new();
        for candidate_headers in [&other_session, &no_session] {
            let mut candidate =
                json!({"previous_response_id":delivered["id"], "input": delivered["output"]});
            let admission = admit_http_request(
                &gateway,
                &AppType::Codex,
                true,
                false,
                &mut candidate,
                candidate_headers,
            )?;
            assert_bound_history_context_refusal(admission, &mut candidate).await?;
        }
        return Ok(());
    }

    if matches!(boundary, HistoryIsolationBoundary::PreparedPolicyGateClosed) {
        let db = crate::database::Database::memory()?;
        let mut during_update = None;
        let failed_update = runtime::update_security_runtime(&db, |_| {
            // Do not assert while the policy writer is held: unwinding must not
            // make the runtime reset wait on the same writer lock.
            during_update = Some((
                matches!(admit_security_runtime(), Err(TokenError::Unavailable)),
                matches!(
                    ready.validate_final_send_history(&replay),
                    Err(DlpForwardError::ContextUnavailable)
                ),
                matches!(
                    ready.encode_outbound_body(&replay),
                    Err(DlpForwardError::ContextUnavailable)
                ),
            ));
            Err::<(), _>(crate::error::AppError::Message(
                "synthetic history policy save failure".into(),
            ))
        });
        let after_failed_update = (
            matches!(admit_security_runtime(), Err(TokenError::Unavailable)),
            matches!(
                ready.validate_final_send_history(&replay),
                Err(DlpForwardError::ContextUnavailable)
            ),
            matches!(
                ready.encode_outbound_body(&replay),
                Err(DlpForwardError::ContextUnavailable)
            ),
        );
        assert!(failed_update.is_err(), "synthetic policy update must fail");
        let (during_closed, during_proof_refused, during_encoding_refused) =
            during_update.ok_or("synthetic update must observe the closed policy gate")?;
        let (after_closed, after_proof_refused, after_encoding_refused) = after_failed_update;
        assert!(
            during_closed && after_closed,
            "policy admission must remain closed during and after an uncertain save"
        );

        // No policy is published by the failed update. Republishing the exact
        // policy must preserve its revision, independently of proof eligibility.
        runtime::set_security_runtime_for_test(policy);
        assert!(
            admit_security_runtime()?.restoration_policy_revision == revision,
            "synthetic gate closure must not depend on a changed policy revision"
        );
        assert!(
            during_proof_refused
                && during_encoding_refused
                && after_proof_refused
                && after_encoding_refused,
            "closed policy gate must revoke prepared history at proof and encoding boundaries"
        );
        return Ok(());
    }

    let mut changed_policy = policy;
    changed_policy.custom_rules[0].pattern = "synthetic-changed-isolation-protected".into();
    changed_policy.custom_rules[0].regex =
        Some(regex::Regex::new("synthetic-changed-isolation-protected")?);
    runtime::set_security_runtime_for_test(changed_policy);
    assert!(
        admit_security_runtime()?.restoration_policy_revision > revision,
        "synthetic policy change must advance restoration authority"
    );
    if matches!(boundary, HistoryIsolationBoundary::PolicyAdmission) {
        let mut candidate =
            json!({"previous_response_id":delivered["id"], "input": delivered["output"]});
        let admission = admit_http_request(
            &gateway,
            &AppType::Codex,
            true,
            false,
            &mut candidate,
            &headers,
        )?;
        assert_bound_history_context_refusal(admission, &mut candidate).await?;
    } else {
        // Revoke proof eligibility, not the request's captured detection matcher.
        let proof_revoked = matches!(
            ready.validate_final_send_history(&replay),
            Err(DlpForwardError::ContextUnavailable)
        );
        let encoding_refused = matches!(
            ready.encode_outbound_body(&replay),
            Err(DlpForwardError::ContextUnavailable)
        );
        assert!(
            proof_revoked && encoding_refused,
            "policy invalidation must revoke prepared history at proof and encoding boundaries"
        );
    }
    Ok(())
}

async fn assert_bound_history_context_refusal(
    admission: RestorationAdmission,
    body: &mut Value,
) -> TestResult {
    let response = match admission {
        RestorationAdmission::Intercepted(response) => response,
        RestorationAdmission::Admitted(request) => {
            let error = match request.prepare_attempt_for_proxy(
                "synthetic-isolation-provider",
                "openai_responses",
                "https://synthetic.invalid/v1/responses",
                Some("synthetic-isolation-principal"),
                body,
                |body| Ok(body.clone()),
            ) {
                Err(crate::proxy::error::ProxyError::DlpForward(error)) => error,
                _ => {
                    return Err("unproven real completion dependency must refuse at binding".into())
                }
            };
            error.into_codex_response()
        }
        RestorationAdmission::Passthrough => {
            return Err("real history dependency cannot pass through".into())
        }
    };
    assert_history_context_refusal_response(response).await
}

async fn assert_history_context_refusal_response(response: Response) -> TestResult {
    assert!(
        response.status() == StatusCode::BAD_REQUEST,
        "history isolation must produce the fixed client refusal status"
    );
    let bytes = axum::body::to_bytes(response.into_body(), 4096)
        .await
        .map_err(|_| "synthetic isolation refusal body must fit the fixed collection bound")?;
    let payload: Value = serde_json::from_slice(&bytes)
        .map_err(|_| "synthetic isolation refusal must encode JSON")?;
    let expected = json!({"error": {
        "message": DlpForwardError::ContextUnavailable.message(),
        "type": "invalid_request_error",
        "code": "dlp_context_unavailable",
        "param": null
    }});
    assert!(
        payload == expected,
        "history isolation must return the exact fixed client refusal envelope without extra fields"
    );
    Ok(())
}

#[test]
fn reasoning_history_roundtrip_chat_conversion_keeps_private_source_chain() -> TestResult {
    let delivered = json!({"output":[
        {"id":"synthetic-chat-reasoning", "type":"reasoning", "summary":[
            {"type":"summary_text", "text":"synthetic thinking rredact-custom-"}]},
        {"id":"synthetic-chat-output", "type":"message", "role":"assistant", "content":[
            {"type":"output_text", "text":"synthetic output rredact-custom-", "annotations":[]}]}
    ]});
    let mut body = json!({"input":delivered["output"]});
    let request = request(&body)?;
    let issuer = request.base_lease.fork_attempt()?;
    let binding = issuer.bind_actual_attempt(
        "final-fixture-provider",
        "openai_chat",
        "https://synthetic.invalid/v1/responses",
        Some("synthetic-account"),
    )?;
    let pending = issuer
        .capture_delivered_history(&binding, &delivered)
        .ok_or("synthetic Chat proof capture must succeed")?;
    assert!(
        issuer.publish_delivered_history(&binding, pending),
        "synthetic Chat proof must publish"
    );
    let result = request.prepare_owned_attempt_with_history_for_proxy(None, "final-fixture-provider", "openai_chat",
        "https://synthetic.invalid/v1/responses", Some("synthetic-account"), &mut body,
        |body, context| {
            let projection = context.projection(&body);
            crate::proxy::providers::transform_codex_chat::responses_to_chat_completions_with_reasoning_and_history(&body, None, projection.as_ref())
        });
    assert!(
        result.is_ok(),
        "exact delivered typed history must survive the actual Chat conversion chain"
    );
    Ok(())
}

fn change_synthetic_chat_target(body: &mut Value, mode: usize) {
    match mode {
        0 => {
            body["messages"][0]["tool_calls"] = json!([{"id":"synthetic-other-call","type":"function",
            "function":{"name":"synthetic_other_tool","arguments":"{}"}}])
        }
        1 => body["messages"][0]["id"] = json!("synthetic-other-item"),
        2 => body["messages"][0]["name"] = json!("synthetic_other_owner"),
        _ => body["messages"][0]["metadata"] = json!({"synthetic_owner":"other"}),
    }
}

fn assert_chat_target_identity_boundary(late: bool) -> TestResult {
    let delivered = json!({"output":[
        {"id":"synthetic-target-reasoning","type":"reasoning","summary":[
            {"type":"summary_text","text":"private-a rredact-custom-"}]},
        {"id":"synthetic-target-output","type":"message","role":"assistant","content":[
            {"type":"output_text","text":"synthetic unchanged output","annotations":[]}]}
    ]});
    let source = json!({"input":delivered["output"]});
    let request = request(&source)?;
    let issuer = request.base_lease.fork_attempt()?;
    let binding = issuer.bind_actual_attempt(
        "final-fixture-provider",
        "openai_chat",
        "https://synthetic.invalid/v1/responses",
        Some("synthetic-account"),
    )?;
    let pending = issuer
        .capture_delivered_history(&binding, &delivered)
        .ok_or("synthetic target capture unavailable")?;
    assert!(issuer.publish_delivered_history(&binding, pending));
    for mode in 0..4 {
        let mut body = source.clone();
        let result = request.prepare_owned_attempt_with_history_for_proxy(None,
            "final-fixture-provider", "openai_chat", "https://synthetic.invalid/v1/responses",
            Some("synthetic-account"), &mut body, |body, context| {
                let projection = context.projection(&body);
                let (mut converted, mapping) = crate::proxy::providers::transform_codex_chat::responses_to_chat_completions_with_reasoning_and_history(
                    &body, None, projection.as_ref())?;
                if !late { change_synthetic_chat_target(&mut converted, mode); }
                Ok((converted, mapping))
            });
        if late {
            let (attempt, _owner) =
                result.map_err(|_| "synthetic unchanged target must prepare")?;
            assert!(attempt.validate_final_send_history(&body).is_ok());
            change_synthetic_chat_target(&mut body, mode);
            assert!(
                attempt.validate_final_send_history(&body).is_err(),
                "unchanged text cannot authorize a different final typed target"
            );
        } else {
            assert!(
                result.is_err(),
                "post-conversion target reassignment must not inherit delivered reasoning proof"
            );
        }
    }
    Ok(())
}

#[test]
fn reasoning_history_chat_target_identity_rejects_post_conversion_override() -> TestResult {
    assert_chat_target_identity_boundary(false)
}

#[test]
fn reasoning_history_chat_target_identity_rejects_late_send_reassignment() -> TestResult {
    assert_chat_target_identity_boundary(true)
}

mod history_client_diagnostics {
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    static FORBIDDEN: AtomicBool = AtomicBool::new(false);
    static CONTROL: AtomicBool = AtomicBool::new(false);
    static CHECKPOINTS: AtomicUsize = AtomicUsize::new(0);
    struct Logger;
    impl log::Log for Logger {
        fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
            metadata.target().contains("::proxy::security")
        }
        fn log(&self, record: &log::Record<'_>) {
            if !self.enabled(record.metadata()) {
                return;
            }
            let exact =
                super::super::super::reversible_integration_tests::history_client_diagnostic_exact;
            if record.target() == "cc_switch_lib::proxy::security::synthetic_history_client_control"
                && exact(*record.args(), "synthetic history client observer control")
            {
                CONTROL.store(true, Ordering::SeqCst);
            } else if record.target() == "cc_switch_lib::proxy::security::restore_diagnostics"
                && exact(*record.args(), "[DLP] context failure code=dlp_context_unavailable checkpoint=final_validation")
            {
                CHECKPOINTS.fetch_add(1, Ordering::SeqCst);
            } else {
                FORBIDDEN.store(true, Ordering::SeqCst);
            }
        }
        fn flush(&self) {}
    }
    static LOGGER: Logger = Logger;
    pub(super) async fn parent(
        child: &str,
        test: &str,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        super::super::super::reversible_integration_tests::history_client_diagnostic_parent(
            child, test,
        )
        .await
    }
    pub(super) fn install() -> super::TestResult {
        log::set_logger(&LOGGER).map_err(|_| "synthetic history client logger unavailable")?;
        log::set_max_level(log::LevelFilter::Trace);
        log::warn!(target: "cc_switch_lib::proxy::security::synthetic_history_client_control",
            "synthetic history client observer control");
        assert!(
            CONTROL.load(Ordering::SeqCst),
            "synthetic history client logger must receive its control"
        );
        Ok(())
    }
    pub(super) fn finish(expected: usize) -> ! {
        if FORBIDDEN.load(Ordering::SeqCst)
            || CHECKPOINTS.load(Ordering::SeqCst) != expected
            || !CONTROL.load(Ordering::SeqCst)
        {
            std::process::exit(86);
        }
        std::process::exit(87);
    }
}

fn history_client_refusal_chat_fixture(
) -> Result<(DlpHttpRequest, Value), Box<dyn std::error::Error>> {
    let delivered = json!({"output":[
        {"id":"synthetic-refusal-reasoning", "type":"reasoning", "summary":[
            {"type":"summary_text", "text":"synthetic reasoning rredact-custom-"}]},
        {"id":"synthetic-refusal-output", "type":"message", "role":"assistant", "content":[
            {"type":"output_text", "text":"synthetic output rredact-custom-", "annotations":[]}]}
    ]});
    let source = json!({"input":delivered["output"]});
    let request = request(&source)?;
    let issuer = request.base_lease.fork_attempt()?;
    let binding = issuer.bind_actual_attempt(
        "final-fixture-provider",
        "openai_chat",
        "https://synthetic.invalid/v1/responses",
        Some("synthetic-account"),
    )?;
    let pending = issuer
        .capture_delivered_history(&binding, &delivered)
        .ok_or("synthetic client refusal fixture must capture delivered history")?;
    assert!(
        issuer.publish_delivered_history(&binding, pending),
        "synthetic client refusal fixture must publish delivered history"
    );
    Ok((request, source))
}

#[tokio::test]
async fn reasoning_history_chat_mapping_failures_return_fixed_client_refusal() -> TestResult {
    const CHILD: &str = "VARYAI_SYNTHETIC_HISTORY_MAPPING_CLIENT_DIAGNOSTIC";
    const TEST: &str = "proxy::security::http_restoration::final_validation_tests::reasoning_history_chat_mapping_failures_return_fixed_client_refusal";
    if history_client_diagnostics::parent(CHILD, TEST).await? {
        return Ok(());
    }
    history_client_diagnostics::install()?;
    use super::super::history_provenance::DlpHistoryTarget;

    let (request, source) = history_client_refusal_chat_fixture()?;
    for mode in 0..8 {
        let mut body = source.clone();
        let result = request.prepare_owned_attempt_with_history_for_proxy(
            None,
            "final-fixture-provider",
            "openai_chat",
            "https://synthetic.invalid/v1/responses",
            Some("synthetic-account"),
            &mut body,
            |body, context| {
                let projection = context.projection(&body);
                let (mut converted, mut mapping) = crate::proxy::providers::transform_codex_chat::responses_to_chat_completions_with_reasoning_and_history(
                    &body, None, projection.as_ref())?;
                match mode {
                    0 => {}
                    1 => {
                        assert!(mapping.is_some(), "synthetic wrong-mapping fixture needs proof");
                        if let Some(mapping) = mapping.as_mut() {
                            mapping.remap_chat_targets(|index| index.checked_add(1));
                        }
                    }
                    2 => converted["messages"][0]["reasoning_content"] =
                        json!("synthetic changed reasoning rredact-custom-"),
                    3 => converted["messages"][0]["content"] =
                        json!("synthetic changed output rredact-custom-"),
                    _ => change_synthetic_chat_target(&mut converted, mode - 4),
                }
                Ok((converted, mapping))
            },
        );
        let (attempt, _owner) = result
            .map_err(|_| "synthetic unproven literals must prepare without acquiring authority")?;
        assert!(
            !attempt.has_active_tokens(),
            "synthetic literal mapping must not grant token authority"
        );
        if mode == 0 {
            for target in [
                DlpHistoryTarget::ChatReasoning { message_index: 0 },
                DlpHistoryTarget::ChatContent { message_index: 0 },
            ] {
                assert!(
                    attempt.history_mapping.as_ref().is_some_and(|mapping| {
                        mapping.validates_target(&attempt.lease, &attempt.binding, &body, target)
                    }),
                    "synthetic refusal fixture must prove both reasoning and ordinary output"
                );
            }
            assert!(
                attempt.validate_final_send_history(&body).is_ok(),
                "synthetic exact history must pass the real final-send check"
            );
            continue;
        }
        // Ordinary literals may prepare, but a stale converter mapping cannot authorize a send.
        for checked in [
            attempt.validate_final_send_history(&body),
            attempt.encode_outbound_body(&body).map(|_| ()),
        ] {
            let error = match checked {
                Err(error) => error,
                Ok(()) => {
                    return Err("synthetic stale mapping must refuse at the send boundary".into())
                }
            };
            assert_history_context_refusal_response(error.into_codex_response()).await?;
        }
    }
    drop(request);
    drop(source);
    history_client_diagnostics::finish(0)
}

#[tokio::test]
async fn reasoning_history_chat_late_reassignments_return_fixed_client_refusal() -> TestResult {
    const CHILD: &str = "VARYAI_SYNTHETIC_HISTORY_LATE_CLIENT_DIAGNOSTIC";
    const TEST: &str = "proxy::security::http_restoration::final_validation_tests::reasoning_history_chat_late_reassignments_return_fixed_client_refusal";
    if history_client_diagnostics::parent(CHILD, TEST).await? {
        return Ok(());
    }
    history_client_diagnostics::install()?;
    let (request, mut body) = history_client_refusal_chat_fixture()?;
    let (attempt, _owner) = request
        .prepare_owned_attempt_with_history_for_proxy(
            None,
            "final-fixture-provider",
            "openai_chat",
            "https://synthetic.invalid/v1/responses",
            Some("synthetic-account"),
            &mut body,
            |body, context| {
                let projection = context.projection(&body);
                crate::proxy::providers::transform_codex_chat::responses_to_chat_completions_with_reasoning_and_history(
                    &body, None, projection.as_ref())
            },
        )
        .map_err(|_| "synthetic unchanged history must prepare before late reassignment")?;
    assert!(
        attempt.validate_final_send_history(&body).is_ok(),
        "synthetic unchanged history must pass before late reassignment"
    );
    for mode in 0..6 {
        let mut changed = body.clone();
        match mode {
            0 => {
                changed["messages"][0]["reasoning_content"] =
                    json!("synthetic safe changed reasoning")
            }
            1 => changed["messages"][0]["content"] = json!("synthetic safe changed output"),
            _ => change_synthetic_chat_target(&mut changed, mode - 2),
        }
        for result in [
            attempt.validate_final_send_history(&changed),
            attempt.encode_outbound_body(&changed).map(|_| ()),
        ] {
            let error = match result {
                Err(error) => error,
                Ok(()) => {
                    return Err(
                        "synthetic late reassignment must produce a real DLP refusal".into(),
                    )
                }
            };
            assert_history_context_refusal_response(error.into_app_response(&AppType::Codex))
                .await?;
        }
    }
    drop(attempt);
    drop(_owner);
    drop(request);
    drop(body);
    history_client_diagnostics::finish(0)
}

#[test]
fn reasoning_history_refusal_audit_uses_bound_eligible_leaves() -> TestResult {
    let delivered = json!({"output":[{"id":"synthetic-audit-reasoning", "type":"reasoning",
        "summary":[{"type":"summary_text","text":"private-a"}]}]});
    let source = json!({"input":delivered["output"]});
    let request = request(&source)?;
    let issuer = request.base_lease.fork_attempt()?;
    let binding = issuer.bind_actual_attempt(
        "final-fixture-provider",
        "openai_responses",
        "https://synthetic.invalid/v1/responses",
        Some("synthetic-account"),
    )?;
    let pending = issuer
        .capture_delivered_history(&binding, &delivered)
        .ok_or("synthetic audit capture unavailable")?;
    assert!(
        issuer.publish_delivered_history(&binding, pending),
        "synthetic audit publication failed"
    );
    let observe = request.audit_observer_for_test();
    let mut enriched = source;
    enriched["input"]
        .as_array_mut()
        .ok_or("synthetic audit input missing")?
        .push(
            json!({"type":"function_call", "call_id":"synthetic-audit-malformed",
            "name":"synthetic", "arguments":"{broken"}),
        );
    assert!(
        request
            .prepare_attempt_for_proxy(
                "final-fixture-provider",
                "openai_responses",
                "https://synthetic.invalid/v1/responses",
                Some("synthetic-account"),
                &mut enriched,
                |body| Ok(body.clone())
            )
            .is_err(),
        "unproven malformed sibling must refuse"
    );
    assert!(
        observe().is_empty(),
        "refusal audit must not detect proven reasoning"
    );
    Ok(())
}

#[test]
fn reasoning_history_chat_mapping_overrides_strict_fallback_and_optional_miss() -> TestResult {
    let delivered = json!({"output":[
        {"id":"synthetic-matrix-reasoning", "type":"reasoning", "summary":[
            {"type":"summary_text", "text":"private-a rredact-custom-"}]},
        {"id":"synthetic-matrix-output", "type":"message", "role":"assistant", "content":[
            {"type":"output_text", "text":"synthetic output rredact-custom-", "annotations":[]}]}
    ]});
    let source = json!({"model":"synthetic-original", "prompt_cache_key":"synthetic-cache", "input":delivered["output"]});
    let request = request(&source)?;
    let issuer = request.base_lease.fork_attempt()?;
    let binding = issuer.bind_actual_attempt(
        "final-fixture-provider",
        "openai_chat",
        "https://synthetic.invalid/v1/responses",
        Some("synthetic-account"),
    )?;
    let pending = issuer
        .capture_delivered_history(&binding, &delivered)
        .ok_or("synthetic matrix capture unavailable")?;
    assert!(
        issuer.publish_delivered_history(&binding, pending),
        "synthetic matrix publication failed"
    );
    for mode in 0..10 {
        let mut body = source.clone();
        let result = request.prepare_owned_attempt_with_history_for_proxy(None,
            "final-fixture-provider", "openai_chat", "https://synthetic.invalid/v1/responses",
            Some("synthetic-account"), &mut body, |mut body, context| {
                body["model"] = json!("synthetic-upstream");
                body.as_object_mut().ok_or(crate::proxy::error::ProxyError::DlpForward(DlpForwardError::RedactionFailed))?
                    .remove("prompt_cache_key");
                let projection = context.projection(&body);
                let fault = (mode == 7).then(|| super::super::history_allocation_test_support::fail_metadata_after(0));
                let (mut converted, mapping) = crate::proxy::providers::transform_codex_chat::responses_to_chat_completions_with_reasoning_and_history(
                    &body, None, if mode == 8 { None } else { projection.as_ref() })?;
                drop(fault);
                match mode {
                    1 => converted["messages"][0]["reasoning_content"] = json!("changed private-a rredact-custom-"),
                    2 => converted["messages"][0]["content"] = json!("changed private-a rredact-custom-"),
                    3 => converted["messages"].as_array_mut().ok_or(crate::proxy::error::ProxyError::DlpForward(DlpForwardError::RedactionFailed))?
                        .push(json!({"role":"user","content":"private-a rredact-custom-"})),
                    4 => converted["messages"][0]["tool_calls"] = json!([{"id":"synthetic-call","type":"function",
                        "function":{"name":"synthetic", "arguments":"{\"value\":\"private-a rredact-custom-\"}"}}]),
                    5 => converted["messages"][0]["name"] = json!("private-a rredact-custom-"),
                    6 => converted["messages"].as_array_mut().ok_or(crate::proxy::error::ProxyError::DlpForward(DlpForwardError::RedactionFailed))?
                        .push(json!({"role":"user","content":[{"type":"future_text","rredact-custom-":"private-a"}]})),
                    9 => converted["messages"].as_array_mut().ok_or(crate::proxy::error::ProxyError::DlpForward(DlpForwardError::RedactionFailed))?
                        .push(json!({"role":"user","content":"rredact-custom-"})),
                    _ => {},
                }
                Ok((converted, mapping))
            });
        if mode == 0 {
            let (attempt, _owner) =
                result.map_err(|_| "synthetic exact Chat matrix must prepare")?;
            assert!(
                attempt.validate_final_send_history(&body).is_ok(),
                "exact Chat mapping must remain bound"
            );
            for pointer in ["/messages/0/content", "/messages/0/reasoning_content"] {
                let mut late = body.clone();
                *late
                    .pointer_mut(pointer)
                    .ok_or("synthetic Chat mapped path missing")? = json!("safe changed");
                assert!(
                    attempt.validate_final_send_history(&late).is_err(),
                    "late safe text change must invalidate source mapping"
                );
                assert!(
                    attempt.encode_outbound_body(&late).is_err(),
                    "encoding must not accept a stale history mapping"
                );
            }
        } else if mode == 9 {
            let (attempt, _owner) =
                result.map_err(|_| "synthetic unproven literal must prepare")?;
            assert!(
                body["messages"]
                    .as_array()
                    .and_then(|messages| messages.last())
                    .and_then(|message| message["content"].as_str())
                    == Some("rredact-custom-"),
                "unproven literal must retain every byte"
            );
            assert!(
                !attempt.has_active_tokens(),
                "literal sibling must not grant authority"
            );
        } else {
            assert!(
                result.is_err(),
                "changed, unproven or optional-miss leaves must stay strict"
            );
        }
    }
    Ok(())
}

#[test]
fn reasoning_history_output_complete_references_keep_request_local_authority() -> TestResult {
    use super::super::tokenization::{TokenCategory, TokenFragment};
    for authorized in [true, false] {
        let mut request = request(&json!({"input":"safe"}))?;
        let mut issuer = request.base_lease.fork_attempt()?;
        let token = issuer.prepare(
            &[TokenFragment {
                raw: "synthetic-complete-reference-source",
                whole_span_categories: &[TokenCategory::Custom],
            }],
            &[],
            |tokens| tokens.first().cloned().ok_or(TokenError::RedactionFailed),
        )?;
        if authorized {
            request.base_lease.prepare(
                &[TokenFragment {
                    raw: "synthetic-complete-reference-source",
                    whole_span_categories: &[TokenCategory::Custom],
                }],
                &[],
                |_| Ok(()),
            )?;
        }
        let binding = issuer.bind_actual_attempt(
            "final-fixture-provider",
            "openai_responses",
            "https://synthetic.invalid/v1/responses",
            Some("synthetic-account"),
        )?;
        let delivered = json!({"output":[{"id":"synthetic-complete-output", "type":"message", "role":"assistant",
            "content":[{"type":"output_text", "text":format!("synthetic malformed rredact-custom- and complete {token}"), "annotations":[]}]}]});
        let pending = issuer
            .capture_delivered_history(&binding, &delivered)
            .ok_or("synthetic complete output capture unavailable")?;
        assert!(
            issuer.publish_delivered_history(&binding, pending),
            "synthetic complete output publication failed"
        );
        let mut body = json!({"input":delivered["output"]});
        request.source_body =
            PayloadJson::try_from_value(&body, request.budget.clone(), request.budget.limit())?;
        let result = request.prepare_attempt_for_proxy(
            "final-fixture-provider",
            "openai_responses",
            "https://synthetic.invalid/v1/responses",
            Some("synthetic-account"),
            &mut body,
            |body| Ok(body.clone()),
        );
        let attempt = result.map_err(|_| "synthetic complete reference must remain readable")?;
        let expected = format!("synthetic malformed rredact-custom- and complete {token}");
        assert!(
            body.pointer("/input/0/content/0/text")
                .and_then(Value::as_str)
                == Some(expected.as_str()),
            "complete reference and surrounding literal bytes must remain unchanged"
        );
        assert!(
            attempt.lease.resolve_if_authorized(&token)?.is_some() == authorized,
            "delivered output must not grant authority to complete references"
        );
        assert!(
            attempt.has_active_tokens() == authorized,
            "only existing request-local authority may activate restoration"
        );
    }
    Ok(())
}

fn synthetic_restored_history_response(text: &str) -> Value {
    json!({"id":"synthetic-restored-history","object":"response","status":"completed","output":[{
        "id":"synthetic-restored-message","type":"message","role":"assistant","status":"completed",
        "content":[
            {"type":"output_text","text":format!("Synthetic first {text}"),"annotations":[]},
            {"type":"output_text","text":format!("Synthetic second {text}"),"annotations":[]}
        ]
    }]})
}

fn assert_restored_history_matches_delivered_not_raw(
    request: &DlpHttpRequest,
    binding: &crate::proxy::security::completed_context::ActualAttemptBinding,
    raw: &Value,
    delivered: &Value,
) -> TestResult {
    use crate::proxy::security::history_provenance::DlpHistoryLeafField as Field;

    let expected = synthetic_restored_history_response("private-a");
    assert!(
        delivered["output"] == expected["output"] && delivered["output"] != raw["output"],
        "final delivery must contain the complete restored typed item"
    );
    let replay = json!({"input":delivered["output"]});
    let projection = request
        .base_lease
        .history_projection(binding, &replay)
        .ok_or("restored delivered item must acquire exact history proof")?;
    for part in 0..2 {
        assert!(
            projection
                .source_for_responses_leaf(0, Field::OutputText, part)
                .is_some(),
            "each restored output leaf must belong to the complete delivered item"
        );
    }
    let raw_replay = json!({"input":raw["output"]});
    assert!(
        request
            .base_lease
            .history_projection(binding, &raw_replay)
            .is_none(),
        "raw tokenized output must not substitute for delivered history"
    );
    for change in 0..5 {
        let mut changed = replay.clone();
        match change {
            0 => changed["input"][0]["id"] = json!("synthetic-other-message"),
            1 => changed["input"][0]["role"] = json!("user"),
            2 => changed["input"][0]["status"] = json!("incomplete"),
            3 => changed["input"][0]["content"][1]["type"] = json!("input_text"),
            _ => changed["input"][0]["content"][1]["annotations"] = json!([{"synthetic":true}]),
        }
        assert!(
            request
                .base_lease
                .history_projection(binding, &changed)
                .is_none(),
            "matching text alone must not authorize a changed typed item"
        );
    }
    Ok(())
}

#[tokio::test]
async fn history_provenance_json_restored_output_matches_delivered_not_raw() -> TestResult {
    use http_body_util::BodyExt;

    let mut source = json!({"input":"private-a"});
    let request = request(&source)?;
    let attempt = request
        .prepare_attempt_for_proxy(
            "final-fixture-provider",
            "openai_responses",
            "https://synthetic.invalid/v1/responses",
            Some("synthetic-account"),
            &mut source,
            |prepared| Ok(prepared.clone()),
        )
        .map_err(|_| "synthetic restored JSON preparation failed")?;
    assert!(
        attempt.has_active_tokens(),
        "restoration fixture needs active authority"
    );
    let binding = attempt.binding.clone();
    let reference = source["input"]
        .as_str()
        .ok_or("synthetic restored JSON reference unavailable")?;
    let raw = synthetic_restored_history_response(reference);
    let raw_replay = json!({"input":raw["output"]});
    assert!(
        request
            .base_lease
            .history_projection(&binding, &raw_replay)
            .is_none(),
        "unpublished raw output must not carry proof"
    );
    let response = attempt
        .restore_client_http_response(
            Response::builder()
                .header("content-type", "application/json")
                .body(Body::from(raw.to_string()))
                .map_err(|_| "synthetic restored JSON response construction failed")?,
            ArgumentNormalizationMode::Exact,
            None,
        )
        .await;
    assert!(
        response.status() == StatusCode::OK,
        "restored JSON must remain deliverable"
    );
    let bytes = response
        .into_body()
        .collect()
        .await
        .map_err(|_| "synthetic restored JSON collection failed")?
        .to_bytes();
    let delivered: Value =
        serde_json::from_slice(&bytes).map_err(|_| "synthetic restored JSON decoding failed")?;
    assert_restored_history_matches_delivered_not_raw(&request, &binding, &raw, &delivered)
}

#[tokio::test]
async fn history_provenance_sse_restored_output_matches_delivered_not_raw() -> TestResult {
    use http_body_util::BodyExt;

    let mut source = json!({"input":"private-a"});
    let request = request(&source)?;
    let attempt = request
        .prepare_attempt_for_proxy(
            "final-fixture-provider",
            "openai_responses",
            "https://synthetic.invalid/v1/responses",
            Some("synthetic-account"),
            &mut source,
            |prepared| Ok(prepared.clone()),
        )
        .map_err(|_| "synthetic restored SSE preparation failed")?;
    assert!(
        attempt.has_active_tokens(),
        "restoration fixture needs active authority"
    );
    let binding = attempt.binding.clone();
    let reference = source["input"]
        .as_str()
        .ok_or("synthetic restored SSE reference unavailable")?;
    let raw = synthetic_restored_history_response(reference);
    let terminal = synthetic_history_sse_frame(
        "response.completed",
        &json!({"type":"response.completed","response":raw}),
    );
    let chunks: Vec<_> = terminal
        .chunks(7)
        .map(|chunk| Ok::<_, std::io::Error>(Bytes::copy_from_slice(chunk)))
        .collect();
    let response = attempt
        .restore_client_http_response(
            Response::builder()
                .header("content-type", "text/event-stream")
                .body(Body::from_stream(futures::stream::iter(chunks)))
                .map_err(|_| "synthetic restored SSE response construction failed")?,
            ArgumentNormalizationMode::Exact,
            None,
        )
        .await;
    assert!(
        response.status() == StatusCode::OK,
        "restored SSE wrapper must be accepted"
    );
    for candidate in [&raw, &synthetic_restored_history_response("private-a")] {
        assert!(
            request
                .base_lease
                .history_projection(&binding, &json!({"input":candidate["output"]}))
                .is_none(),
            "unpolled SSE must not publish either raw or delivered proof"
        );
    }
    let bytes = response
        .into_body()
        .collect()
        .await
        .map_err(|_| "synthetic restored SSE collection failed")?
        .to_bytes();
    let text =
        std::str::from_utf8(&bytes).map_err(|_| "synthetic restored SSE must remain UTF-8")?;
    let mut delivered = None;
    for data in text.lines().filter_map(|line| line.strip_prefix("data:")) {
        if data.trim() == "[DONE]" {
            continue;
        }
        let event: Value = serde_json::from_str(data)
            .map_err(|_| "synthetic restored SSE event decoding failed")?;
        assert!(
            event["type"] != "response.failed",
            "restored SSE must not release a failure event"
        );
        if event["type"] == "response.completed" {
            assert!(
                delivered.is_none(),
                "restored SSE needs exactly one terminal response"
            );
            delivered = event.get("response").cloned();
        }
    }
    let delivered = delivered.ok_or("restored SSE terminal response unavailable")?;
    assert_restored_history_matches_delivered_not_raw(&request, &binding, &raw, &delivered)
}

// These scenarios exercise real admission, preparation, restoration, and replay without transport.
mod synthetic_repeat_diagnostics {
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        OnceLock,
    };

    static CHECKPOINT: AtomicUsize = AtomicUsize::new(0);
    static INSTALLED: OnceLock<Result<(), log::SetLoggerError>> = OnceLock::new();
    struct Logger;
    static LOGGER: Logger = Logger;

    impl log::Log for Logger {
        fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
            metadata.target() == "cc_switch_lib::proxy::security::restore_diagnostics"
        }
        fn log(&self, record: &log::Record<'_>) {
            if !self.enabled(record.metadata()) {
                return;
            }
            let exact =
                super::super::super::reversible_integration_tests::history_client_diagnostic_exact;
            let checkpoint = if exact(
                *record.args(),
                "[DLP] context failure code=dlp_context_unavailable checkpoint=request_prepare",
            ) {
                1
            } else if exact(
                *record.args(),
                "[DLP] context failure code=dlp_context_unavailable checkpoint=final_validation",
            ) {
                2
            } else if exact(
                *record.args(),
                "[DLP] context failure code=dlp_context_unavailable checkpoint=attempt_history",
            ) {
                3
            } else {
                4
            };
            CHECKPOINT.store(checkpoint, Ordering::SeqCst);
        }
        fn flush(&self) {}
    }

    pub(super) struct Reset(log::LevelFilter);
    impl Drop for Reset {
        fn drop(&mut self) {
            log::set_max_level(self.0);
        }
    }
    pub(super) fn install() -> Result<Reset, &'static str> {
        if INSTALLED.get_or_init(|| log::set_logger(&LOGGER)).is_err() {
            return Err("synthetic_repeat stage=fixture category=logger_unavailable");
        }
        let reset = Reset(log::max_level());
        log::set_max_level(log::LevelFilter::Warn);
        super::super::super::restore_diagnostics::log_context_failure(
            super::super::super::restore_diagnostics::ContextCheckpoint::FinalValidation,
        );
        if CHECKPOINT.swap(0, Ordering::SeqCst) != 2 {
            return Err("synthetic_repeat stage=fixture category=logger_control");
        }
        Ok(reset)
    }
    pub(super) fn take_checkpoint() -> usize {
        CHECKPOINT.swap(0, Ordering::SeqCst)
    }
    pub(super) fn finish() -> ! {
        std::process::exit(87);
    }
}

struct SyntheticRepeatRuntimeReset(SecurityRuntimeConfig);

impl Drop for SyntheticRepeatRuntimeReset {
    fn drop(&mut self) {
        super::super::runtime::set_security_runtime_for_test(self.0.clone());
    }
}

fn synthetic_repeat_pii() -> (String, String) {
    let phone = format!("139{}", "0".repeat(8));
    // The administrative prefix is deliberately synthetic; the detector checks the checksum.
    let mut identity = "99000020000101001".to_owned();
    let weights = [7u32, 9, 10, 5, 8, 4, 2, 1, 6, 3, 7, 9, 10, 5, 8, 4, 2];
    let sum: u32 = identity
        .bytes()
        .zip(weights)
        .map(|(digit, weight)| u32::from(digit - b'0') * weight)
        .sum();
    identity.push(['1', '0', 'X', '9', '8', '7', '6', '5', '4', '3', '2'][(sum % 11) as usize]);
    (phone, identity)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SyntheticRepeatHistory {
    Plain,
    OutputLiteral,
    ReasoningExact,
    ReasoningWhitespace,
}

async fn check_synthetic_repeat_pii(
    protocol: &str,
    replay_history: bool,
    history_kind: SyntheticRepeatHistory,
    case_filter: Option<usize>,
) -> TestResult {
    use http_body_util::BodyExt;
    let literal_history = history_kind == SyntheticRepeatHistory::OutputLiteral;
    let reasoning_history = matches!(
        history_kind,
        SyntheticRepeatHistory::ReasoningExact | SyntheticRepeatHistory::ReasoningWhitespace
    );
    let _diagnostics = synthetic_repeat_diagnostics::install()?;

    let _reset =
        SyntheticRepeatRuntimeReset((*super::super::runtime::get_security_runtime()).clone());
    let mut policy = SecurityRuntimeConfig::default();
    for rule in ["chinese_phone_number", "chinese_id_card"] {
        policy
            .builtin_action_overrides
            .insert(rule.into(), AuditAction::RedactRestore);
    }
    super::super::runtime::set_security_runtime_for_test(policy);
    let (phone, identity) = synthetic_repeat_pii();
    assert!(
        super::super::pii::is_valid_chinese_id_card(&identity),
        "synthetic_repeat stage=fixture category=checksum"
    );

    for case in (0..3).filter(|case| case_filter.is_none_or(|selected| selected == *case)) {
        let prompt = match case {
            0 => format!("Synthetic phone: {phone}."),
            1 => format!("Synthetic identity: {identity}."),
            _ => format!("Synthetic phone: {phone}; synthetic identity: {identity}."),
        };
        let gateway = RestorationGateway::with_rules(&[], 32 * 1024 * 1024);
        let mut headers = HeaderMap::new();
        headers.insert(
            "session-id",
            HeaderValue::from_static("synthetic-repeat-session"),
        );
        let mut history = Vec::new();
        for turn in 0..2 {
            let user = json!({"type":"message", "role":"user", "content":[
                {"type":"input_text", "text":prompt}
            ]});
            let mut items = if replay_history {
                history.clone()
            } else {
                Vec::new()
            };
            items.push(user.clone());
            let mut outbound = json!({"model":"synthetic-repeat-model", "input":items});
            let request = match admit_http_request(
                &gateway,
                &AppType::Codex,
                true,
                false,
                &mut outbound,
                &headers,
            )
            .map_err(|_| "synthetic_repeat stage=admission category=error")?
            {
                RestorationAdmission::Admitted(request) => request,
                _ => return Err("synthetic_repeat stage=admission category=intercepted".into()),
            };
            if reasoning_history && turn == 1 {
                use super::super::history_provenance::{DlpHistoryLeafField, DlpHistoryTarget};
                let issuer = request.base_lease.fork_attempt()?;
                let binding = issuer.bind_actual_attempt(
                    "synthetic-repeat-provider",
                    protocol,
                    "https://synthetic.invalid/v1/responses",
                    Some("synthetic-repeat-principal"),
                )?;
                let projection = issuer
                    .history_projection(&binding, &outbound)
                    .ok_or("synthetic_repeat stage=proof category=missing_reasoning")?;
                assert!(
                    projection.policy_for_target(
                        &outbound,
                        DlpHistoryTarget::ResponsesLeaf {
                            input_index: 1,
                            field: DlpHistoryLeafField::ReasoningSummary,
                            part_index: 0,
                        }
                    ) == HistoryLeafPolicy::DeliveredReasoning,
                    "synthetic_repeat stage=proof category=initial_reasoning"
                );
            }
            if literal_history && turn == 1 {
                use super::super::history_provenance::DlpHistoryLeafField;
                let issuer = request.base_lease.fork_attempt()?;
                let binding = issuer.bind_actual_attempt(
                    "synthetic-repeat-provider",
                    protocol,
                    "https://synthetic.invalid/v1/responses",
                    Some("synthetic-repeat-principal"),
                )?;
                let projection = issuer
                    .history_projection(&binding, &outbound)
                    .ok_or("synthetic_repeat stage=proof category=missing")?;
                assert!(
                    projection
                        .source_for_responses_leaf(1, DlpHistoryLeafField::OutputText, 0)
                        .is_some(),
                    "synthetic_repeat stage=proof category=initial_source"
                );
                let mut changed = outbound.clone();
                let original = outbound
                    .pointer("/input/1/content/0/text")
                    .and_then(Value::as_str)
                    .ok_or("synthetic_repeat stage=proof category=missing_text")?;
                // A safe replacement isolates exact-item invalidation without token authority.
                changed["input"][1]["content"][0]["text"] = json!(original
                    .replace(&phone, "[REDACTED]")
                    .replace(&identity, "[REDACTED]"));
                // Remove PII from every other leaf to isolate literal-only final validation.
                for item in changed["input"]
                    .as_array_mut()
                    .ok_or("synthetic input missing")?
                {
                    if item.get("role").and_then(Value::as_str) == Some("user") {
                        let text = item
                            .pointer("/content/0/text")
                            .and_then(Value::as_str)
                            .ok_or("synthetic user text missing")?;
                        item["content"][0]["text"] = json!(text
                            .replace(&phone, "[REDACTED]")
                            .replace(&identity, "[REDACTED]"));
                    }
                }
                assert!(
                    projection.policy_for_target(
                        &changed,
                        super::super::history_provenance::DlpHistoryTarget::ResponsesLeaf {
                            input_index: 1,
                            field: DlpHistoryLeafField::OutputText,
                            part_index: 0,
                        }
                    ) == HistoryLeafPolicy::Strict,
                    "synthetic_repeat stage=proof category=changed_item_policy"
                );
                let before = projection
                    .responses_mapping(&outbound)
                    .ok_or("synthetic_repeat stage=proof category=missing_mapping")?;
                let after = projection
                    .responses_mapping(&changed)
                    .ok_or("synthetic_repeat stage=proof category=missing_changed_mapping")?;
                let context = |history| FinalOutboundContext {
                    runtime: &request.runtime,
                    budget: &request.budget,
                    audit_state: &request.audit_state,
                    provider_id: "synthetic-repeat-provider",
                    binding: Some(&binding),
                    history: Some(history),
                };
                // The original exact leaf permits literal lookalikes but still detects PII.
                assert!(
                    matches!(
                        validate_final_outbound_body(
                            &outbound,
                            "openai_responses",
                            &issuer,
                            context(&before)
                        ),
                        Err(DlpForwardError::RedactionFailed)
                    ),
                    "synthetic_repeat stage=proof category=original_reference_policy"
                );
                assert!(
                    validate_final_outbound_body(
                        &changed,
                        "openai_responses",
                        &issuer,
                        context(&after)
                    )
                    .is_ok(),
                    "synthetic_repeat stage=proof category=changed_reference_policy"
                );
                synthetic_repeat_diagnostics::take_checkpoint();
                assert!(
                    changed != outbound,
                    "synthetic_repeat stage=proof category=unchanged_fixture"
                );
            }
            let prepared = request.prepare_owned_attempt_with_history_for_proxy(
                None,
                "synthetic-repeat-provider",
                protocol,
                "https://synthetic.invalid/v1/responses",
                Some("synthetic-repeat-principal"),
                &mut outbound,
                |body, context| {
                    if protocol == "openai_chat" {
                        let projection = context.projection(&body);
                        let converted = crate::proxy::providers::transform_codex_chat::responses_to_chat_completions_with_reasoning_and_history(
                            &body,
                            None,
                            projection.as_ref(),
                        )?;
                        if reasoning_history && turn == 1 {
                            assert!(converted.1.is_some(), "synthetic_repeat stage=conversion category=mapping_absent");
                            let text = converted.0.pointer("/messages/1/reasoning_content").and_then(Value::as_str)
                                .ok_or(crate::proxy::error::ProxyError::DlpForward(DlpForwardError::RedactionFailed))?;
                            assert!(text == "Synthetic reasoning rredact-custom-",
                                "synthetic_repeat stage=conversion category=reasoning_shape");
                            let target = super::super::history_provenance::DlpHistoryTarget::ChatReasoning { message_index: 1 };
                            let expected = if history_kind == SyntheticRepeatHistory::ReasoningWhitespace {
                                HistoryLeafPolicy::Strict
                            } else {
                                HistoryLeafPolicy::DeliveredReasoning
                            };
                            assert!(converted.1.as_ref().is_some_and(|mapping| {
                                mapping.policy_for_target(context.lease, context.binding, &converted.0, target) == expected
                            }), "synthetic_repeat stage=conversion category=reasoning_policy");
                        }
                        Ok(converted)
                    } else {
                        Ok((body, None))
                    }
                },
            );
            let (attempt, _owner) = prepared.map_err(|error| {
                let checkpoint = synthetic_repeat_diagnostics::take_checkpoint();
                match error {
                    crate::proxy::error::ProxyError::DlpForward(DlpForwardError::ContextUnavailable) if checkpoint == 1 => {
                        "synthetic_repeat checkpoint=request_prepare category=dlp_context_unavailable"
                    }
                    crate::proxy::error::ProxyError::DlpForward(DlpForwardError::ContextUnavailable) => {
                        "synthetic_repeat stage=prepare category=dlp_context_unavailable"
                    }
                    _ => "synthetic_repeat stage=prepare category=other",
                }
            })?;
            let mut authorized = 0;
            attempt.lease.visit_authorized_tokens(|reference| {
                let plaintext = attempt.lease.resolve(reference)?;
                assert!(
                    plaintext == phone || plaintext == identity,
                    "synthetic_repeat stage=authority category=unexpected_plaintext"
                );
                authorized += 1;
                Ok(())
            })?;
            assert!(
                authorized == if case == 2 { 2 } else { 1 },
                "synthetic_repeat stage=authority category=literal_granted_authority"
            );
            if turn == 1 && (literal_history || reasoning_history) {
                assert!(
                    serde_json::to_string(&outbound)?.contains("rredact-custom-"),
                    "synthetic_repeat stage=outbound category=literal_bytes_changed"
                );
            }
            let encoded = attempt
                .encode_outbound_body(&outbound)
                .map_err(|_| "synthetic_repeat stage=encode category=refused")?
                .ok_or("synthetic_repeat stage=encode category=missing_body")?;
            let encoded_text = std::str::from_utf8(encoded.as_ref())
                .map_err(|_| "synthetic_repeat stage=encode category=invalid_utf8")?;
            assert!(
                !encoded_text.contains(&phone) && !encoded_text.contains(&identity),
                "synthetic_repeat stage=outbound category=plaintext"
            );
            let text = if protocol == "openai_chat" {
                outbound
                    .get("messages")
                    .and_then(Value::as_array)
                    .and_then(|messages| messages.last())
                    .and_then(|message| message.get("content"))
                    .and_then(Value::as_str)
            } else {
                outbound
                    .get("input")
                    .and_then(Value::as_array)
                    .and_then(|items| items.last())
                    .and_then(|item| item.pointer("/content/0/text"))
                    .and_then(Value::as_str)
            }
            .ok_or("synthetic_repeat stage=outbound category=missing_text")?;
            assert!(
                !text.contains(&phone) && !text.contains(&identity),
                "synthetic_repeat stage=outbound category=unredacted_leaf"
            );
            let reasoning_text = if history_kind == SyntheticRepeatHistory::ReasoningWhitespace {
                "  Synthetic reasoning rredact-custom-  "
            } else {
                "Synthetic reasoning rredact-custom-"
            };
            let expected_response = if literal_history {
                format!("{prompt} Synthetic literal rredact-custom-")
            } else {
                prompt.clone()
            };
            let response_text = if literal_history {
                format!("{text} Synthetic literal rredact-custom-")
            } else {
                text.to_owned()
            };
            let raw = if protocol == "openai_chat" {
                let mut chat = json!({"id":format!("synthetic-repeat-chat-{turn}"),
                    "object":"chat.completion", "created":0, "model":"synthetic-repeat-model",
                    "choices":[{"index":0, "message":{"role":"assistant", "content":response_text},
                        "finish_reason":"stop"}]});
                if reasoning_history {
                    chat["choices"][0]["message"]["reasoning_content"] = json!(reasoning_text);
                }
                attempt
                    .verify_upstream_json_completion(
                        JsonCompletionProtocol::ChatCompletions,
                        &chat.to_string(),
                    )
                    .map_err(|_| "synthetic_repeat stage=completion category=refused")?;
                crate::proxy::providers::transform_codex_chat::chat_completion_to_response(chat)
                    .map_err(|_| "synthetic_repeat stage=conversion category=refused")?
            } else {
                let message = json!({"id":format!("synthetic-repeat-message-{turn}"),
                "type":"message", "role":"assistant", "status":"completed", "content":[
                    {"type":"output_text", "text":response_text, "annotations":[]}
                ]});
                let output = if reasoning_history {
                    vec![
                        json!({"id":format!("synthetic-repeat-reasoning-{turn}"),
                        "type":"reasoning", "summary":[{"type":"summary_text", "text":reasoning_text}]}),
                        message,
                    ]
                } else {
                    vec![message]
                };
                json!({"id":format!("synthetic-repeat-response-{turn}"), "object":"response",
                    "status":"completed", "output":output})
            };
            let response = attempt
                .restore_client_http_response(
                    Response::builder()
                        .header("content-type", "application/json")
                        .body(Body::from(raw.to_string()))?,
                    ArgumentNormalizationMode::Exact,
                    None,
                )
                .await;
            assert!(
                response.status() == StatusCode::OK,
                "synthetic_repeat stage=restoration category=refused"
            );
            let bytes = response
                .into_body()
                .collect()
                .await
                .map_err(|_| "synthetic_repeat stage=delivery category=body")?
                .to_bytes();
            let delivered: Value = serde_json::from_slice(&bytes)
                .map_err(|_| "synthetic_repeat stage=delivery category=json")?;
            let output_index = usize::from(reasoning_history);
            assert!(
                delivered
                    .get("output")
                    .and_then(Value::as_array)
                    .and_then(|items| items.get(output_index))
                    .and_then(|item| item.pointer("/content/0/text"))
                    .and_then(Value::as_str)
                    == Some(expected_response.as_str()),
                "synthetic_repeat stage=delivery category=restoration_mismatch"
            );
            if reasoning_history {
                assert!(
                    delivered
                        .pointer("/output/0/summary/0/text")
                        .and_then(Value::as_str)
                        == Some(reasoning_text),
                    "synthetic_repeat stage=delivery category=reasoning_mismatch"
                );
            }
            history.push(user);
            history.extend(
                delivered
                    .get("output")
                    .and_then(Value::as_array)
                    .ok_or("synthetic_repeat stage=delivery category=output")?
                    .iter()
                    .cloned(),
            );
        }
    }
    Ok(())
}

async fn run_synthetic_repeat_pii(
    name: &'static str,
    protocol: &str,
    replay_history: bool,
    history_kind: SyntheticRepeatHistory,
    case_filter: Option<usize>,
) -> TestResult {
    let test = format!("proxy::security::http_restoration::final_validation_tests::{name}");
    if history_client_diagnostics::parent("VARYAI_SYNTHETIC_REPEAT_PII_CHILD", &test).await? {
        return Ok(());
    }
    check_synthetic_repeat_pii(protocol, replay_history, history_kind, case_filter).await?;
    synthetic_repeat_diagnostics::finish()
}

#[tokio::test]
#[serial_test::serial]
async fn synthetic_repeat_pii_native_without_history() -> TestResult {
    run_synthetic_repeat_pii(
        "synthetic_repeat_pii_native_without_history",
        "openai_responses",
        false,
        SyntheticRepeatHistory::Plain,
        None,
    )
    .await
}

#[tokio::test]
#[serial_test::serial]
async fn synthetic_repeat_pii_chat_without_history() -> TestResult {
    run_synthetic_repeat_pii(
        "synthetic_repeat_pii_chat_without_history",
        "openai_chat",
        false,
        SyntheticRepeatHistory::Plain,
        None,
    )
    .await
}

#[tokio::test]
#[serial_test::serial]
async fn synthetic_repeat_pii_native_restored_history() -> TestResult {
    run_synthetic_repeat_pii(
        "synthetic_repeat_pii_native_restored_history",
        "openai_responses",
        true,
        SyntheticRepeatHistory::Plain,
        None,
    )
    .await
}

#[tokio::test]
#[serial_test::serial]
async fn synthetic_repeat_pii_chat_restored_history() -> TestResult {
    run_synthetic_repeat_pii(
        "synthetic_repeat_pii_chat_restored_history",
        "openai_chat",
        true,
        SyntheticRepeatHistory::Plain,
        None,
    )
    .await
}

#[tokio::test]
#[serial_test::serial]
async fn synthetic_repeat_pii_native_restored_history_literal() -> TestResult {
    run_synthetic_repeat_pii(
        "synthetic_repeat_pii_native_restored_history_literal",
        "openai_responses",
        true,
        SyntheticRepeatHistory::OutputLiteral,
        None,
    )
    .await
}

#[tokio::test]
#[serial_test::serial]
async fn synthetic_repeat_pii_chat_restored_history_literal() -> TestResult {
    run_synthetic_repeat_pii(
        "synthetic_repeat_pii_chat_restored_history_literal",
        "openai_chat",
        true,
        SyntheticRepeatHistory::OutputLiteral,
        None,
    )
    .await
}

// Regressions retain literal bytes while exercising normal PII protection on both turns.
macro_rules! synthetic_repeat_literal_regression {
    ($name:ident, $protocol:literal, $case:literal) => {
        #[tokio::test]
        #[serial_test::serial]
        async fn $name() -> TestResult {
            run_synthetic_repeat_pii(
                stringify!($name),
                $protocol,
                true,
                SyntheticRepeatHistory::OutputLiteral,
                Some($case),
            )
            .await
        }
    };
}

synthetic_repeat_literal_regression!(
    synthetic_repeat_pii_native_phone_output_literal_preserved_regression,
    "openai_responses",
    0
);
synthetic_repeat_literal_regression!(
    synthetic_repeat_pii_native_identity_output_literal_preserved_regression,
    "openai_responses",
    1
);
synthetic_repeat_literal_regression!(
    synthetic_repeat_pii_native_mixed_output_literal_preserved_regression,
    "openai_responses",
    2
);
synthetic_repeat_literal_regression!(
    synthetic_repeat_pii_chat_phone_output_literal_preserved_regression,
    "openai_chat",
    0
);
synthetic_repeat_literal_regression!(
    synthetic_repeat_pii_chat_identity_output_literal_preserved_regression,
    "openai_chat",
    1
);
synthetic_repeat_literal_regression!(
    synthetic_repeat_pii_chat_mixed_output_literal_preserved_regression,
    "openai_chat",
    2
);

macro_rules! synthetic_repeat_reasoning_replay {
    ($name:ident, $protocol:literal, $kind:ident) => {
        #[tokio::test]
        #[serial_test::serial]
        async fn $name() -> TestResult {
            run_synthetic_repeat_pii(
                stringify!($name),
                $protocol,
                true,
                SyntheticRepeatHistory::$kind,
                None,
            )
            .await
        }
    };
}

synthetic_repeat_reasoning_replay!(
    synthetic_repeat_pii_native_reasoning_exact,
    "openai_responses",
    ReasoningExact
);
synthetic_repeat_reasoning_replay!(
    synthetic_repeat_pii_chat_reasoning_exact,
    "openai_chat",
    ReasoningExact
);
synthetic_repeat_reasoning_replay!(
    synthetic_repeat_pii_native_reasoning_whitespace,
    "openai_responses",
    ReasoningWhitespace
);
synthetic_repeat_reasoning_replay!(
    synthetic_repeat_pii_chat_reasoning_whitespace,
    "openai_chat",
    ReasoningWhitespace
);

macro_rules! synthetic_repeat_reasoning_literal_regression {
    ($name:ident, $case:literal) => {
        #[tokio::test]
        #[serial_test::serial]
        async fn $name() -> TestResult {
            run_synthetic_repeat_pii(
                stringify!($name),
                "openai_chat",
                true,
                SyntheticRepeatHistory::ReasoningWhitespace,
                Some($case),
            )
            .await
        }
    };
}

synthetic_repeat_reasoning_literal_regression!(
    synthetic_repeat_pii_chat_phone_reasoning_literal_preserved_regression,
    0
);
synthetic_repeat_reasoning_literal_regression!(
    synthetic_repeat_pii_chat_identity_reasoning_literal_preserved_regression,
    1
);
synthetic_repeat_reasoning_literal_regression!(
    synthetic_repeat_pii_chat_mixed_reasoning_literal_preserved_regression,
    2
);
