//! Synthetic end-to-end tests of private request preparation, without networking.
use serde_json::{json, Value};

use super::{
    payload_budget::PayloadBudget,
    payload_buffer::test_support::{self, Action, Phase},
    request_plan::PlanError,
    request_preparation::{prepare_codex_request_with_budget, DlpPreparedRequest},
    runtime::{CompiledCustomRule, SecurityRuntimeConfig},
    tokenization::{DlpTokenStore, TokenCategory, TokenError, TokenFragment},
    types::{AuditAction, RiskLevel},
};

// Ordinary preparation fixtures must not race the sole process-global budget probe.
fn prepare_codex_request<T>(
    lease: &mut super::tokenization::DlpRequestLease,
    body: &Value,
    runtime: &SecurityRuntimeConfig,
    selected: &[&str],
    history: &[&str],
    validate: impl FnMut(&Value, &[u8], &[&str]) -> Result<T, TokenError>,
) -> Result<DlpPreparedRequest<T>, PlanError> {
    prepare_codex_request_with_budget(
        lease,
        body,
        runtime,
        selected,
        history,
        PayloadBudget::with_limit(64 * 1024 * 1024),
        validate,
    )
}

fn rule(id: &str, pattern: &str, action: AuditAction) -> CompiledCustomRule {
    CompiledCustomRule {
        id: id.into(),
        name: "synthetic".into(),
        rule_type: "regex".into(),
        pattern: pattern.into(),
        regex: regex::Regex::new(pattern).ok(),
        risk_level: RiskLevel::Medium,
        action,
        enabled: true,
    }
}

