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
    SecurityRuntimeConfig {
        custom_rules: rules
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
            .collect(),
        ..Default::default()
    }
}

#[test]
fn generic_sk_audit_records_the_selected_hiding_action() -> TestResult {
    let id = "catalog:generic-sk-key-notice";
    for (action, selected) in [
        (AuditAction::RedactRestore, vec![]),
        (AuditAction::Redact, vec![id]),
    ] {
        let mut runtime = SecurityRuntimeConfig::default();
        runtime.builtin_action_overrides.insert(id.into(), action);
        let budget = PayloadBudget::with_limit(4 * 1024 * 1024);
        let lease = DlpTokenStore::new().admit("codex", None, 1)?;
        let state = RequestAuditState::new(&AppType::Codex, None, budget.clone())?;
        let pending = PendingRequestAudit::capture(
            &json!({"input":"sk-syntheticfixture"}),
            &runtime,
            &selected,
            &lease,
            budget,
        )
        .map_err(|_| std::io::Error::other("fixture summary failed"))?;
        let mut events = Vec::new();
        state.record_with(&pending, "synthetic-provider", None, false, |event| {
            events.push(event)
        });
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].log.rule_id, id);
        assert_eq!(events[0].log.action_taken, "redact_restore");
    }
    Ok(())
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
    runtime.custom_rules[0].name = "private-a rredact-custom-00000000000000000000-ai".into();
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
    assert!(!logs.contains("rredact-custom-"));
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
        false,
        &mut body,
        &Default::default(),
    )? {
        RestorationAdmission::Admitted(request) => request,
        _ => return Err("synthetic restoration request was not admitted".into()),
    };
    assert!(body["input"]
        .as_str()
        .is_some_and(|text| text.contains("rredact-custom-")));
    let _source_owner;
    let mut enriched = {
        let (value, owner) = request
            .clone_source_body()
            .map_err(|error| std::io::Error::other(error.code()))?;
        _source_owner = owner;
        value
    };
    enriched["prompt"] = json!("private-b");
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
    retry_body["prompt"] = json!("private-b");
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

#[test]
fn resent_history_does_not_repeat_audits_even_when_the_request_is_refused() -> TestResult {
    let runtime = policy(&[
        ("past", "private-a", AuditAction::RedactRestore),
        ("current", "private-b", AuditAction::RedactRestore),
    ]);
    for root in ["input", "messages", "contents"] {
        for current in ["private-b", "safe-current-text"] {
            let body = json!({(root): [
                {"role":"user", "content":"private-a", "parts":[{"text":"private-a"}]},
                {"role":"assistant", "content":"safe reply", "parts":[{"text":"safe reply"}]},
                {"role":"user", "content":current, "parts":[{"text":current}]}
            ]});
            let mut inspected = body.clone();
            assert!(
                super::SecurityAuditEngine::inspect_and_redact_with_runtime(
                    &mut inspected,
                    &runtime
                )
                .is_blocked(),
                "history must remain subject to the hiding policy"
            );
            let budget = PayloadBudget::with_limit(4 * 1024 * 1024);
            let state = RequestAuditState::new(&AppType::Codex, None, budget.clone())?;
            let pending = PendingRequestAudit::capture_refusal(&body, &runtime, budget)?;
            let mut events = Vec::new();
            state.record_with(&pending, "fixture", None, true, |event| events.push(event));
            assert_eq!(events.len(), usize::from(current == "private-b"));
            assert!(events.iter().all(|event| event.log.rule_id == "current"));
        }
    }
    Ok(())
}

#[test]
fn resent_responses_history_is_inspected_but_only_current_findings_are_reported() -> TestResult {
    let runtime = policy(&[
        ("past", "private-a", AuditAction::RedactRestore),
        ("current", "private-b", AuditAction::RedactRestore),
    ]);
    let body = json!({"input":[{"role":"user","content":"private-a"},{"role":"assistant","content":"safe reply"},{"role":"user","content":"private-b"}]});
    let budget = PayloadBudget::with_limit(4 * 1024 * 1024);
    let lease = DlpTokenStore::new().admit("codex", None, 1)?;
    let pending = PendingRequestAudit::capture(&body, &runtime, &[], &lease, budget.clone())
        .map_err(|_| std::io::Error::other("fixture summary failed"))?;
    let state = RequestAuditState::new(&AppType::Codex, None, budget)?;
    let mut events = Vec::new();
    state.record_with(&pending, "fixture", None, false, |event| events.push(event));
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].log.rule_id, "current");
    Ok(())
}

