//! Synthetic native request contracts; no production adapter or route is enabled.
use std::{cell::Cell, rc::Rc};

use serde_json::{json, Value};

use super::{
    payload_budget::PayloadBudget,
    payload_buffer::test_support as buffer_events,
    payload_json::test_support as json_events,
    request_plan::PlanError,
    request_preparation::{prepare_codex_request_with_budget, prepare_observed_request},
    restoration_contract::{
        ClientProtocol, ContinuityScope, EditConstraint, HistoryClaim, IngressNamespace,
        RequestObservation, SourceClass,
    },
    runtime::{CompiledCustomRule, SecurityRuntimeConfig},
    tokenization::{DlpRequestLease, DlpTokenStore, TokenCategory, TokenError, TokenFragment},
    types::{AuditAction, RiskLevel},
};
use crate::proxy::protocol_bridge::Protocol;

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;
const CONTACT: &str = "13900000000";
const RESTORE_RULES: &[&str] = &["chinese_phone_number"];
const BUDGET_BYTES: usize = 4 * 1024 * 1024;

fn native_lease(store: &DlpTokenStore) -> Result<DlpRequestLease, TokenError> {
    store.admit_scoped_published_policy(
        ContinuityScope::Native {
            client: ClientProtocol::new(Protocol::Chat),
            ingress: IngressNamespace::Direct,
        },
        "synthetic-c5-app",
        Some("synthetic-c5-session"),
        1,
    )
}

fn observation<'a>(
    native: &'a Value,
    budget: &PayloadBudget,
) -> TestResult<RequestObservation<'a>> {
    Ok(RequestObservation::new(
        native,
        ClientProtocol::new(Protocol::Chat),
        budget.clone(),
    )?)
}

fn custom_runtime(pattern: &str, action: AuditAction) -> TestResult<SecurityRuntimeConfig> {
    Ok(SecurityRuntimeConfig {
        custom_rules: vec![CompiledCustomRule {
            id: "synthetic-c5-rule".into(),
            name: "Synthetic C5 rule".into(),
            rule_type: "regex".into(),
            pattern: pattern.into(),
            regex: Some(regex::Regex::new(pattern)?),
            risk_level: RiskLevel::High,
            action,
            enabled: true,
        }],
        ..Default::default()
    })
}

#[test]
fn c5_request_observation_owns_budgeted_borrowed_metadata_without_authority() -> TestResult {
    let body = json!({"messages":[
        {"role":"assistant", "tool_calls":[{"id":"synthetic-call", "type":"function",
            "function":{"name":"synthetic-tool", "arguments":"{}"}}]},
        {"role":"tool", "tool_call_id":"synthetic-call", "content":"Synthetic result"}
    ], "opaque":{"synthetic":true}});
    let budget = PayloadBudget::with_limit(BUDGET_BYTES);
    let store = DlpTokenStore::new();
    let lease = native_lease(&store)?;
    let before = store.stats()?;
    let mut observed = observation(&body, &budget)?;
    observed.observe_text(
        &body["messages"][1]["content"],
        SourceClass::ToolResult,
        EditConstraint::Editable,
    )?;
    let site_charge = budget.used();
    observed.claim_history(HistoryClaim::AssistantItem {
        native: &body["messages"][0],
    })?;
    observed.claim_history(HistoryClaim::CallResultPair {
        call: &body["messages"][0]["tool_calls"][0],
        result: &body["messages"][1],
    })?;
    observed.claim_history(HistoryClaim::OpaqueItem {
        native: &body["opaque"],
        source: SourceClass::HistoricalText,
    })?;
    assert!(std::ptr::eq(observed.native(), &body), "native_owner_lost");
    assert!(
        observed.client().protocol() == Protocol::Chat,
        "client_changed"
    );
    assert!(
        site_charge > 0 && budget.used() > site_charge,
        "metadata_not_charged"
    );
    assert!(observed.history().len() == 3, "history_claims_lost");
    assert!(
        matches!(observed.history()[1], HistoryClaim::CallResultPair { call, result }
        if std::ptr::eq(call, &body["messages"][0]["tool_calls"][0])
            && std::ptr::eq(result, &body["messages"][1])),
        "native_claim_location_changed"
    );
    let site = observed.sites().site(0).ok_or("synthetic_site_missing")?;
    assert!(
        site.source() == SourceClass::ToolResult,
        "source_class_lost"
    );
    assert!(
        lease.is_empty() && store.stats()? == before,
        "claim_granted_authority"
    );
    assert!(
        observed.budget().used() == budget.used(),
        "budget_not_shared"
    );
    drop(observed);
    assert!(budget.used() == 0, "metadata_charge_retained");
    Ok(())
}