#[test]
fn preparation_connects_detectors_carriers_transaction_and_exact_serialization(
) -> Result<(), PlanError> {
    let store = DlpTokenStore::with_test_key(std::array::from_fn(|index| index as u8));
    let mut lease = store.admit("codex", Some("synthetic-session"), 1)?;
    let body = json!({"instructions": "13800138000", "input": [{"type": "function_call", "name": "synthetic", "arguments": r#"{ "value": "13800138000", "number": 18446744073709551617 }"#}]});
    let prepared = prepare_codex_request(
        &mut lease,
        &body,
        &SecurityRuntimeConfig::default(),
        &["chinese_phone_number"],
        &[],
        |candidate, encoded, proof| {
            assert!(serde_json::from_slice::<Value>(encoded).ok().as_ref() == Some(candidate));
            assert_eq!(proof.len(), 1);
            assert!(store.stats()?.entries == 0);
            Ok(42usize)
        },
    )?;
    assert!(prepared.body.as_value()["instructions"] == "vary_ph_413ff0a68f6f0d574cb5_ai");
    assert!(
        prepared.body.as_value()["input"][0]["arguments"]
            == r#"{ "value": "vary_ph_413ff0a68f6f0d574cb5_ai", "number": 18446744073709551617 }"#
    );
    assert_eq!(
        (
            prepared.reversible_occurrences,
            prepared.irreversible_occurrences
        ),
        (2, 0)
    );
    assert_eq!(prepared.validated, 42);
    assert!(!prepared.encoded.as_slice().is_empty());
    assert_eq!(store.stats()?.entries, 1);
    assert!(body["instructions"] == "13800138000");
    Ok(())
}

#[test]
fn preparation_keeps_unselected_redaction_irreversible() -> Result<(), PlanError> {
    let store = DlpTokenStore::new();
    let mut lease = store.admit("codex", None, 1)?;
    let prepared = prepare_codex_request(
        &mut lease,
        &json!({"input": "13800138000"}),
        &SecurityRuntimeConfig::default(),
        &[],
        &[],
        |_, _, proof| {
            assert!(proof.is_empty());
            Ok(())
        },
    )?;
    assert!(prepared.body.as_value()["input"] == "[REDACTED]");
    assert_eq!(
        (
            prepared.reversible_occurrences,
            prepared.irreversible_occurrences
        ),
        (0, 1)
    );
    assert_eq!(store.stats()?.entries, 0);
    Ok(())
}

#[test]
fn preparation_rejects_sensitive_read_only_values_before_mapping_commit() -> Result<(), PlanError> {
    let store = DlpTokenStore::new();
    let mut lease = store.admit("codex", None, 1)?;
    let body = json!({"instructions": "13800138000", "input": [{"type": "function_call", "name": "13800138000", "arguments": "{}"}]});
    let result = prepare_codex_request(
        &mut lease,
        &body,
        &SecurityRuntimeConfig::default(),
        &["chinese_phone_number"],
        &[],
        |_, _, _| Ok(()),
    );
    assert_eq!(result.err(), Some(PlanError::Unsupported));
    assert_eq!(store.stats()?.entries, 0);
    Ok(())
}

#[test]
fn preparation_preserves_block_precedence_and_never_promotes_allow_or_generic_sk(
) -> Result<(), PlanError> {
    let store = DlpTokenStore::new();
    let mut lease = store.admit("codex", None, 1)?;
    let mut runtime = SecurityRuntimeConfig::default();
    runtime.custom_rules.push(rule(
        "synthetic-block",
        "synthetic-stop",
        AuditAction::Block,
    ));
    let blocked = prepare_codex_request(
        &mut lease,
        &json!({"input": "synthetic-stop 13800138000"}),
        &runtime,
        &["synthetic-block", "chinese_phone_number"],
        &[],
        |_, _, _| Ok(()),
    );
    assert_eq!(blocked.err(), Some(PlanError::Blocked));
    runtime.custom_rules.clear();
    runtime.custom_rules.push(rule(
        "synthetic-allow",
        "synthetic-allowed",
        AuditAction::Allow,
    ));
    let allowed = prepare_codex_request(
        &mut lease,
        &json!({"input": "synthetic-allowed sk-shortsynthetic"}),
        &runtime,
        &["synthetic-allow", "catalog:generic-sk-key-notice"],
        &[],
        |_, _, proof| {
            assert!(proof.is_empty());
            Ok(())
        },
    )?;
    assert!(allowed.body.as_value()["input"] == "synthetic-allowed sk-shortsynthetic");
    assert_eq!(store.stats()?.entries, 0);
    Ok(())
}

#[test]
fn preparation_exempts_only_proven_token_spans_during_residual_inspection() -> Result<(), PlanError>
{
    let store = DlpTokenStore::new();
    let mut lease = store.admit("codex", None, 1)?;
    let mut runtime = SecurityRuntimeConfig::default();
    runtime.custom_rules.push(rule(
        "synthetic-token-syntax",
        "vary_[a-z_0-9]+",
        AuditAction::Redact,
    ));
    let prepared = prepare_codex_request(
        &mut lease,
        &json!({"input": "13800138000"}),
        &runtime,
        &["chinese_phone_number"],
        &[],
        |_, _, _| Ok(()),
    )?;
    assert!(prepared.body.as_value()["input"]
        .as_str()
        .is_some_and(|s| s.starts_with("vary_ph_")));
    assert_eq!(store.stats()?.entries, 1);
    let store = DlpTokenStore::new();
    let mut lease = store.admit("codex", None, 1)?;
    runtime.custom_rules.push(rule(
        "synthetic-crossing",
        "prefix-vary_[a-z_0-9]+",
        AuditAction::Redact,
    ));
    let rejected = prepare_codex_request(
        &mut lease,
        &json!({"input": "prefix-13800138000"}),
        &runtime,
        &["chinese_phone_number"],
        &[],
        |_, _, _| Ok(()),
    );
    assert_eq!(
        rejected.err(),
        Some(PlanError::Token(TokenError::RedactionFailed))
    );
    assert_eq!(store.stats()?.entries, 0);
    Ok(())
}

#[test]
fn preparation_serialized_candidate_validation_failure_rolls_back_every_new_mapping(
) -> Result<(), PlanError> {
    let store = DlpTokenStore::new();
    let mut lease = store.admit("codex", None, 1)?;
    let mut proposed = String::new();
    let result = prepare_codex_request(
        &mut lease,
        &json!({"prompt": "13800138000"}),
        &SecurityRuntimeConfig::default(),
        &["chinese_phone_number"],
        &[],
        |_, _, proof| {
            proposed = proof[0].to_owned();
            Err::<(), _>(TokenError::RedactionFailed)
        },
    );
    assert_eq!(
        result.err(),
        Some(PlanError::Token(TokenError::RedactionFailed))
    );
    assert_eq!(store.stats()?.entries, 0);
    assert_eq!(
        lease.resolve(&proposed).err(),
        Some(TokenError::ContextUnavailable)
    );
    Ok(())
}

#[test]
fn preparation_uses_current_lease_history_and_rejects_unknown_references() -> Result<(), PlanError>
{
    let store = DlpTokenStore::new();
    let mut lease = store.admit("codex", Some("synthetic-history"), 1)?;
    let tokens = lease.prepare(
        &[TokenFragment {
            raw: "synthetic-history-plaintext",
            whole_span_categories: &[TokenCategory::Custom],
        }],
        &[],
        |tokens| Ok(tokens.to_vec()),
    )?;
    let body = json!({"input": tokens[0]});
    let prepared = prepare_codex_request(
        &mut lease,
        &body,
        &SecurityRuntimeConfig::default(),
        &[],
        &[&tokens[0]],
        |_, _, proof| {
            assert_eq!(proof.len(), 1);
            Ok(())
        },
    )?;
    assert!(prepared.body.as_value() == &body);
    let mut sibling = store.admit("codex", Some("synthetic-history"), 1)?;
    let result = prepare_codex_request(
        &mut sibling,
        &body,
        &SecurityRuntimeConfig::default(),
        &[],
        &[],
        |_, _, _| Ok(()),
    );
    assert_eq!(
        result.err(),
        Some(PlanError::Token(TokenError::ContextUnavailable))
    );
    Ok(())
}

#[test]
fn preparation_password_context_and_cross_field_irreversibility_are_preserved(
) -> Result<(), PlanError> {
    let store = DlpTokenStore::new();
    let mut lease = store.admit("codex", None, 1)?;
    let body = json!({"instructions": "synthetic-secret", "input": [{"type": "function_call", "arguments": {"password": "synthetic-secret"}}]});
    let mut runtime = SecurityRuntimeConfig::default();
    runtime.custom_rules.push(rule(
        "synthetic-exact",
        "synthetic-secret",
        AuditAction::Redact,
    ));
    runtime
        .builtin_action_overrides
        .insert("catalog:password-field".into(), AuditAction::Redact);
    let prepared = prepare_codex_request(
        &mut lease,
        &body,
        &runtime,
        &["synthetic-exact"],
        &[],
        |_, _, proof| {
            assert!(proof.is_empty());
            Ok(())
        },
    )?;
    assert!(prepared.body.as_value()["instructions"] == "[REDACTED]");
    assert!(prepared.body.as_value()["input"][0]["arguments"]["password"] == "[REDACTED]");
    assert_eq!(prepared.irreversible_occurrences, 2);
    assert_eq!(store.stats()?.entries, 0);
    Ok(())
}

#[test]
fn preparation_residual_custom_match_on_placeholder_fails_closed() -> Result<(), PlanError> {
    let store = DlpTokenStore::new();
    let mut lease = store.admit("codex", None, 1)?;
    let mut runtime = SecurityRuntimeConfig::default();
    runtime.custom_rules.push(rule(
        "synthetic-placeholder",
        "REDACTED",
        AuditAction::Redact,
    ));
    let result = prepare_codex_request(
        &mut lease,
        &json!({"input": "13800138000"}),
        &runtime,
        &[],
        &[],
        |_, _, _| Ok(()),
    );
    assert_eq!(
        result.err(),
        Some(PlanError::Token(TokenError::RedactionFailed))
    );
    assert_eq!(store.stats()?.entries, 0);
    Ok(())
}

#[test]
fn preparation_disabled_runtime_preserves_text_without_minting_tokens() -> Result<(), PlanError> {
    let store = DlpTokenStore::new();
    let mut lease = store.admit("codex", None, 1)?;
    let runtime = SecurityRuntimeConfig {
        enabled: false,
        ..SecurityRuntimeConfig::default()
    };
    let body = json!({"instructions": "13800138000", "model": "synthetic-model"});
    let prepared = prepare_codex_request(
        &mut lease,
        &body,
        &runtime,
        &["chinese_phone_number"],
        &[],
        |_, _, _| Ok(()),
    )?;
    assert!(prepared.body.as_value() == &body);
    assert_eq!(store.stats()?.entries, 0);
    Ok(())
}

#[test]
fn review_preparation_checks_all_existing_lease_dependencies_even_if_history_argument_omits_them(
) -> Result<(), PlanError> {
    let store = DlpTokenStore::new();
    let mut lease = store.admit("codex", None, 1)?;
    lease.prepare(
        &[TokenFragment {
            raw: "synthetic-history-secret",
            whole_span_categories: &[TokenCategory::Custom],
        }],
        &[],
        |_| Ok(()),
    )?;
    let mut runtime = SecurityRuntimeConfig::default();
    runtime.custom_rules.push(rule(
        "synthetic-now-irreversible",
        "history",
        AuditAction::Redact,
    ));
    let result = prepare_codex_request(
        &mut lease,
        &json!({"input": "history"}),
        &runtime,
        &[],
        &[],
        |_, _, _| Ok(()),
    );
    assert_eq!(result.err(), Some(PlanError::PolicyConflict));
    assert_eq!(store.stats()?.entries, 1);
    Ok(())
}

#[test]
fn review_preparation_keeps_zero_width_allow_audit_and_protective_block() -> Result<(), PlanError> {
    let store = DlpTokenStore::new();
    let mut lease = store.admit("codex", None, 1)?;
    for chosen in [AuditAction::Allow, AuditAction::Audit] {
        let runtime = SecurityRuntimeConfig {
            custom_rules: vec![rule("synthetic-zero", "^", chosen)],
            ..SecurityRuntimeConfig::default()
        };
        let prepared = prepare_codex_request(
            &mut lease,
            &json!({"input": "synthetic"}),
            &runtime,
            &["synthetic-zero"],
            &[],
            |_, _, _| Ok(()),
        )?;
        assert!(prepared.body.as_value()["input"] == "synthetic");
    }
    let runtime = SecurityRuntimeConfig {
        custom_rules: vec![rule("synthetic-zero", "^", AuditAction::Block)],
        ..SecurityRuntimeConfig::default()
    };
    let blocked = prepare_codex_request(
        &mut lease,
        &json!({"input": "synthetic"}),
        &runtime,
        &[],
        &[],
        |_, _, _| Ok(()),
    );
    assert_eq!(blocked.err(), Some(PlanError::Blocked));
    Ok(())
}

fn assert_zero_width_block_is_not_exempted(pattern: &str, suffix: &str) -> Result<(), PlanError> {
    let store = DlpTokenStore::new();
    let mut lease = store.admit("codex", None, 1)?;
    let tokens = lease.prepare(
        &[TokenFragment {
            raw: "synthetic-history",
            whole_span_categories: &[TokenCategory::Custom],
        }],
        &[],
        |tokens| Ok(tokens.to_vec()),
    )?;
    let runtime = SecurityRuntimeConfig {
        custom_rules: vec![rule("synthetic-zero-block", pattern, AuditAction::Block)],
        ..SecurityRuntimeConfig::default()
    };
    let mut validated = false;
    let result = prepare_codex_request(
        &mut lease,
        &json!({"input": format!("{}{suffix}", tokens[0])}),
        &runtime,
        &[],
        &[],
        |_, _, _| {
            validated = true;
            Ok(())
        },
    );
    assert_eq!(result.err(), Some(PlanError::Blocked));
    assert!(!validated);
    assert_eq!(store.stats()?.entries, 1);
    Ok(())
}

#[test]
fn review_preparation_zero_width_block_at_authorized_token_start_rejects() -> Result<(), PlanError>
{
    assert_zero_width_block_is_not_exempted("^", " outside")
}

#[test]
fn review_preparation_zero_width_block_at_authorized_token_end_rejects() -> Result<(), PlanError> {
    assert_zero_width_block_is_not_exempted("$", "")
}

#[test]
fn review_preparation_zero_width_block_inside_authorized_token_rejects() -> Result<(), PlanError> {
    assert_zero_width_block_is_not_exempted(r"\B", "")
}

#[test]
fn review_preparation_zero_width_residual_block_rejects_before_commit() -> Result<(), PlanError> {
    let store = DlpTokenStore::new();
    let mut lease = store.admit("codex", None, 1)?;
    let runtime = SecurityRuntimeConfig {
        custom_rules: vec![
            rule("synthetic-restore", "🦀", AuditAction::Redact),
            rule("synthetic-word-boundary", r"\b", AuditAction::Block),
        ],
        ..SecurityRuntimeConfig::default()
    };
    // The original non-word text has no word boundary; its new token does.
    let mut validated = false;
    let result = prepare_codex_request(
        &mut lease,
        &json!({"input": "🦀"}),
        &runtime,
        &["synthetic-restore"],
        &[],
        |_, _, _| {
            validated = true;
            Ok(())
        },
    );
    assert_eq!(
        result.err(),
        Some(PlanError::Token(TokenError::RedactionFailed))
    );
    assert!(!validated);
    assert_eq!(store.stats()?.entries, 0);
    Ok(())
}

#[test]
fn review_preparation_validates_large_history_before_owned_copy() -> Result<(), PlanError> {
    use super::request_preparation::COPY_ATTEMPT_MAX_BYTES;

    let store = DlpTokenStore::new();
    let mut lease = store.admit("codex", None, 1)?;
    let invalid = "x".repeat(2 * 1024 * 1024);
    COPY_ATTEMPT_MAX_BYTES.with(|largest| largest.set(0));
    let mut validated = false;
    let result = prepare_codex_request(
        &mut lease,
        &json!({"input": "ok"}),
        &SecurityRuntimeConfig::default(),
        &[],
        &[&invalid],
        |_, _, _| {
            validated = true;
            Ok(())
        },
    );
    assert_eq!(
        result.err(),
        Some(PlanError::Token(TokenError::ContextUnavailable))
    );
    assert!(!validated);
    assert!(COPY_ATTEMPT_MAX_BYTES.with(|largest| largest.get()) < invalid.len());
    assert_eq!(store.stats()?.entries, 0);
    Ok(())
}

#[test]
fn review_preparation_validates_history_authority_before_owned_copy() -> Result<(), PlanError> {
    use super::request_preparation::COPY_ATTEMPT_MAX_BYTES;

    let store = DlpTokenStore::new();
    let mut lease = store.admit("codex", None, 1)?;
    let invalid = "vary_cu_00000000000000000000_ai";
    COPY_ATTEMPT_MAX_BYTES.with(|largest| largest.set(0));
    let result = prepare_codex_request(
        &mut lease,
        &json!({"input": "ok"}),
        &SecurityRuntimeConfig::default(),
        &[],
        &[invalid],
        |_, _, _| Ok(()),
    );
    assert_eq!(
        result.err(),
        Some(PlanError::Token(TokenError::ContextUnavailable))
    );
    assert!(COPY_ATTEMPT_MAX_BYTES.with(|largest| largest.get()) < invalid.len());
    assert_eq!(store.stats()?.entries, 0);
    Ok(())
}

#[test]
fn serialization_budget_denies_before_validation_or_mapping_commit() -> Result<(), PlanError> {
    let store = DlpTokenStore::new();
    let mut lease = store.admit("codex", None, 1)?;
    let expected_len = br#"{"input":"vary_ph_00000000000000000000_ai"}"#.len();
    for limit in [0, expected_len - 1] {
        let budget = PayloadBudget::with_limit(limit);
        let mut validated = false;
        let result = prepare_codex_request_with_budget(
            &mut lease,
            &json!({"input": "13800138000"}),
            &SecurityRuntimeConfig::default(),
            &["chinese_phone_number"],
            &[],
            budget.clone(),
            |_, _, _| {
                validated = true;
                Ok(())
            },
        );
        assert_eq!(
            result.err(),
            Some(PlanError::Token(TokenError::CapacityExceeded))
        );
        assert!(!validated);
        assert_eq!(store.stats()?.entries, 0);
        assert!(lease.authorized_tokens()?.is_empty());
        assert_eq!(budget.used(), 0);
    }
    Ok(())
}

// Initial and residual carrier snapshots coexist through final validation.
fn phone_carrier_copies() -> usize {
    let (step, leaf, _) = super::request_carriers::metadata_sizes();
    2 * (2 * 128 + 512 + 5 + 4 * step + 5 + 4 * leaf) + 11 + 31
}
fn phone_candidate_charge() -> usize {
    2 * 128 + 512 + 5 + 31
}
// Real capacities coexist with both carrier snapshots until validation returns.
fn one_field_owner_charge(text_bytes: usize, protected: bool) -> usize {
    use super::{
        payload_text::PayloadText,
        request_plan::{PlanField, PlannedField, PlannedSpan, SpanMatch},
    };
    use std::mem::size_of;
    4 * super::request_preparation::logical_field_size()
        + 4 * size_of::<PlanField<'_>>()
        + 4 * size_of::<PlannedField>()
        + 4 * size_of::<PayloadText>()
        + text_bytes
        + if protected {
            4 * size_of::<SpanMatch>()
                + 4 * size_of::<PlannedSpan>()
                + 4 * size_of::<TokenFragment<'_>>()
                + 4 * size_of::<&str>()
        } else {
            0
        }
}
fn phone_owner_charge() -> usize {
    // Store prevalidation scratch remains live through candidate validation.
    let scratch = 4 * std::mem::size_of::<[u8; 10]>()
        + 4 * std::mem::size_of::<Option<super::token_store_owners::SharedAllocation<()>>>()
        + 8 * std::mem::size_of::<usize>()
        + 31
        + 4 * std::mem::size_of::<super::payload_text::PayloadText>();
    one_field_owner_charge(31, true) + scratch
}

fn phone_preparation_peak() -> usize {
    phone_carrier_copies()
        + phone_owner_charge()
        + phone_candidate_charge()
        + 4 * super::request_carriers::metadata_sizes().0
        + 5
}

fn test_json_copy_charge(value: &Value) -> usize {
    128 + match value {
        Value::String(text) => text.len(),
        Value::Number(number) => number.to_string().len(),
        Value::Array(values) => values.iter().map(test_json_copy_charge).sum(),
        Value::Object(fields) => fields
            .iter()
            .map(|(key, value)| 512 + key.len() + test_json_copy_charge(value))
            .sum(),
        _ => 0,
    }
}

#[test]
fn serialization_budget_is_held_before_allocation_validation_and_until_owner_drop(
) -> Result<(), PlanError> {
    let store = DlpTokenStore::new();
    let mut lease = store.admit("codex", None, 1)?;
    let expected_len = br#"{"input":"vary_ph_00000000000000000000_ai"}"#.len();
    let budget = PayloadBudget::with_limit(phone_preparation_peak());
    let allocations = std::rc::Rc::new(std::cell::Cell::new(0));
    let observed = std::rc::Rc::clone(&allocations);
    let _hook = test_support::install(move |event| {
        assert_eq!(
            event.used,
            expected_len + phone_carrier_copies() + phone_candidate_charge() + phone_owner_charge()
        );
        if event.phase == Phase::BeforeAllocation {
            observed.set(observed.get() + 1);
            assert_eq!(event.requested, expected_len);
        }
        Action::Continue
    });
    let prepared = prepare_codex_request_with_budget(
        &mut lease,
        &json!({"input": "13800138000"}),
        &SecurityRuntimeConfig::default(),
        &["chinese_phone_number"],
        &[],
        budget.clone(),
        |candidate, encoded, _| {
            assert_eq!(
                budget.used(),
                expected_len
                    + phone_carrier_copies()
                    + phone_candidate_charge()
                    + phone_owner_charge()
            );
            assert_eq!(encoded.len(), expected_len);
            assert!(serde_json::from_slice::<Value>(encoded).ok().as_ref() == Some(candidate));
            assert_eq!(store.stats()?.entries, 0);
            Ok(())
        },
    )?;
    assert_eq!(allocations.get(), 1);
    assert_eq!(budget.used(), expected_len + phone_candidate_charge());
    assert_eq!(store.stats()?.entries, 1);
    drop(prepared);
    assert_eq!(budget.used(), 0);
    Ok(())
}

#[test]
fn serialization_budget_validation_error_releases_bytes_and_new_mappings() -> Result<(), PlanError>
{
    let store = DlpTokenStore::new();
    let mut lease = store.admit("codex", None, 1)?;
    let budget = PayloadBudget::with_limit(8192);
    let result = prepare_codex_request_with_budget(
        &mut lease,
        &json!({"input": "13800138000"}),
        &SecurityRuntimeConfig::default(),
        &["chinese_phone_number"],
        &[],
        budget.clone(),
        |_, encoded, _| {
            assert_eq!(
                budget.used(),
                encoded.len()
                    + phone_carrier_copies()
                    + phone_candidate_charge()
                    + phone_owner_charge()
            );
            Err::<(), _>(TokenError::Unavailable)
        },
    );
    assert_eq!(
        result.err(),
        Some(PlanError::Token(TokenError::Unavailable))
    );
    assert_eq!(budget.used(), 0);
    assert_eq!(store.stats()?.entries, 0);
    assert!(lease.authorized_tokens()?.is_empty());
    Ok(())
}

#[test]
fn serialization_budget_validation_unwind_releases_bytes_and_mappings() -> Result<(), PlanError> {
    let store = DlpTokenStore::new();
    let mut lease = store.admit("codex", None, 1)?;
    let budget = PayloadBudget::with_limit(8192);
    let reached_synthetic_panic = std::cell::Cell::new(false);
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        prepare_codex_request_with_budget(
            &mut lease,
            &json!({"input": "13800138000"}),
            &SecurityRuntimeConfig::default(),
            &["chinese_phone_number"],
            &[],
            budget.clone(),
            |_, encoded, _| -> Result<(), TokenError> {
                assert_eq!(
                    budget.used(),
                    encoded.len()
                        + phone_carrier_copies()
                        + phone_candidate_charge()
                        + phone_owner_charge()
                );
                reached_synthetic_panic.set(true);
                panic!("synthetic final validation unwind");
            },
        )
    }));
    assert!(reached_synthetic_panic.get());
    assert!(result.is_err());
    assert_eq!(budget.used(), 0);
    assert_eq!(store.stats()?.entries, 0);
    assert!(lease.authorized_tokens()?.is_empty());
    Ok(())
}

#[test]
fn serialization_budget_allocation_failure_releases_bytes_before_commit() -> Result<(), PlanError> {
    let store = DlpTokenStore::new();
    let mut lease = store.admit("codex", None, 1)?;
    let budget = PayloadBudget::with_limit(8192);
    let _hook = test_support::install(|event| {
        assert!(event.used > 0);
        if event.phase == Phase::BeforeAllocation {
            Action::FailAllocation
        } else {
            Action::Continue
        }
    });
    let result = prepare_codex_request_with_budget(
        &mut lease,
        &json!({"input": "13800138000"}),
        &SecurityRuntimeConfig::default(),
        &["chinese_phone_number"],
        &[],
        budget.clone(),
        |_, _, _| Ok(()),
    );
    assert_eq!(
        result.err(),
        Some(PlanError::Token(TokenError::Unavailable))
    );
    assert_eq!(budget.used(), 0);
    assert_eq!(store.stats()?.entries, 0);
    Ok(())
}

#[test]
fn serialization_budget_is_shared_by_distinct_preparations() -> Result<(), PlanError> {
    let store = DlpTokenStore::new();
    let mut first_lease = store.admit("codex", None, 1)?;
    let mut second_lease = store.admit("codex", None, 1)?;
    let expected_len = br#"{"input":"vary_ph_00000000000000000000_ai"}"#.len();
    let budget = PayloadBudget::with_limit(
        expected_len + phone_candidate_charge() + phone_preparation_peak() - 1,
    );
    let first = prepare_codex_request_with_budget(
        &mut first_lease,
        &json!({"input": "13800138000"}),
        &SecurityRuntimeConfig::default(),
        &["chinese_phone_number"],
        &[],
        budget.clone(),
        |_, _, _| Ok(()),
    )?;
    let second = prepare_codex_request_with_budget(
        &mut second_lease,
        &json!({"input": "13900139000"}),
        &SecurityRuntimeConfig::default(),
        &["chinese_phone_number"],
        &[],
        budget.clone(),
        |_, _, _| Ok(()),
    );
    assert_eq!(
        second.err(),
        Some(PlanError::Token(TokenError::CapacityExceeded))
    );
    assert_eq!(store.stats()?.entries, 1);
    assert_eq!(budget.used(), expected_len + phone_candidate_charge());
    drop(first);
    assert_eq!(budget.used(), 0);
    let retry = prepare_codex_request_with_budget(
        &mut second_lease,
        &json!({"input": "13900139000"}),
        &SecurityRuntimeConfig::default(),
        &["chinese_phone_number"],
        &[],
        budget.clone(),
        |_, _, _| Ok(()),
    )?;
    assert_eq!(store.stats()?.entries, 2);
    drop(retry);
    assert_eq!(budget.used(), 0);
    Ok(())
}

#[test]
fn serialization_budget_unpolled_cancellation_releases_owned_bytes() -> Result<(), PlanError> {
    let store = DlpTokenStore::new();
    let mut lease = store.admit("codex", None, 1)?;
    let budget = PayloadBudget::with_limit(8192);
    let prepared = prepare_codex_request_with_budget(
        &mut lease,
        &json!({"input": "13800138000"}),
        &SecurityRuntimeConfig::default(),
        &["chinese_phone_number"],
        &[],
        budget.clone(),
        |_, _, _| Ok(()),
    )?;
    assert!(budget.used() > 0);
    let future = async move {
        std::future::pending::<()>().await;
        drop(prepared);
    };
    drop(future);
    assert_eq!(budget.used(), 0);
    Ok(())
}

#[test]
fn serialization_budget_postallocation_copy_failure_preserves_fixed_error() -> Result<(), PlanError>
{
    let store = DlpTokenStore::new();
    let mut lease = store.admit("codex", None, 1)?;
    let budget = PayloadBudget::with_limit(8192);
    let _hook = test_support::install(|event| {
        if event.phase == Phase::BeforeCopy {
            Action::FailAllocation
        } else {
            Action::Continue
        }
    });
    let result = prepare_codex_request_with_budget(
        &mut lease,
        &json!({"input": "13800138000"}),
        &SecurityRuntimeConfig::default(),
        &["chinese_phone_number"],
        &[],
        budget.clone(),
        |_, _, _| Ok(()),
    );
    assert_eq!(
        result.err(),
        Some(PlanError::Token(TokenError::Unavailable))
    );
    assert_eq!(budget.used(), 0);
    assert_eq!(store.stats()?.entries, 0);
    Ok(())
}

#[test]
fn serialization_budget_discards_conflicted_attempt_before_retrying() -> Result<(), PlanError> {
    let store = DlpTokenStore::new();
    let mut lease = store.admit("codex", Some("synthetic-retry"), 1)?;
    let mut concurrent = store.admit("codex", Some("synthetic-retry"), 1)?;
    let expected_len = br#"{"input":"vary_ph_00000000000000000000_ai"}"#.len();
    let budget = PayloadBudget::with_limit(phone_preparation_peak());
    let mut validations = 0;
    let prepared = prepare_codex_request_with_budget(
        &mut lease,
        &json!({"input": "13800138000"}),
        &SecurityRuntimeConfig::default(),
        &["chinese_phone_number"],
        &[],
        budget.clone(),
        |_, encoded, _| {
            validations += 1;
            assert_eq!(
                budget.used(),
                encoded.len()
                    + phone_carrier_copies()
                    + phone_candidate_charge()
                    + phone_owner_charge()
            );
            if validations == 1 {
                concurrent.prepare(
                    &[TokenFragment {
                        raw: "synthetic-concurrent",
                        whole_span_categories: &[TokenCategory::Custom],
                    }],
                    &[],
                    |_| Ok(()),
                )?;
            }
            Ok(())
        },
    )?;
    assert_eq!(validations, 2);
    assert_eq!(budget.used(), expected_len + phone_candidate_charge());
    assert_eq!(store.stats()?.entries, 2);
    drop(prepared);
    assert_eq!(budget.used(), 0);
    Ok(())
}

#[test]
fn serialization_budget_postvalidation_commit_failure_releases_encoded_owner(
) -> Result<(), PlanError> {
    let store = DlpTokenStore::new();
    let mut lease = store.admit("codex", None, 1)?;
    let budget = PayloadBudget::with_limit(8192);
    store.fail_next_mapping_reserve_for_test();
    let mut validated = false;
    let result = prepare_codex_request_with_budget(
        &mut lease,
        &json!({"input": "13800138000"}),
        &SecurityRuntimeConfig::default(),
        &["chinese_phone_number"],
        &[],
        budget.clone(),
        |_, encoded, _| {
            assert_eq!(
                budget.used(),
                encoded.len()
                    + phone_carrier_copies()
                    + phone_candidate_charge()
                    + phone_owner_charge()
            );
            validated = true;
            Ok(())
        },
    );
    assert!(validated);
    assert_eq!(
        result.err(),
        Some(PlanError::Token(TokenError::CapacityExceeded))
    );
    assert_eq!(budget.used(), 0);
    assert_eq!(store.stats()?.entries, 0);
    assert!(lease.authorized_tokens()?.is_empty());
    Ok(())
}

#[test]
fn serialization_budget_measures_json_escaping_and_utf8_not_logical_text_length(
) -> Result<(), PlanError> {
    let store = DlpTokenStore::new();
    let mut lease = store.admit("codex", None, 1)?;
    let body = json!({"input": "synthetic \"quoted\"\\ \n\t\u{0} 🦀", "temperature": 0.5});
    let expected = serde_json::to_vec(&body).map_err(|_| TokenError::Unavailable)?;
    let profile = super::reversible_carrier_tests::carrier_profile(&body)?;
    let candidate_charge = test_json_copy_charge(&body);
    let owner_charge = one_field_owner_charge(
        body["input"].as_str().ok_or(TokenError::Unavailable)?.len(),
        false,
    );
    let transient_charge = 2 * profile.retained + candidate_charge + owner_charge;
    let budget = PayloadBudget::with_limit(
        (expected.len() + transient_charge)
            .max(profile.retained + candidate_charge + profile.peak + owner_charge),
    );
    let prepared = prepare_codex_request_with_budget(
        &mut lease,
        &body,
        &SecurityRuntimeConfig::default(),
        &[],
        &[],
        budget.clone(),
        |candidate, bytes, proof| {
            assert!(bytes == expected.as_slice());
            assert!(candidate == &body);
            assert!(proof.is_empty());
            assert_eq!(budget.used(), expected.len() + transient_charge);
            Ok(())
        },
    )?;
    assert_eq!(prepared.encoded.capacity(), expected.len());
    drop(prepared);
    assert_eq!(budget.used(), 0);
    Ok(())
}

#[test]
fn serialization_budget_polled_cancellation_releases_owner() -> Result<(), PlanError> {
    use std::{
        future::Future,
        sync::Arc,
        task::{Context, Poll, Wake, Waker},
    };
    struct Noop;
    impl Wake for Noop {
        fn wake(self: Arc<Self>) {}
    }
    let store = DlpTokenStore::new();
    let mut lease = store.admit("codex", None, 1)?;
    let budget = PayloadBudget::with_limit(8192);
    let prepared = prepare_codex_request_with_budget(
        &mut lease,
        &json!({"input": "13800138000"}),
        &SecurityRuntimeConfig::default(),
        &["chinese_phone_number"],
        &[],
        budget.clone(),
        |_, _, _| Ok(()),
    )?;
    let mut future = Box::pin(async move {
        std::future::pending::<()>().await;
        drop(prepared);
    });
    let waker = Waker::from(Arc::new(Noop));
    let mut context = Context::from_waker(&waker);
    assert!(matches!(future.as_mut().poll(&mut context), Poll::Pending));
    assert!(budget.used() > 0);
    drop(future);
    assert_eq!(budget.used(), 0);
    Ok(())
}

#[test]
fn preparation_carrier_copies_share_budget_before_validation_and_mapping_commit(
) -> Result<(), PlanError> {
    use super::payload_json::test_support::{
        self as json_support, Action as JsonAction, Phase as JsonPhase,
    };
    let store = DlpTokenStore::new();
    let mut lease = store.admit("codex", None, 1)?;
    let body = json!({"input":"13800138000"});
    let initial_charge = test_json_copy_charge(&body);
    let initial_retained = super::reversible_carrier_tests::carrier_profile(&body)?.retained;
    let residual = test_json_copy_charge(&json!({"input":"x".repeat(31)}));
    let budget =
        PayloadBudget::with_limit(initial_retained + 2 * residual + phone_owner_charge() - 1);
    let builds = std::rc::Rc::new(std::cell::Cell::new(0));
    let observed = std::rc::Rc::clone(&builds);
    let _hook = json_support::install(move |event| {
        if event.phase == JsonPhase::BeforeBuild {
            observed.set(observed.get() + 1);
            if observed.get() == 1 {
                assert_eq!(event.used, initial_charge);
            }
        }
        JsonAction::Continue
    });
    let mut validated = false;
    let result = prepare_codex_request_with_budget(
        &mut lease,
        &body,
        &SecurityRuntimeConfig::default(),
        &["chinese_phone_number"],
        &[],
        budget.clone(),
        |_, _, _| {
            validated = true;
            Ok(())
        },
    );
    assert_eq!(builds.get(), 2);
    assert_eq!(
        result.err(),
        Some(PlanError::Token(TokenError::CapacityExceeded))
    );
    assert!(!validated);
    assert_eq!(budget.used(), 0);
    assert_eq!(store.stats()?.entries, 0);
    assert!(lease.authorized_tokens()?.is_empty());
    Ok(())
}

#[test]
fn preparation_carrier_copy_failures_do_not_commit_mappings() -> Result<(), PlanError> {
    use super::payload_json::test_support::{
        self as json_support, Action as JsonAction, Phase as JsonPhase,
    };
    for fail_at in [1, 2] {
        let store = DlpTokenStore::new();
        let mut lease = store.admit("codex", None, 1)?;
        let budget = PayloadBudget::with_limit(8192);
        let hits = std::rc::Rc::new(std::cell::Cell::new(0));
        let observed = std::rc::Rc::clone(&hits);
        let _hook = json_support::install(move |event| {
            if event.phase == JsonPhase::AfterBuild {
                observed.set(observed.get() + 1);
                if observed.get() == fail_at {
                    return JsonAction::Fail;
                }
            }
            JsonAction::Continue
        });
        let result = prepare_codex_request_with_budget(
            &mut lease,
            &json!({"input":"13800138000"}),
            &SecurityRuntimeConfig::default(),
            &["chinese_phone_number"],
            &[],
            budget.clone(),
            |_, _, _| Ok(()),
        );
        assert_eq!(hits.get(), fail_at);
        assert_eq!(
            result.err(),
            Some(PlanError::Token(TokenError::Unavailable))
        );
        assert_eq!(budget.used(), 0);
        assert_eq!(store.stats()?.entries, 0);
        assert!(lease.authorized_tokens()?.is_empty());
    }
    Ok(())
}

struct DecodedPreparationFixture {
    body: Value,
    parse_peaks: [usize; 2],
    retained: usize,
    peak: usize,
    first_retained: usize,
    candidate_charge: usize,
}

fn decoded_preparation_fixture() -> Result<DecodedPreparationFixture, TokenError> {
    let initial = r#"{"value":"13800138000"}"#;
    let replacement = format!(r#"{{"value":"{}"}}"#, "x".repeat(31));
    let body = json!({"input":[{"type":"function_call", "arguments":initial}]});
    let candidate = json!({"input":[{"type":"function_call", "arguments":replacement}]});
    let initial_profile = super::reversible_carrier_tests::carrier_profile(&body)?;
    let final_profile = super::reversible_carrier_tests::carrier_profile(&candidate)?;
    let candidate_charge = test_json_copy_charge(&candidate);
    // The decoded value and read-only function type share four-slot metadata capacities.
    let owner_charge = phone_owner_charge() + "function_call".len();
    let rewrite_peak = initial_profile.retained
        + owner_charge
        + candidate_charge
        + replacement.len()
        + 4 * std::mem::size_of::<(usize, &str)>()
        + 4 * std::mem::size_of::<(usize, super::payload_buffer::PayloadBuffer)>()
        + 4 * std::mem::size_of::<super::payload_json::JsonStringReplacement<'_>>();
    Ok(DecodedPreparationFixture {
        body,
        parse_peaks: [
            initial_profile.parses[0],
            initial_profile.retained + owner_charge + candidate_charge + final_profile.parses[0],
        ],
        retained: initial_profile.retained
            + owner_charge
            + candidate_charge
            + final_profile.retained,
        peak: initial_profile
            .peak
            .max(initial_profile.retained + owner_charge + candidate_charge + final_profile.peak)
            .max(rewrite_peak),
        first_retained: initial.len() + 3 * 512 + 1024,
        candidate_charge,
    })
}

#[test]
fn preparation_decoded_peak_exhaustion_precedes_validation_and_mapping_commit(
) -> Result<(), PlanError> {
    use super::payload_json::test_support::{self, Action, Phase};
    let fixture = decoded_preparation_fixture()?;
    let limit = fixture.parse_peaks[1] - 1;
    let parses = std::rc::Rc::new(std::cell::Cell::new(0));
    let observed = std::rc::Rc::clone(&parses);
    let _hook = test_support::install(move |event| {
        if event.phase == Phase::BeforeParse {
            observed.set(observed.get() + 1);
        }
        Action::Continue
    });
    let store = DlpTokenStore::new();
    let mut lease = store.admit("codex", None, 1)?;
    let budget = PayloadBudget::with_limit(limit);
    let mut validated = false;
    let result = prepare_codex_request_with_budget(
        &mut lease,
        &fixture.body,
        &SecurityRuntimeConfig::default(),
        &["chinese_phone_number"],
        &[],
        budget.clone(),
        |_, _, _| {
            validated = true;
            Ok(())
        },
    );
    assert_eq!(
        result.err(),
        Some(PlanError::Token(TokenError::CapacityExceeded))
    );
    assert_eq!(parses.get(), 1);
    assert!(!validated);
    assert_eq!(budget.used(), 0);
    assert_eq!(store.stats()?.entries, 0);
    assert!(lease.authorized_tokens()?.is_empty());
    Ok(())
}

#[test]
fn preparation_decoded_initial_and_residual_owners_share_bytes_budget_until_validation(
) -> Result<(), PlanError> {
    use super::payload_json::test_support::{self, Action, Phase};
    let fixture = decoded_preparation_fixture()?;
    let parse_peaks = fixture.parse_peaks;
    let first_retained = fixture.first_retained;
    let parses = std::rc::Rc::new(std::cell::Cell::new(0));
    let observed = std::rc::Rc::clone(&parses);
    let _hook = test_support::install(move |event| {
        if event.phase == Phase::AfterParse {
            observed.set(observed.get() + 1);
            if observed.get() == 1 {
                assert_eq!(event.charged, first_retained);
                assert_eq!(event.used, parse_peaks[0]);
            } else {
                assert_eq!(event.used, parse_peaks[1]);
            }
        }
        Action::Continue
    });
    let store = DlpTokenStore::new();
    let mut lease = store.admit("codex", None, 1)?;
    let budget = PayloadBudget::with_limit(fixture.peak);
    let prepared = prepare_codex_request_with_budget(
        &mut lease,
        &fixture.body,
        &SecurityRuntimeConfig::default(),
        &["chinese_phone_number"],
        &[],
        budget.clone(),
        |_, bytes, proof| {
            assert_eq!(proof.len(), 1);
            assert_eq!(budget.used(), fixture.retained + bytes.len());
            Ok(())
        },
    )?;
    assert_eq!(parses.get(), 2);
    assert_eq!(prepared.reversible_occurrences, 1);
    assert_eq!(
        budget.used(),
        fixture.candidate_charge + prepared.encoded.capacity()
    );
    assert_eq!(store.stats()?.entries, 1);
    drop(prepared);
    assert_eq!(budget.used(), 0);
    Ok(())
}

#[test]
fn preparation_candidate_owner_retains_tree_plus_encoded_until_separate_drops(
) -> Result<(), PlanError> {
    let store = DlpTokenStore::new();
    let mut lease = store.admit("codex", None, 1)?;
    let budget = PayloadBudget::with_limit(64 * 1024);
    let prepared = prepare_codex_request_with_budget(
        &mut lease,
        &json!({"input":"13800138000"}),
        &SecurityRuntimeConfig::default(),
        &["chinese_phone_number"],
        &[],
        budget.clone(),
        |_, _, _| Ok(()),
    )?;
    const FINAL: usize = 2 * 128 + 512 + 5 + 31;
    assert_eq!(budget.used(), FINAL + prepared.encoded.len());
    let DlpPreparedRequest { body, encoded, .. } = prepared;
    drop(encoded);
    assert_eq!(budget.used(), FINAL);
    drop(body);
    assert_eq!(budget.used(), 0);
    assert_eq!(store.stats()?.entries, 1);
    Ok(())
}

#[test]
fn preparation_candidate_owner_is_charged_at_validation_without_plaintext_recopies(
) -> Result<(), PlanError> {
    let store = DlpTokenStore::new();
    let mut lease = store.admit("codex", None, 1)?;
    let budget = PayloadBudget::with_limit(64 * 1024);
    super::request_preparation::COPY_ATTEMPT_MAX_BYTES.with(|size| size.set(0));
    let prepared = prepare_codex_request_with_budget(
        &mut lease,
        &json!({"input":"13800138000"}),
        &SecurityRuntimeConfig::default(),
        &["chinese_phone_number"],
        &[],
        budget.clone(),
        |_, encoded, _| {
            assert_eq!(
                budget.used(),
                phone_carrier_copies()
                    + (2 * 128 + 512 + 5 + 31)
                    + encoded.len()
                    + phone_owner_charge()
            );
            assert_eq!(store.stats()?.entries, 0);
            Ok(())
        },
    )?;
    assert_eq!(
        super::request_preparation::COPY_ATTEMPT_MAX_BYTES.with(std::cell::Cell::get),
        0
    );
    drop(prepared);
    assert_eq!(budget.used(), 0);
    Ok(())
}

#[test]
fn preparation_candidate_owner_build_failure_precedes_validation_and_mapping_commit(
) -> Result<(), PlanError> {
    use super::payload_json::test_support as json_support;
    let store = DlpTokenStore::new();
    let mut lease = store.admit("codex", None, 1)?;
    let budget = PayloadBudget::with_limit(64 * 1024);
    let builds = std::rc::Rc::new(std::cell::Cell::new(0));
    let observed = std::rc::Rc::clone(&builds);
    let _hook = json_support::install(move |event| {
        if event.phase == json_support::Phase::BeforeBuild {
            observed.set(observed.get() + 1);
            // Original snapshot builds first, replacement candidate must be second.
            if observed.get() == 2 {
                return json_support::Action::Fail;
            }
        }
        json_support::Action::Continue
    });
    let validated = std::cell::Cell::new(false);
    let result = prepare_codex_request_with_budget(
        &mut lease,
        &json!({"input":"13800138000"}),
        &SecurityRuntimeConfig::default(),
        &["chinese_phone_number"],
        &[],
        budget.clone(),
        |_, _, _| {
            validated.set(true);
            Ok(())
        },
    );
    assert_eq!(
        result.err(),
        Some(PlanError::Token(TokenError::Unavailable))
    );
    assert_eq!(builds.get(), 2);
    assert!(!validated.get());
    assert_eq!(budget.used(), 0);
    assert_eq!(store.stats()?.entries, 0);
    assert!(lease.authorized_tokens()?.is_empty());
    Ok(())
}

#[test]
fn preparation_owner_borrows_carrier_text_without_logical_copy() -> Result<(), PlanError> {
    use super::request_preparation::COPY_ATTEMPT_MAX_BYTES;
    let store = DlpTokenStore::new();
    let mut lease = store.admit("codex", None, 1)?;
    COPY_ATTEMPT_MAX_BYTES.with(|size| size.set(0));
    let prepared = prepare_codex_request(
        &mut lease,
        &json!({"input": "synthetic ordinary text"}),
        &SecurityRuntimeConfig {
            enabled: false,
            ..Default::default()
        },
        &[],
        &[],
        |_, _, _| Ok(()),
    )?;
    assert_eq!(COPY_ATTEMPT_MAX_BYTES.with(|size| size.get()), 0);
    drop(prepared);
    Ok(())
}

#[test]
fn preparation_owner_candidate_uses_request_budget() -> Result<(), PlanError> {
    use super::payload_text::test_support::{
        self as text_support, Action as TextAction, Phase as TextPhase,
    };
    let store = DlpTokenStore::new();
    let mut lease = store.admit("codex", None, 1)?;
    let budget = PayloadBudget::with_limit(1 << 20);
    let shared = budget.clone();
    let seen = std::rc::Rc::new(std::cell::Cell::new(false));
    let observed = seen.clone();
    let hook = text_support::install(move |event| {
        if event.phase == TextPhase::BeforeAllocation && event.bytes == 31 {
            assert_eq!(event.used, shared.used());
            observed.set(true);
        }
        TextAction::Continue
    });
    let prepared = prepare_codex_request_with_budget(
        &mut lease,
        &json!({"input": "13800138000"}),
        &SecurityRuntimeConfig::default(),
        &["chinese_phone_number"],
        &[],
        budget.clone(),
        |_, _, _| Ok(()),
    )?;
    drop(hook);
    assert!(seen.get());
    drop(prepared);
    assert_eq!(budget.used(), 0);
    Ok(())
}

#[test]
fn preparation_owner_history_snapshot_is_charged_and_failure_atomic() -> Result<(), PlanError> {
    use super::payload_text::test_support::{
        self as text_support, Action as TextAction, Phase as TextPhase,
    };
    let store = DlpTokenStore::new();
    let mut lease = store.admit("codex", None, 1)?;
    let tokens = lease.prepare(
        &[TokenFragment {
            raw: "synthetic-secret",
            whole_span_categories: &[TokenCategory::Custom],
        }],
        &[],
        |tokens| Ok(tokens.to_vec()),
    )?;
    let budget = PayloadBudget::with_limit(1 << 20);
    let hook = text_support::install(|event| {
        if event.phase == TextPhase::BeforeCopy && event.bytes == 31 {
            return TextAction::Fail;
        }
        TextAction::Continue
    });
    let mut validated = false;
    let result = prepare_codex_request_with_budget(
        &mut lease,
        &json!({"input": "ok"}),
        &SecurityRuntimeConfig {
            enabled: false,
            ..Default::default()
        },
        &[],
        &[&tokens[0]],
        budget.clone(),
        |_, _, _| {
            validated = true;
            Ok(())
        },
    );
    drop(hook);
    assert_eq!(
        result.err(),
        Some(PlanError::Token(TokenError::Unavailable))
    );
    assert!(!validated);
    assert_eq!(budget.used(), 0);
    assert_eq!(store.stats()?.entries, 1);
    assert_eq!(lease.resolve(&tokens[0])?, "synthetic-secret");
    Ok(())
}

#[test]
fn preparation_owner_repeated_history_has_one_charged_snapshot_and_exact_input_limit(
) -> Result<(), PlanError> {
    use super::payload_text::{
        test_support::{self as text_support, Action as TextAction, Phase as TextPhase},
        PayloadText,
    };
    let store = DlpTokenStore::new();
    let mut lease = store.admit("codex", None, 1)?;
    let tokens = lease.prepare(
        &[TokenFragment {
            raw: "synthetic-secret",
            whole_span_categories: &[TokenCategory::Custom],
        }],
        &[],
        |tokens| Ok(tokens.to_vec()),
    )?;
    let history = vec![tokens[0].as_str(); 4097];
    let budget = PayloadBudget::with_limit(64 * 1024);
    let copies = std::rc::Rc::new(std::cell::Cell::new(0));
    let observed = copies.clone();
    let shared = budget.clone();
    let hook = text_support::install(move |event| {
        if event.phase == TextPhase::BeforeCopy && event.bytes == 31 {
            observed.set(observed.get() + 1);
            assert_eq!(shared.used(), event.used);
        }
        TextAction::Continue
    });
    let prepared = prepare_codex_request_with_budget(
        &mut lease,
        &json!({}),
        &SecurityRuntimeConfig::default(),
        &[],
        &history[..4096],
        budget.clone(),
        |_, _, proof| {
            assert_eq!(proof, [tokens[0].as_str()]);
            // Three empty JSON snapshots, one dependency snapshot, and history/proven/proof capacities.
            assert_eq!(
                budget.used(),
                3 * 128
                    + 4 * std::mem::size_of::<PayloadText>()
                    + 31
                    + 12 * std::mem::size_of::<&str>()
                    + 2
            );
            Ok(())
        },
    )?;
    assert_eq!(copies.get(), 1);
    drop(prepared);
    assert_eq!(budget.used(), 0);
    let result = prepare_codex_request_with_budget(
        &mut lease,
        &json!({}),
        &SecurityRuntimeConfig::default(),
        &[],
        &history,
        budget.clone(),
        |_, _, _| Ok(()),
    );
    assert_eq!(
        result.err(),
        Some(PlanError::Token(TokenError::CapacityExceeded))
    );
    assert_eq!(copies.get(), 1);
    assert_eq!(budget.used(), 0);
    assert_eq!(store.stats()?.entries, 1);
    drop(hook);
    Ok(())
}

#[test]
fn preparation_owner_all_metadata_and_text_failure_points_release_partial_owners(
) -> Result<(), PlanError> {
    use super::{payload_metadata::test_support as metadata, payload_text::test_support as text};
    // Each sweep starts from one valid pinned historical dependency and one new fragment.
    for text_hook in [false, true] {
        let calls = std::rc::Rc::new(std::cell::Cell::new(0));
        let run = |fail_at: usize, unwind: bool| -> Result<usize, PlanError> {
            let store = DlpTokenStore::new();
            let mut lease = store.admit("codex", None, 1)?;
            let history = lease.prepare(
                &[TokenFragment {
                    raw: "synthetic-secret",
                    whole_span_categories: &[TokenCategory::Custom],
                }],
                &[],
                |tokens| Ok(tokens.to_vec()),
            )?;
            let budget = PayloadBudget::with_limit(1 << 20);
            calls.set(0);
            let seen = calls.clone();
            let metadata_guard = (!text_hook).then(|| {
                metadata::install(move |_| {
                    seen.set(seen.get() + 1);
                    if seen.get() == fail_at {
                        if unwind {
                            panic!("synthetic preparation metadata unwind");
                        }
                        return metadata::Action::Fail;
                    }
                    metadata::Action::Continue
                })
            });
            let seen = calls.clone();
            let text_guard = text_hook.then(|| {
                text::install(move |_| {
                    seen.set(seen.get() + 1);
                    if seen.get() == fail_at {
                        if unwind {
                            panic!("synthetic preparation text unwind");
                        }
                        return text::Action::Fail;
                    }
                    text::Action::Continue
                })
            });
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                prepare_codex_request_with_budget(
                    &mut lease,
                    &json!({"input":"13800138000"}),
                    &SecurityRuntimeConfig::default(),
                    &["chinese_phone_number"],
                    &[],
                    budget.clone(),
                    |_, _, _| Ok(()),
                )
            }));
            drop(metadata_guard);
            drop(text_guard);
            if fail_at == 0 {
                let prepared = result.map_err(|_| TokenError::Unavailable)??;
                assert_eq!(store.stats()?.entries, 2);
                drop(prepared);
            } else {
                if unwind {
                    assert!(result.is_err());
                } else {
                    assert_eq!(
                        result.ok().and_then(Result::err),
                        Some(PlanError::Token(TokenError::Unavailable))
                    );
                }
                assert_eq!(store.stats()?.entries, 1);
                assert_eq!(lease.authorized_tokens()?.len(), 1);
                assert_eq!(lease.resolve(&history[0])?, "synthetic-secret");
            }
            assert_eq!(budget.used(), 0);
            Ok(calls.get())
        };
        let points = run(0, false)?;
        assert!(points > 0);
        for fail_at in 1..=points {
            run(fail_at, false)?;
            run(fail_at, true)?;
        }
    }
    Ok(())
}

