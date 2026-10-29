//! Request-action audits must not imply successful response restoration.
use super::{runtime, AppType, AuditAction};
use serde_json::json;

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[tokio::test]
#[serial_test::serial]
async fn restoration_audit_readonly_legacy_entry_rejects_restore_action() -> TestResult {
    struct RuntimeReset(runtime::SecurityRuntimeConfig);
    impl Drop for RuntimeReset {
        fn drop(&mut self) {
            runtime::set_security_runtime_for_test(self.0.clone());
        }
    }
    let _reset = RuntimeReset((*runtime::get_security_runtime()).clone());
    let mut config = runtime::SecurityRuntimeConfig::default();
    config
        .builtin_action_overrides
        .insert("chinese_phone_number".into(), AuditAction::RedactRestore);
    runtime::set_security_runtime_for_test(config);
    let body = json!({"input": "fixture 13800138000"});
    let response = super::inspect_and_intercept(&AppType::Codex, &body)?;
    assert!(
        response.is_some(),
        "read-only callers must refuse restoration findings without a gateway"
    );
    if let Some(response) = response {
        assert_eq!(response.status(), axum::http::StatusCode::BAD_REQUEST);
    }
    assert_eq!(body, json!({"input": "fixture 13800138000"}));
    Ok(())
}

use super::{
    aggregation::PendingAuditEvent,
    payload_budget::PayloadBudget,
    restoration_audit::{PendingRequestAudit, RequestAuditState},
    runtime::{CompiledCustomRule, SecurityRuntimeConfig},
    tokenization::DlpTokenStore,
    RiskLevel,
};

fn policy(rules: &[(&str, &str, AuditAction)]) -> SecurityRuntimeConfig {
    let mut runtime = SecurityRuntimeConfig::default();
    runtime.custom_rules = rules
        .iter()
        .map(|(id, pattern, action)| CompiledCustomRule {
            id: (*id).into(),
            name: "Synthetic policy".into(),
            rule_type: "regex".into(),
            pattern: (*pattern).into(),
            regex: regex::Regex::new(pattern).ok(),
            risk_level: RiskLevel::High,
            action: *action,
            enabled: true,
        })
        .collect();
    runtime
}

#[test]
fn restoration_audit_only_first_permitted_attempt_records_and_new_history_is_independent(
) -> TestResult {
    let runtime = policy(&[("fixture", "private-[ab]", AuditAction::RedactRestore)]);
    let budget = PayloadBudget::with_limit(4 * 1024 * 1024);
    let store = DlpTokenStore::new();
    let lease = store.admit("codex", None, 1)?;
    let state = RequestAuditState::new(&AppType::Codex, Some("fixture-session"), budget.clone())?;
    let first = PendingRequestAudit::capture(
        &json!({"input": "private-a"}),
        &runtime,
        &[],
        &lease,
        budget.clone(),
    )
    .map_err(|_| std::io::Error::other("fixture summary failed"))?;
    let mut events = Vec::new();
    assert!(
        events.is_empty(),
        "preparation alone is not an outbound action"
    );
    state.record_with(&first, "first-provider", Some("request-one"), false, |e| {
        events.push(e)
    });
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].log.action_taken, "redact_restore");
    assert_eq!(events[0].log.session_id.as_deref(), Some("fixture-session"));
    assert_eq!(events[0].log.provider_id, "first-provider");
    let retry = PendingRequestAudit::capture(
        &json!({"input": ["private-a", "private-a"]}),
        &runtime,
        &[],
        &lease,
        budget.clone(),
    )
    .map_err(|_| std::io::Error::other("fixture summary failed"))?;
    state.record_with(
        &retry,
        "fallback-provider",
        Some("request-one"),
        false,
        |e| events.push(e),
    );
    assert_eq!(
        events.len(),
        1,
        "location/provider changes cannot recount the same finding"
    );
    let enriched = PendingRequestAudit::capture(
        &json!({"input": ["private-a", "private-b"]}),
        &runtime,
        &[],
        &lease,
        budget.clone(),
    )
    .map_err(|_| std::io::Error::other("fixture summary failed"))?;
    assert_eq!(
        events.len(),
        1,
        "new history waits for its permitted attempt"
    );
    state.record_with(
        &enriched,
        "fallback-provider",
        Some("request-one"),
        false,
        |e| events.push(e),
    );
    assert_eq!(events.len(), 2);
    assert_eq!(events[1].log.provider_id, "fallback-provider");
    let independent = RequestAuditState::new(&AppType::Codex, Some("fixture-session"), budget)?;
    independent.record_with(&first, "first-provider", Some("request-two"), false, |e| {
        events.push(e)
    });
    assert_eq!(
        events.len(),
        3,
        "deduplication is per admitted request, not per session"
    );
    Ok(())
}