#[test]
fn c5_request_observation_metadata_denial_refunds_without_copying() -> TestResult {
    let body = json!({"messages":[{"role":"user", "content":CONTACT}]});
    let budget = PayloadBudget::with_limit(1);
    let mut observed = observation(&body, &budget)?;
    assert!(
        matches!(
            observed.observe_text(
                &body["messages"][0]["content"],
                SourceClass::CurrentInput,
                EditConstraint::Editable
            ),
            Err(super::restoration_contract::ContractError::Resource(
                TokenError::CapacityExceeded
            ))
        ),
        "site_metadata_capacity_not_enforced"
    );
    assert!(
        matches!(
            observed.claim_history(HistoryClaim::AssistantItem {
                native: &body["messages"][0]
            }),
            Err(super::restoration_contract::ContractError::Resource(
                TokenError::CapacityExceeded
            ))
        ),
        "history_metadata_capacity_not_enforced"
    );
    assert!(budget.used() == 0, "denied_metadata_charge_retained");
    Ok(())
}

#[test]
fn c5_request_native_chat_messages_transaction_patches_only_actual_sites() -> TestResult {
    let body = json!({"model":"synthetic-model", "temperature":0.25, "stream":false,
        "messages":[
            {"role":"user", "content":CONTACT},
            {"role":"assistant", "tool_calls":[{"id":"synthetic-call", "type":"function",
                "function":{"name":"synthetic-tool", "arguments":"{\"synthetic\":true}"}}]},
            {"role":"tool", "tool_call_id":"synthetic-call", "content":CONTACT},
            {"role":"user", "content":[{"type":"text", "text":CONTACT},
                {"type":"image_url", "image_url":{"url":"https://synthetic.invalid/image"}}]}
        ], "unknown":{"same_value":CONTACT, "types":[true, null, 7]}});
    let original = body.clone();
    let store = DlpTokenStore::new();
    let mut lease = native_lease(&store)?;
    let before = store.stats()?;
    let budget = PayloadBudget::with_limit(BUDGET_BYTES);
    let mut observed = observation(&body, &budget)?;
    for (node, source) in [
        (&body["messages"][0]["content"], SourceClass::CurrentInput),
        (&body["messages"][2]["content"], SourceClass::ToolResult),
        (
            &body["messages"][3]["content"][0]["text"],
            SourceClass::CurrentInput,
        ),
    ] {
        observed.observe_text(node, source, EditConstraint::Editable)?;
    }
    observed.claim_history(HistoryClaim::CallResultPair {
        call: &body["messages"][1]["tool_calls"][0],
        result: &body["messages"][2],
    })?;
    let baseline = budget.used();
    let validated = Cell::new(false);
    let prepared = prepare_observed_request(
        &mut lease,
        &observed,
        &SecurityRuntimeConfig::default(),
        RESTORE_RULES,
        &[],
        |candidate, encoded, proof| {
            validated.set(true);
            assert!(
                store.stats()?.entries == before.entries,
                "mapping_committed_before_validation"
            );
            assert!(proof.len() == 1, "provisional_proof_not_exact");
            assert!(
                candidate.get("input").is_none() && candidate.get("messages").is_some(),
                "native_request_replaced_with_responses_envelope"
            );
            let decoded: Value =
                serde_json::from_slice(encoded).map_err(|_| TokenError::RedactionFailed)?;
            assert!(&decoded == candidate, "native_encoding_mismatch");
            for node in [
                &candidate["messages"][0]["content"],
                &candidate["messages"][2]["content"],
                &candidate["messages"][3]["content"][0]["text"],
            ] {
                assert!(node.as_str() == Some(proof[0]), "actual_site_not_replaced");
            }
            assert!(
                candidate["unknown"] == original["unknown"],
                "equal_value_sibling_was_patched"
            );
            assert!(
                candidate["messages"][1] == original["messages"][1],
                "tool_envelope_changed"
            );
            assert!(
                candidate["messages"][3]["content"][1] == original["messages"][3]["content"][1],
                "opaque_media_changed"
            );
            assert!(
                candidate["model"] == original["model"]
                    && candidate["temperature"] == original["temperature"]
                    && candidate["stream"] == original["stream"],
                "protocol_parameters_changed"
            );
            Ok(proof.len())
        },
    )
    .map_err(|_| "native_chat_request_transaction_missing")?;
    assert!(
        validated.get() && prepared.validated == 1,
        "native_validation_not_executed"
    );
    assert!(
        prepared.reversible_occurrences == 3 && prepared.irreversible_occurrences == 0,
        "native_occurrence_count_changed"
    );
    let reference = prepared.body.as_value()["messages"][0]["content"]
        .as_str()
        .ok_or("synthetic_reference_missing")?;
    assert!(
        lease.resolve(reference)? == CONTACT,
        "committed_mapping_missing"
    );
    assert!(
        store.stats()?.entries == before.entries + 1,
        "mapping_not_deduplicated"
    );
    assert!(body == original, "native_source_mutated");
    assert!(budget.used() > baseline, "prepared_owners_not_charged");
    drop(prepared);
    assert!(budget.used() == baseline, "prepared_owner_charge_retained");
    drop(observed);
    assert!(budget.used() == 0, "observation_charge_retained");
    Ok(())
}