fn detector_attempt_uses_request_budget_and_rolls_back(stop: usize) -> Result<(), PlanError> {
    use std::{cell::Cell, rc::Rc};
    let budget = PayloadBudget::with_limit(1024 * 1024);
    let store = DlpTokenStore::new();
    let mut lease = store.admit("codex", None, 1)?;
    let mut runtime = SecurityRuntimeConfig::default();
    runtime
        .custom_rules
        .push(rule("synthetic-allow", "synthetic", AuditAction::Allow));
    let visits = Rc::new(Cell::new(0));
    let shared = Rc::new(Cell::new(true));
    let observed_visits = visits.clone();
    let observed_shared = shared.clone();
    let observed_budget = budget.clone();
    let _hook = super::request_detection::test_support::install(move |used| {
        observed_visits.set(observed_visits.get() + 1);
        observed_shared.set(observed_shared.get() && used == observed_budget.used() && used > 0);
        if observed_visits.get() == stop {
            return Err(TokenError::Unavailable);
        }
        Ok(())
    });
    let result = prepare_codex_request_with_budget(
        &mut lease,
        &json!({"input":"13800138000 synthetic"}),
        &runtime,
        &["chinese_phone_number"],
        &[],
        budget.clone(),
        |_, _, _| -> Result<(), TokenError> {
            panic!("synthetic callback must not run after interrupted detection")
        },
    );
    assert_eq!(
        result.err(),
        Some(PlanError::Token(TokenError::Unavailable))
    );
    assert_eq!(visits.get(), stop);
    assert!(
        shared.get(),
        "each detection must borrow the original request budget"
    );
    assert_eq!(budget.used(), 0);
    assert_eq!(store.stats()?.entries, 0);
    assert!(lease.authorized_tokens()?.is_empty());
    Ok(())
}