#[test]
fn restoration_audit_irreversible_overlap_and_cross_field_conflict_record_redact() -> TestResult {
    let runtime = policy(&[
        ("restore", "private-a(?:-tail)?", AuditAction::RedactRestore),
        ("irreversible", "private-a", AuditAction::Redact),
        ("audit", "visible-marker", AuditAction::Audit),
    ]);
    let budget = PayloadBudget::with_limit(4 * 1024 * 1024);
    let lease = DlpTokenStore::new().admit("codex", None, 1)?;
    let state = RequestAuditState::new(&AppType::Codex, None, budget.clone())?;
    let pending = PendingRequestAudit::capture(
        &json!({"input": ["private-a-tail", "private-a", "visible-marker"]}),
        &runtime,
        &[],
        &lease,
        budget,
    )
    .map_err(|_| std::io::Error::other("fixture summary failed"))?;
    let mut events = Vec::new();
    state.record_with(&pending, "provider", None, false, |e| events.push(e));
    let restored: Vec<_> = events
        .iter()
        .filter(|event| event.log.rule_id == "restore")
        .collect();
    assert_eq!(restored.len(), 2);
    assert!(restored
        .iter()
        .all(|event| event.log.action_taken == "redact"));
    assert_eq!(
        events
            .iter()
            .find(|event| event.log.rule_id == "audit")
            .map(|event| event.log.action_taken.as_str()),
        Some("audit")
    );
    Ok(())
}

#[test]
fn restoration_audit_refusal_records_only_actual_nonallow_findings_and_masks_configuration(
) -> TestResult {
    let mut runtime = policy(&[
        ("restore", "private-a", AuditAction::RedactRestore),
        ("audit", "visible-marker", AuditAction::Audit),
        ("allowed", "allowed-marker", AuditAction::Allow),
    ]);
    runtime.custom_rules[0].name = "private-a vary_cu_00000000000000000000_ai".into();
    let budget = PayloadBudget::with_limit(4 * 1024 * 1024);
    let pending = PendingRequestAudit::capture_refusal(
        &json!({"input": "private-a visible-marker allowed-marker"}),
        &runtime,
        budget.clone(),
    )?;
    let state = RequestAuditState::new(&AppType::Codex, None, budget.clone())?;
    let mut events: Vec<PendingAuditEvent> = Vec::new();
    state.record_with(&pending, "provider", None, true, |e| events.push(e));
    assert_eq!(events.len(), 2);
    assert!(events.iter().all(|event| event.log.action_taken == "block"));
    let logs = serde_json::to_string(&events.iter().map(|event| &event.log).collect::<Vec<_>>())?;
    assert!(!logs.contains("private-a"));
    assert!(!logs.contains("vary_cu_"));
    assert!(!logs.contains("visible-marker"));
    let no_finding =
        PendingRequestAudit::capture_refusal(&json!({"input": "harmless"}), &runtime, budget)?;
    state.record_with(&no_finding, "provider", None, true, |e| events.push(e));
    assert_eq!(
        events.len(),
        2,
        "cursor/authorization failures cannot fabricate findings"
    );
    Ok(())
}

#[test]
fn restoration_audit_summary_ownership_is_bounded_and_freed() -> TestResult {
    let runtime = policy(&[("fixture", "private-a", AuditAction::RedactRestore)]);
    let lease = DlpTokenStore::new().admit("codex", None, 1)?;
    let tiny = PayloadBudget::with_limit(0);
    assert!(PendingRequestAudit::capture(
        &json!({"input":"private-a"}),
        &runtime,
        &[],
        &lease,
        tiny.clone()
    )
    .is_err());
    assert_eq!(tiny.used(), 0);
    let budget = PayloadBudget::with_limit(4 * 1024 * 1024);
    let pending = PendingRequestAudit::capture(
        &json!({"input":"private-a"}),
        &runtime,
        &[],
        &lease,
        budget.clone(),
    )
    .map_err(|_| std::io::Error::other("fixture summary failed"))?;
    assert!(budget.used() > 0);
    drop(pending);
    assert_eq!(budget.used(), 0);
    Ok(())
}