#[test]
fn c5_request_immutable_required_patch_rejects_entire_transaction() -> TestResult {
    let body = json!({"messages":[{"role":"user", "content":CONTACT},
        {"role":"assistant", "content":CONTACT, "synthetic_signature":"opaque"}]});
    let original = body.clone();
    let store = DlpTokenStore::new();
    let mut lease = native_lease(&store)?;
    let before = store.stats()?;
    let budget = PayloadBudget::with_limit(BUDGET_BYTES);
    let mut observed = observation(&body, &budget)?;
    observed.observe_text(
        &body["messages"][0]["content"],
        SourceClass::CurrentInput,
        EditConstraint::Editable,
    )?;
    observed.observe_text(
        &body["messages"][1]["content"],
        SourceClass::HistoricalText,
        EditConstraint::Immutable,
    )?;
    let baseline = budget.used();
    let called = Cell::new(false);
    let result = prepare_observed_request(
        &mut lease,
        &observed,
        &SecurityRuntimeConfig::default(),
        RESTORE_RULES,
        &[],
        |_, _, _| {
            called.set(true);
            Ok(())
        },
    );
    assert!(
        matches!(result, Err(PlanError::Unsupported)),
        "immutable_mandatory_patch_not_rejected"
    );
    assert!(!called.get(), "immutable_failure_reached_validator");
    assert!(
        lease.is_empty() && store.stats()? == before,
        "immutable_failure_committed_mapping"
    );
    assert!(
        body == original && budget.used() == baseline,
        "immutable_failure_not_atomic"
    );
    Ok(())
}