#[test]
fn detector_initial_attempt_shares_request_budget_and_rolls_back() -> Result<(), PlanError> {
    detector_attempt_uses_request_budget_and_rolls_back(1)
}

#[test]
fn detector_final_attempt_shares_request_budget_and_rolls_back() -> Result<(), PlanError> {
    detector_attempt_uses_request_budget_and_rolls_back(3)
}

#[test]
fn detector_capacity_failures_in_both_rounds_preserve_existing_authority() -> Result<(), PlanError>
{
    use std::{
        cell::{Cell, RefCell},
        rc::Rc,
    };
    // First round: phone, allow keyword. Candidate round: only allow keyword.
    for stop in [1, 3] {
        let budget = PayloadBudget::with_limit(1024 * 1024);
        let store = DlpTokenStore::new();
        let mut lease = store.admit("codex", None, 1)?;
        let token = lease.prepare(
            &[TokenFragment {
                raw: "synthetic-pinned",
                whole_span_categories: &[TokenCategory::Custom],
            }],
            &[],
            |tokens| Ok(tokens[0].clone()),
        )?;
        let mut runtime = SecurityRuntimeConfig::default();
        runtime
            .custom_rules
            .push(rule("synthetic-allow", "synthetic", AuditAction::Allow));
        let visits = Rc::new(Cell::new(0));
        let shared_visits = visits.clone();
        let held = Rc::new(RefCell::new(None));
        let shared_held = held.clone();
        let shared_budget = budget.clone();
        let _hook = super::request_detection::test_support::install(move |used| {
            shared_visits.set(shared_visits.get() + 1);
            assert_eq!(used, shared_budget.used());
            if shared_visits.get() == stop {
                // Exhaust the same request budget immediately before result growth.
                *shared_held.borrow_mut() =
                    Some(shared_budget.reserve(shared_budget.limit() - used)?);
            }
            Ok(())
        });
        let result = prepare_codex_request_with_budget(
            &mut lease,
            &json!({"input":"13800138000 synthetic"}),
            &runtime,
            &["chinese_phone_number"],
            &[],
            budget.clone(),
            |_, _, _| -> Result<(), TokenError> {
                panic!("synthetic capacity failure must precede validation")
            },
        );
        assert_eq!(
            result.err(),
            Some(PlanError::Token(TokenError::CapacityExceeded))
        );
        assert_eq!(visits.get(), stop);
        assert_eq!(store.stats()?.entries, 1);
        assert_eq!(lease.resolve(&token)?, "synthetic-pinned");
        assert_eq!(lease.authorized_tokens()?.len(), 1);
        drop(held.borrow_mut().take());
        assert_eq!(budget.used(), 0);
    }
    Ok(())
}