#[test]
#[serial_test::serial]
fn restoration_audit_http_pretokenized_and_history_wait_for_actual_attempt() -> TestResult {
    use super::http_restoration::{admit_http_request, RestorationAdmission, RestorationGateway};
    let previous = (*runtime::get_security_runtime()).clone();
    struct Reset(SecurityRuntimeConfig);
    impl Drop for Reset {
        fn drop(&mut self) {
            runtime::set_security_runtime_for_test(self.0.clone());
        }
    }
    let _reset = Reset(previous);
    runtime::set_security_runtime_for_test(policy(&[(
        "fixture",
        "private-[ab]",
        AuditAction::RedactRestore,
    )]));
    let gateway = RestorationGateway::with_rules(&[], 8 * 1024 * 1024);
    let mut body = json!({"input": "private-a"});
    let request = match admit_http_request(
        &gateway,
        &AppType::Codex,
        true,
        false,
        &mut body,
        &Default::default(),
    )? {
        RestorationAdmission::Admitted(request) => request,
        _ => return Err("synthetic restoration request was not admitted".into()),
    };
    assert!(body["input"]
        .as_str()
        .is_some_and(|text| text.contains("vary_cu_")));
    let _source_owner;
    let mut enriched = {
        let (value, owner) = request
            .clone_source_body()
            .map_err(|error| std::io::Error::other(error.code()))?;
        _source_owner = owner;
        value
    };
    enriched["instructions"] = json!("private-b");
    let attempt = request
        .prepare_attempt(
            "first-provider",
            "openai_responses",
            "https://synthetic.invalid/v1/responses",
            None,
            &mut enriched,
            |body| Ok(body.clone()),
        )
        .map_err(|error| std::io::Error::other(error.code()))?;
    let events = attempt.audit_events_for_test("first-provider", Some("request-one"));
    assert_eq!(
        events.len(),
        2,
        "raw admission and newly enriched history must survive tokenization"
    );
    assert!(events
        .iter()
        .all(|event| event.log.action_taken == "redact_restore"));
    let _retry_source_owner;
    let mut retry_body = {
        let (value, owner) = request
            .clone_source_body()
            .map_err(|error| std::io::Error::other(error.code()))?;
        _retry_source_owner = owner;
        value
    };
    retry_body["instructions"] = json!("private-b");
    let retry = request
        .prepare_attempt(
            "fallback-provider",
            "openai_responses",
            "https://synthetic.invalid/v1/responses",
            None,
            &mut retry_body,
            |body| Ok(body.clone()),
        )
        .map_err(|error| std::io::Error::other(error.code()))?;
    assert!(retry
        .audit_events_for_test("fallback-provider", Some("request-one"))
        .is_empty());
    Ok(())
}

#[test]
fn restoration_audit_refusal_in_unsupported_chat_roots_is_observable_without_plaintext(
) -> TestResult {
    let runtime = policy(&[("fixture", "private-a", AuditAction::RedactRestore)]);
    let budget = PayloadBudget::with_limit(4 * 1024 * 1024);
    let pending = PendingRequestAudit::capture_refusal(
        &json!({"messages":[{"role":"user", "content":"private-a"}]}),
        &runtime,
        budget.clone(),
    )?;
    let state = RequestAuditState::new(&AppType::Codex, None, budget)?;
    let mut events = Vec::new();
    state.record_with(&pending, "fixture-provider", None, true, |event| {
        events.push(event)
    });
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].log.action_taken, "block");
    assert_eq!(
        state.recorded_for_test().len(),
        1,
        "test observer sees actual emission, not preparation"
    );
    Ok(())
}

#[test]
fn restoration_audit_auxiliary_entropy_skips_summary_only_and_recovers() -> TestResult {
    let runtime = policy(&[("synthetic-rule", "private-a", AuditAction::RedactRestore)]);
    let budget = PayloadBudget::with_limit(4 * 1024 * 1024);
    let store = DlpTokenStore::new();
    let lease = store.admit("codex", None, 1)?;
    let state = RequestAuditState::new(&AppType::Codex, None, budget.clone())?;
    let body = json!({"input":"private-a"});
    let fault = super::identity::test_support::fail_audit_fingerprint();
    let degraded = PendingRequestAudit::capture(&body, &runtime, &[], &lease, budget.clone());
    assert!(
        degraded.is_ok(),
        "auxiliary fingerprint failure must not fail core preparation"
    );
    let mut events = Vec::new();
    state.record_with(
        &degraded.map_err(|_| "synthetic summary unavailable")?,
        "synthetic-provider",
        None,
        false,
        |e| events.push(e),
    );
    assert!(
        events.is_empty(),
        "no substitute identity, summary or dedup entry on auxiliary failure"
    );
    drop(fault);
    let recovered = PendingRequestAudit::capture(&body, &runtime, &[], &lease, budget)
        .map_err(|_| "synthetic recovered summary unavailable")?;
    state.record_with(&recovered, "synthetic-provider", None, false, |e| {
        events.push(e)
    });
    assert_eq!(
        events.len(),
        1,
        "recovery records genuine identity, not a previously fabricated dedup identity"
    );
    assert_eq!(events[0].log.matched_snippet_masked, "[redacted]");
    assert_eq!(events[0].log.action_taken, "redact_restore");
    Ok(())
}