#[test]
fn c5_request_immutable_unchanged_readable_site_passes_without_authority() -> TestResult {
    let body = json!({"messages":[{"role":"assistant", "content":"Synthetic readable history",
        "synthetic_signature":"opaque"}]});
    let store = DlpTokenStore::new();
    let mut lease = native_lease(&store)?;
    let before = store.stats()?;
    let budget = PayloadBudget::with_limit(BUDGET_BYTES);
    let mut observed = observation(&body, &budget)?;
    observed.observe_text(
        &body["messages"][0]["content"],
        SourceClass::HistoricalText,
        EditConstraint::Immutable,
    )?;
    let baseline = budget.used();
    let prepared = prepare_observed_request(
        &mut lease,
        &observed,
        &SecurityRuntimeConfig::default(),
        RESTORE_RULES,
        &[],
        |candidate, _, proof| {
            assert!(
                candidate == &body && proof.is_empty(),
                "unchanged_immutable_gained_patch_or_proof"
            );
            Ok(())
        },
    )
    .map_err(|_| "unchanged_immutable_native_request_rejected")?;
    assert!(prepared.body.as_value() == &body, "immutable_unit_changed");
    assert!(
        lease.is_empty() && store.stats()? == before,
        "unchanged_immutable_granted_authority"
    );
    drop(prepared);
    assert!(
        budget.used() == baseline,
        "unchanged_immutable_charge_retained"
    );
    Ok(())
}

#[test]
fn c5_request_block_precedes_immutable_patch_rejection() -> TestResult {
    for pattern in ["synthetic-stop", "^"] {
        let body = json!({"messages":[{"role":"assistant", "content":CONTACT},
            {"role":"user", "content":"synthetic-stop"}]});
        let store = DlpTokenStore::new();
        let mut lease = native_lease(&store)?;
        let before = store.stats()?;
        let budget = PayloadBudget::with_limit(BUDGET_BYTES);
        let mut observed = observation(&body, &budget)?;
        observed.observe_text(
            &body["messages"][0]["content"],
            SourceClass::HistoricalText,
            EditConstraint::Immutable,
        )?;
        observed.observe_text(
            &body["messages"][1]["content"],
            SourceClass::CurrentInput,
            EditConstraint::Editable,
        )?;
        let baseline = budget.used();
        let called = Cell::new(false);
        let result = prepare_observed_request(
            &mut lease,
            &observed,
            &custom_runtime(pattern, AuditAction::Block)?,
            RESTORE_RULES,
            &[],
            |_, _, _| {
                called.set(true);
                Ok(())
            },
        );
        assert!(
            matches!(result, Err(PlanError::Blocked)),
            "explicit_block_lost_precedence"
        );
        assert!(
            !called.get() && lease.is_empty(),
            "blocked_request_advanced"
        );
        assert!(
            store.stats()? == before && budget.used() == baseline,
            "blocked_request_not_atomic"
        );
    }
    Ok(())
}

#[test]
fn c5_request_final_validation_failure_rolls_back_provisional_mappings() -> TestResult {
    let body = json!({"messages":[{"role":"user", "content":CONTACT}]});
    let original = body.clone();
    let store = DlpTokenStore::new();
    let mut lease = native_lease(&store)?;
    let before = store.stats()?;
    let budget = PayloadBudget::with_limit(BUDGET_BYTES);
    let mut observed = observation(&body, &budget)?;
    observed.observe_text(
        &body["messages"][0]["content"],
        SourceClass::CurrentInput,
        EditConstraint::Editable,
    )?;
    let baseline = budget.used();
    let called = Cell::new(false);
    let result = prepare_observed_request::<()>(
        &mut lease,
        &observed,
        &SecurityRuntimeConfig::default(),
        RESTORE_RULES,
        &[],
        |candidate, encoded, proof| {
            called.set(true);
            assert!(
                proof.len() == 1 && candidate["messages"][0]["content"].as_str() == Some(proof[0]),
                "validation_did_not_receive_native_candidate"
            );
            assert!(
                !encoded.is_empty() && store.stats()?.entries == before.entries,
                "validation_not_precommit"
            );
            Err(TokenError::RedactionFailed)
        },
    );
    assert!(called.get(), "native_final_validator_not_reached");
    assert!(
        matches!(result, Err(PlanError::Token(TokenError::RedactionFailed))),
        "validation_failure_reclassified"
    );
    assert!(
        lease.is_empty() && store.stats()? == before,
        "failed_validation_committed_mapping"
    );
    assert!(
        body == original && budget.used() == baseline,
        "failed_validation_not_atomic"
    );
    Ok(())
}