#[test]
#[serial_test::serial]
fn capture_refusal_for_lease_skips_authorized_tokens_without_suppressing_sibling_plaintext(
) -> TestResult {
    use crate::proxy::{
        protocol_bridge::Protocol,
        security::{
            request_preparation::prepare_observed_request,
            restoration_contract::{
                ClientProtocol, ContinuityScope, EditConstraint, IngressNamespace,
                RequestObservation, SourceClass,
            },
            unified_test_support::ResponsesFixture,
        },
    };
    let _runtime_fixture = ResponsesFixture::new();
    let admission = runtime::admit_security_runtime()?;
    let mut cfg = policy(&[("fixture", "private-a", AuditAction::RedactRestore)]);
    cfg.builtin_action_overrides.insert(
        "catalog:login-url-password".into(),
        AuditAction::RedactRestore,
    );
    let store = _runtime_fixture.store.clone();
    let mut lease = store.admit_scoped_published_policy(
        ContinuityScope::Native {
            client: ClientProtocol::new(Protocol::Chat),
            ingress: IngressNamespace::Direct,
        },
        "codex",
        Some("audit-lease-session"),
        admission.restoration_policy_revision,
    )?;
    let _binding = lease.bind_actual_attempt(
        "fixture-provider",
        "openai_chat",
        "https://synthetic.invalid/v1/chat/completions",
        Some("synthetic-account"),
    )?;
    let budget = PayloadBudget::with_limit(4 * 1024 * 1024);
    let seed = json!({"messages":[{"role":"user","content":"https://user:synthetic-url-pwd@example.invalid"}]});
    let mut obs =
        RequestObservation::new(&seed, ClientProtocol::new(Protocol::Chat), budget.clone())?;
    obs.observe_text(
        &seed["messages"][0]["content"],
        SourceClass::CurrentInput,
        EditConstraint::Editable,
    )?;
    let prepared = prepare_observed_request(
        &mut lease,
        &obs,
        &cfg,
        &["catalog:login-url-password"],
        &[],
        |_, _, _| Ok(()),
    )
    .map_err(|_| std::io::Error::other("seed redaction failed"))?;
    let redacted_url = prepared.body.as_value()["messages"][0]["content"]
        .as_str()
        .ok_or("missing redacted url")?
        .to_owned();
    assert!(redacted_url.contains("rredact-password-"));
    assert!(!redacted_url.contains("synthetic-url-pwd"));
    drop(prepared);
    drop(obs);

    let pending_token_only = PendingRequestAudit::capture_refusal_for_lease(
        &json!({"input": redacted_url}),
        &cfg,
        &lease,
        budget.clone(),
    )?;
    let state = RequestAuditState::new(&AppType::Codex, None, budget.clone())?;
    let mut events = Vec::new();
    state.record_with(&pending_token_only, "provider", None, true, |e| {
        events.push(e)
    });
    assert!(
        events.is_empty(),
        "authorized password reference inside login URL must not trigger false block audit"
    );

    let pending_with_malformed_and_plaintext = PendingRequestAudit::capture_refusal_for_lease(
        &json!({"input": format!("{redacted_url} rredact-incomplete private-a")}),
        &cfg,
        &lease,
        budget.clone(),
    )?;
    state.record_with(
        &pending_with_malformed_and_plaintext,
        "provider",
        None,
        true,
        |e| events.push(e),
    );
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].log.rule_id, "fixture");
    assert_eq!(events[0].log.action_taken, "block");

    let pending_without_lease =
        PendingRequestAudit::capture_refusal(&json!({"input": redacted_url}), &cfg, budget)?;
    let mut legacy_events = Vec::new();
    state.record_with(&pending_without_lease, "provider", None, true, |e| {
        legacy_events.push(e)
    });
    assert!(
        legacy_events
            .iter()
            .any(|event| event.log.rule_id == "catalog:login-url-password"),
        "lease-less capture_refusal preserves historical behavior"
    );
    Ok(())
}