#[test]
fn keyword_missing_compiled_snapshot_rejects_preparation_before_validation_or_commit(
) -> Result<(), PlanError> {
    let store = DlpTokenStore::new();
    let mut lease = store.admit("codex", None, 1)?;
    let prior = lease.prepare(
        &[TokenFragment {
            raw: "synthetic-retained",
            whole_span_categories: &[TokenCategory::Custom],
        }],
        &[],
        |tokens| Ok(tokens[0].clone()),
    )?;
    let before = store.stats()?;
    let runtime = SecurityRuntimeConfig {
        custom_rules: vec![CompiledCustomRule {
            rule_type: "keyword".into(),
            regex: None,
            ..rule(
                "synthetic-missing-keyword",
                "synthetic-no-match",
                AuditAction::Redact,
            )
        }],
        ..Default::default()
    };
    let budget = PayloadBudget::with_limit(64 * 1024 * 1024);
    let mut validated = false;
    let result = prepare_codex_request_with_budget(
        &mut lease,
        &json!({"input": "13800138000"}),
        &runtime,
        &["chinese_phone_number"],
        &[],
        budget.clone(),
        |_, _, _| {
            validated = true;
            Ok(())
        },
    );
    assert_eq!(
        result.err(),
        Some(PlanError::Token(TokenError::RedactionFailed))
    );
    assert!(!validated);
    assert_eq!(store.stats()?, before);
    assert_eq!(budget.used(), 0);
    assert_eq!(lease.authorized_tokens()?.len(), 1);
    assert!(lease.resolve(&prior)? == "synthetic-retained");
    Ok(())
}