#[test]
fn c5_request_candidate_rebuild_failure_is_atomic() -> TestResult {
    let body = json!({"messages":[{"role":"user", "content":CONTACT}]});
    let store = DlpTokenStore::new();
    let mut lease = native_lease(&store)?;
    let before = store.stats()?;
    let budget = PayloadBudget::with_limit(BUDGET_BYTES);
    let mut observed = observation(&body, &budget)?;
    observed.observe_text(
        &body["messages"][0]["content"],
        SourceClass::CurrentInput,
        EditConstraint::Editable,
    )?;
    let baseline = budget.used();
    let rebuild = Rc::new(Cell::new(false));
    let seen = Rc::clone(&rebuild);
    let hook = json_events::install(move |event| {
        if event.phase == json_events::Phase::BeforeBuild {
            seen.set(true);
            json_events::Action::Fail
        } else {
            json_events::Action::Continue
        }
    });
    let called = Cell::new(false);
    let result = prepare_observed_request(
        &mut lease,
        &observed,
        &SecurityRuntimeConfig::default(),
        RESTORE_RULES,
        &[],
        |_, _, _| {
            called.set(true);
            Ok(())
        },
    );
    drop(hook);
    assert!(
        rebuild.get(),
        "budgeted_native_candidate_rebuild_not_reached"
    );
    assert!(
        matches!(result, Err(PlanError::Token(TokenError::Unavailable))),
        "candidate_failure_reclassified"
    );
    assert!(
        !called.get() && lease.is_empty(),
        "candidate_failure_advanced"
    );
    assert!(
        store.stats()? == before && budget.used() == baseline,
        "candidate_failure_not_atomic"
    );
    Ok(())
}

#[test]
fn c5_request_encoding_allocation_failure_is_precommit_and_atomic() -> TestResult {
    let body = json!({"messages":[{"role":"user", "content":CONTACT}]});
    let original = body.clone();
    let store = DlpTokenStore::new();
    let mut lease = native_lease(&store)?;
    let before = store.stats()?;
    let budget = PayloadBudget::with_limit(BUDGET_BYTES);
    let mut observed = observation(&body, &budget)?;
    observed.observe_text(
        &body["messages"][0]["content"],
        SourceClass::CurrentInput,
        EditConstraint::Editable,
    )?;
    let baseline = budget.used();
    let rebuilt = Rc::new(Cell::new(false));
    let seen = Rc::clone(&rebuilt);
    let json_hook = json_events::install(move |event| {
        if event.phase == json_events::Phase::AfterBuild {
            seen.set(true);
        }
        json_events::Action::Continue
    });
    let denied = Rc::new(Cell::new(false));
    let seen = Rc::clone(&denied);
    let candidate_ready = Rc::clone(&rebuilt);
    let store_at_encoding = store.clone();
    let buffer_hook = buffer_events::install(move |event| {
        if candidate_ready.get() && event.phase == buffer_events::Phase::BeforeAllocation {
            seen.set(true);
            assert!(
                event.used > baseline && event.requested > 0,
                "encoding_not_precharged"
            );
            assert!(
                store_at_encoding
                    .stats()
                    .is_ok_and(|stats| stats.entries == before.entries),
                "mapping_committed_before_encoding"
            );
            buffer_events::Action::FailAllocation
        } else {
            buffer_events::Action::Continue
        }
    });
    let called = Cell::new(false);
    let result = prepare_observed_request(
        &mut lease,
        &observed,
        &SecurityRuntimeConfig::default(),
        RESTORE_RULES,
        &[],
        |_, _, _| {
            called.set(true);
            Ok(())
        },
    );
    drop(buffer_hook);
    drop(json_hook);
    assert!(
        rebuilt.get() && denied.get(),
        "native_encoding_allocation_not_reached"
    );
    assert!(
        matches!(result, Err(PlanError::Token(TokenError::Unavailable))),
        "encoding_failure_reclassified"
    );
    assert!(
        !called.get() && lease.is_empty(),
        "encoding_failure_advanced"
    );
    assert!(
        store.stats()? == before && budget.used() == baseline,
        "encoding_failure_not_atomic"
    );
    assert!(body == original, "encoding_failure_mutated_source");
    Ok(())
}

