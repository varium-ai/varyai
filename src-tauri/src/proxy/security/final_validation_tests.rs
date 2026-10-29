//! Synthetic fixtures for final protocol-bound request inspection and refusal audits.
use super::*;
use crate::proxy::security::{runtime::CompiledCustomRule, AuditAction, RiskLevel};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn request(source: &Value) -> Result<DlpHttpRequest, TokenError> {
    let store = DlpTokenStore::new();
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
        !encoded.contains("vary_"),
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
            encoded.contains("vary_"),
            "conversion must retain the authorized reference"
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