#[test]
fn keyword_loaded_snapshot_preparation_rechecks_residuals_under_original_budget(
) -> Result<(), Box<dyn std::error::Error>> {
    use super::{restoration_policy::DlpPolicyGate, runtime::reload_with_gate};
    use std::{
        cell::{Cell, RefCell},
        rc::Rc,
    };
    let db = crate::database::Database::memory()?;
    for (id, pattern, action) in [
        ("synthetic-keyword-restore", "K.Σ", "redact"),
        ("synthetic-keyword-observer", "vary", "audit"),
    ] {
        db.save_custom_security_rule(&crate::database::CustomSecurityRule {
            id: id.into(),
            name: "Synthetic keyword".into(),
            rule_type: "keyword".into(),
            pattern: pattern.into(),
            risk_level: "MEDIUM".into(),
            action: action.into(),
            enabled: true,
            description: None,
            updated_at: 1,
        })?;
    }
    let gate = DlpPolicyGate::new();
    reload_with_gate(&db, &gate)?;
    let snapshot = gate.admit()?;
    for deny_residual in [false, true] {
        let store = DlpTokenStore::new();
        let mut lease = store.admit("codex", None, snapshot.restoration_policy_revision)?;
        let before = store.stats()?;
        let budget = PayloadBudget::with_limit(64 * 1024 * 1024);
        let detections = Rc::new(Cell::new(0));
        let observed = detections.clone();
        let shared = budget.clone();
        let blocker = Rc::new(RefCell::new(None));
        let blocker_hook = blocker.clone();
        let guard = super::request_detection::test_support::install(move |used| {
            assert_eq!(shared.used(), used);
            let pass = observed.get() + 1;
            observed.set(pass);
            if pass == 2 && deny_residual {
                // Exhaust the same owner before the residual result grows.
                *blocker_hook.borrow_mut() = Some(shared.reserve(shared.limit() - used)?);
            }
            Ok(())
        });
        let mut validated = false;
        let result = prepare_codex_request_with_budget(
            &mut lease,
            &json!({"input": "尾 k.ς"}),
            &snapshot.runtime,
            &["synthetic-keyword-restore"],
            &[],
            budget.clone(),
            |candidate, _, proof| {
                validated = true;
                assert_eq!(proof.len(), 1);
                assert!(candidate["input"]
                    .as_str()
                    .is_some_and(|text| text.starts_with("尾 vary_cu_")));
                assert_eq!(store.stats()?.entries, 0);
                Ok(())
            },
        );
        drop(guard);
        assert_eq!(detections.get(), 2);
        drop(blocker.borrow_mut().take());
        if deny_residual {
            assert_eq!(
                result.err(),
                Some(PlanError::Token(TokenError::CapacityExceeded))
            );
            assert!(!validated);
            assert_eq!(store.stats()?, before);
            assert!(lease.authorized_tokens()?.is_empty());
        } else {
            let prepared = result.map_err(|_| "synthetic keyword preparation failed")?;
            assert!(validated);
            assert_eq!(prepared.reversible_occurrences, 1);
            assert_eq!(store.stats()?.entries, 1);
            assert!(budget.used() > 0);
            drop(prepared);
        }
        assert_eq!(budget.used(), 0);
    }
    Ok(())
}