#[test]
fn c5_request_residual_crossing_proof_rejects_before_commit() -> TestResult {
    let body = json!({"messages":[{"role":"user", "content":format!("prefix {CONTACT}")}]});
    let store = DlpTokenStore::new();
    let mut lease = native_lease(&store)?;
    let before = store.stats()?;
    let budget = PayloadBudget::with_limit(BUDGET_BYTES);
    let mut observed = observation(&body, &budget)?;
    observed.observe_text(
        &body["messages"][0]["content"],
        SourceClass::CurrentInput,
        EditConstraint::Editable,
    )?;
    let baseline = budget.used();
    let rebuilt = Rc::new(Cell::new(false));
    let seen = Rc::clone(&rebuilt);
    let hook = json_events::install(move |event| {
        if event.phase == json_events::Phase::AfterBuild {
            seen.set(true);
        }
        json_events::Action::Continue
    });
    let called = Cell::new(false);
    let result = prepare_observed_request(
        &mut lease,
        &observed,
        &custom_runtime("prefix rredact-", AuditAction::Redact)?,
        RESTORE_RULES,
        &[],
        |_, _, _| {
            called.set(true);
            Ok(())
        },
    );
    drop(hook);
    assert!(rebuilt.get(), "native_residual_candidate_not_rebuilt");
    assert!(
        matches!(result, Err(PlanError::Token(TokenError::RedactionFailed))),
        "crossing_residual_not_rejected"
    );
    assert!(
        !called.get() && lease.is_empty(),
        "residual_failure_advanced"
    );
    assert!(
        store.stats()? == before && budget.used() == baseline,
        "residual_failure_not_atomic"
    );
    Ok(())
}

#[test]
fn c5_request_history_pairing_and_literals_never_grant_mapping_authority() -> TestResult {
    let foreign_store = DlpTokenStore::new();
    let mut foreign_lease = native_lease(&foreign_store)?;
    let reference = foreign_lease.prepare(
        &[TokenFragment {
            raw: CONTACT,
            whole_span_categories: &[TokenCategory::Phone],
        }],
        &[],
        |tokens| Ok(tokens[0].clone()),
    )?;
    let body = json!({"messages":[
        {"role":"assistant", "tool_calls":[{"id":"synthetic-call", "type":"function",
            "function":{"name":"synthetic-tool", "arguments":reference}}]},
        {"role":"tool", "tool_call_id":"synthetic-call", "content":reference}
    ]});
    let store = DlpTokenStore::new();
    let mut lease = native_lease(&store)?;
    let before = store.stats()?;
    let budget = PayloadBudget::with_limit(BUDGET_BYTES);
    let mut observed = observation(&body, &budget)?;
    observed.observe_text(
        &body["messages"][0]["tool_calls"][0]["function"]["arguments"],
        SourceClass::ToolArguments,
        EditConstraint::Editable,
    )?;
    observed.observe_text(
        &body["messages"][1]["content"],
        SourceClass::ToolResult,
        EditConstraint::Editable,
    )?;
    observed.claim_history(HistoryClaim::AssistantItem {
        native: &body["messages"][0],
    })?;
    observed.claim_history(HistoryClaim::CallResultPair {
        call: &body["messages"][0]["tool_calls"][0],
        result: &body["messages"][1],
    })?;
    let baseline = budget.used();
    let prepared = prepare_observed_request(
        &mut lease,
        &observed,
        &SecurityRuntimeConfig::default(),
        RESTORE_RULES,
        &[],
        |candidate, _, proof| {
            assert!(
                candidate == &body && proof.is_empty(),
                "unproven_history_gained_authority"
            );
            Ok(())
        },
    )
    .map_err(|_| "unproven_native_history_literal_rejected")?;
    assert!(
        lease.resolve(&reference).is_err() && lease.is_empty(),
        "claim_resolved_foreign_mapping"
    );
    assert!(store.stats()? == before, "history_claim_created_mapping");
    drop(prepared);
    assert!(budget.used() == baseline, "history_literal_charge_retained");
    let called = Cell::new(false);
    let result = prepare_observed_request(
        &mut lease,
        &observed,
        &SecurityRuntimeConfig::default(),
        RESTORE_RULES,
        &[reference.as_str()],
        |_, _, _| {
            called.set(true);
            Ok(())
        },
    );
    assert!(
        matches!(
            result,
            Err(PlanError::Token(TokenError::ContextUnavailable))
        ),
        "caller_token_granted_authority"
    );
    assert!(
        !called.get() && store.stats()? == before && budget.used() == baseline,
        "invalid_history_not_atomic"
    );
    Ok(())
}

#[test]
fn c5_request_foreign_equal_node_is_not_a_native_site_or_history_item() -> TestResult {
    let body = json!({"messages":[{"role":"user", "content":CONTACT}]});
    let foreign = body.clone();
    for foreign_history in [false, true] {
        let store = DlpTokenStore::new();
        let mut lease = native_lease(&store)?;
        let before = store.stats()?;
        let budget = PayloadBudget::with_limit(BUDGET_BYTES);
        let mut observed = observation(&body, &budget)?;
        observed.observe_text(
            if foreign_history {
                &body["messages"][0]["content"]
            } else {
                &foreign["messages"][0]["content"]
            },
            SourceClass::CurrentInput,
            EditConstraint::Editable,
        )?;
        if foreign_history {
            observed.claim_history(HistoryClaim::AssistantItem {
                native: &foreign["messages"][0],
            })?;
        }
        let baseline = budget.used();
        let called = Cell::new(false);
        let result = prepare_observed_request(
            &mut lease,
            &observed,
            &SecurityRuntimeConfig::default(),
            RESTORE_RULES,
            &[],
            |_, _, _| {
                called.set(true);
                Ok(())
            },
        );
        assert!(
            matches!(result, Err(PlanError::Token(TokenError::RedactionFailed))),
            "foreign_native_node_accepted"
        );
        assert!(!called.get() && lease.is_empty(), "foreign_node_advanced");
        assert!(
            store.stats()? == before && budget.used() == baseline,
            "foreign_node_failure_not_atomic"
        );
    }
    Ok(())
}

#[test]
fn c5_request_existing_responses_entry_rejects_native_lease_unchanged() -> TestResult {
    let body = json!({"messages":[{"role":"user", "content":CONTACT}], "synthetic_extension":true});
    let original = body.clone();
    let store = DlpTokenStore::new();
    let mut lease = native_lease(&store)?;
    let before = store.stats()?;
    let budget = PayloadBudget::with_limit(BUDGET_BYTES);
    let called = Cell::new(false);
    let result = prepare_codex_request_with_budget(
        &mut lease,
        &body,
        &SecurityRuntimeConfig::default(),
        RESTORE_RULES,
        &[],
        budget.clone(),
        |_, _, _| {
            called.set(true);
            Ok(())
        },
    );
    assert!(
        matches!(
            result,
            Err(PlanError::Token(TokenError::ContextUnavailable))
        ),
        "responses_scope_guard_weakened"
    );
    assert!(
        !called.get() && lease.is_empty() && store.stats()? == before,
        "responses_entry_granted_native_authority"
    );
    assert!(
        body == original && budget.used() == 0,
        "responses_guard_changed_request_or_budget"
    );
    Ok(())
}

#[test]
fn c5_request_native_entry_rejects_legacy_and_cross_client_leases() -> TestResult {
    let body = json!({"messages":[{"role":"user", "content":CONTACT}]});
    for scope in [
        ContinuityScope::LegacyResponses,
        ContinuityScope::Native {
            client: ClientProtocol::new(Protocol::Anthropic),
            ingress: IngressNamespace::Direct,
        },
        ContinuityScope::Native {
            client: ClientProtocol::new(Protocol::Gemini),
            ingress: IngressNamespace::Direct,
        },
    ] {
        let store = DlpTokenStore::new();
        let mut lease = store.admit_scoped_published_policy(
            scope,
            "synthetic-c5-app",
            Some("synthetic-c5-session"),
            1,
        )?;
        let before = store.stats()?;
        let budget = PayloadBudget::with_limit(BUDGET_BYTES);
        let mut observed = observation(&body, &budget)?;
        observed.observe_text(
            &body["messages"][0]["content"],
            SourceClass::CurrentInput,
            EditConstraint::Editable,
        )?;
        let baseline = budget.used();
        let called = Cell::new(false);
        let result = prepare_observed_request(
            &mut lease,
            &observed,
            &SecurityRuntimeConfig::default(),
            RESTORE_RULES,
            &[],
            |_, _, _| {
                called.set(true);
                Ok(())
            },
        );
        assert!(
            matches!(
                result,
                Err(PlanError::Token(TokenError::ContextUnavailable))
            ),
            "native_entry_accepted_wrong_lease_scope"
        );
        assert!(
            !called.get() && lease.is_empty() && store.stats()? == before,
            "wrong_native_scope_granted_authority"
        );
        assert!(
            budget.used() == baseline,
            "wrong_native_scope_retained_charge"
        );
    }
    let store = DlpTokenStore::new();
    let mut lease = native_lease(&store)?;
    let budget = PayloadBudget::with_limit(BUDGET_BYTES);
    let mut observed = observation(&body, &budget)?;
    observed.observe_text(
        &body["messages"][0]["content"],
        SourceClass::CurrentInput,
        EditConstraint::Editable,
    )?;
    let prepared = prepare_observed_request(
        &mut lease,
        &observed,
        &SecurityRuntimeConfig::default(),
        RESTORE_RULES,
        &[],
        |candidate, _, proof| {
            assert!(
                proof.len() == 1 && candidate["messages"][0]["content"].as_str() == Some(proof[0]),
                "native_scope_positive_missing_patch"
            );
            Ok(())
        },
    )
    .map_err(|_| "matching_native_scope_transaction_rejected")?;
    assert!(
        prepared.reversible_occurrences == 1,
        "native_scope_positive_missing_transaction"
    );
    Ok(())
}

#[test]
fn c5_request_immutable_authorized_reference_requires_rejection_after_block() -> TestResult {
    for blocked in [false, true] {
        let store = DlpTokenStore::new();
        let mut lease = native_lease(&store)?;
        let reference = lease.prepare(
            &[TokenFragment {
                raw: CONTACT,
                whole_span_categories: &[TokenCategory::Phone],
            }],
            &[],
            |tokens| Ok(tokens[0].clone()),
        )?;
        let body = json!({"messages":[{"role":"assistant", "content":reference},
            {"role":"user", "content":"synthetic-stop"}]});
        let before = store.stats()?;
        let budget = PayloadBudget::with_limit(BUDGET_BYTES);
        let mut observed = observation(&body, &budget)?;
        observed.observe_text(
            &body["messages"][0]["content"],
            SourceClass::HistoricalText,
            EditConstraint::Immutable,
        )?;
        observed.observe_text(
            &body["messages"][1]["content"],
            SourceClass::CurrentInput,
            EditConstraint::Editable,
        )?;
        let baseline = budget.used();
        let runtime = if blocked {
            custom_runtime("synthetic-stop", AuditAction::Block)?
        } else {
            SecurityRuntimeConfig::default()
        };
        let called = Cell::new(false);
        let result = prepare_observed_request(
            &mut lease,
            &observed,
            &runtime,
            RESTORE_RULES,
            &[],
            |_, _, _| {
                called.set(true);
                Ok(())
            },
        );
        assert!(
            matches!(
                (&result, blocked),
                (Err(PlanError::Blocked), true) | (Err(PlanError::Unsupported), false)
            ),
            "immutable_authorized_reference_not_rejected_after_block"
        );
        assert!(
            !called.get() && store.stats()? == before && budget.used() == baseline,
            "immutable_authorized_reference_changed_transaction_state"
        );
        assert!(
            lease.resolve(&reference).is_ok(),
            "existing_authority_was_lost"
        );
    }
    Ok(())
}
