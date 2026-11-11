//! AN1 request test-first preparation; native registration remains Disabled.
//!
//! Protocol-state table (AN2/AN3 rows specify boundaries, not implemented behavior):
//! | Native carrier / index | Readable state and edit constraint | Opaque / unsupported |
//! | system string or text[].text | CurrentInput, editable | Unknown block kinds/members |
//! | messages[i].content string or text[j].text | User: CurrentInput; assistant: HistoricalText; editable | Media, IDs, names, cache hints, unknown roles/kinds |
//! | tool_use block input, recursive object/array string leaves | ToolArguments, editable; preserve native JSON types | Object keys, tool ID/name, sibling members |
//! | tool_result.content string or text[j].text | ToolResult, editable, untrusted current input on every replay | Media/unknown members; no delivered-result attestation |
//! | thinking[j].thinking, with or without signature | HistoricalText, immutable from outset, readable for DLP | Signature bytes never inspected or granted authority |
//! | redacted_thinking / image / document / unknown block | No readable site | data, source, media, unknown values stay opaque |
//!
//! | Message / block state | Allowed transition / completion requirement | Invalid / incomplete |
//! | NotStarted | message_start -> Started; empty initial content | Duplicate/missing start, wrong event/type |
//! | Started, next block index n | content_block_start(n) -> Open(n, kind) | Reused/skipped/foreign index, simultaneous open blocks |
//! | Open text / thinking / tool_use | Matching text_delta / thinking_delta or signature_delta / input_json_delta | Wrong delta kind/index; partial JSON is not complete arguments |
//! | Open thinking | Immutable at block start, even before late signature_delta | Never emit a patched prefix then attach a signature |
//! | Open(n) | Matching content_block_stop -> Closed(n), advance index | Delta after stop, duplicate stop, truncated tool input |
//! | All blocks closed | message_delta stop_reason: end_turn, stop_sequence, or validated tool_use | max_tokens, model_context_window_exceeded, refusal, pause_turn, unknown/missing reason |
//! | Valid reason | message_stop then clean entity EOF | EOF alone, open block, missing stop, error event, abort/cancel/transport error |
//! | Terminal | No further semantic events; ping is not proof | Duplicate terminal, trailing content, malformed or unfinished frame |
//!
//! Signatures remain on the original thinking block: never move, split, merge,
//! strip, synthesize, decode, or treat them as session/history/mapping proof.
//! Mandatory immutable redaction or authorized restoration fails before commit;
//! unknown unauthorized reference syntax alone does not grant authority.
//! WebSockets, unlisted framing/carriers, signed-payload replay caches, response
//! restoration/verifiers, history authorization, HTTP wiring and enablement are
//! outside AN1. The existing strict Anthropic completion checks remain the guide
//! for later AN2 tests; this table is not a new permissive completion verifier.
use crate::proxy::{protocol_bridge::Protocol, security::restoration_contract::Operation};

const CONTRACT_PROTOCOL: Protocol = Protocol::Anthropic;
const CONTRACT_OPERATION: Operation = Operation::Messages;

#[path = "../contract_probe.rs"]
mod contract_probe;

use std::cell::Cell;

use serde_json::{json, Value};

use super::observe_request;
use crate::proxy::security::{
    payload_budget::PayloadBudget,
    request_plan::PlanError,
    request_preparation::prepare_observed_request,
    restoration_contract::{
        ClientProtocol, ContinuityScope, ContractError, EditConstraint, HistoryClaim,
        IngressNamespace, RequestObservation, SourceClass, TextPatch,
    },
    runtime::{
        admit_security_runtime, get_security_runtime, set_security_runtime_for_test,
        CompiledCustomRule, SecurityRuntimeConfig,
    },
    tokenization::{DlpRequestLease, DlpTokenStore, TokenError},
    types::{AuditAction, RiskLevel},
};

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;
const MARKER: &str = "SYNTHETIC_AN1_MARKER";
const BUDGET_BYTES: usize = 4 * 1024 * 1024;

fn observation<'native>(
    body: &'native Value,
    budget: &PayloadBudget,
) -> TestResult<RequestObservation<'native>> {
    let result = observe_request(body, budget.clone());
    // A fixed-error scaffold must fail here at runtime, not during fixture setup.
    assert!(result.is_ok(), "an1_native_observation_missing");
    Ok(result?)
}

fn assert_site(
    observed: &RequestObservation<'_>,
    node: &Value,
    source: SourceClass,
    constraint: EditConstraint,
) {
    let count = (0..observed.sites().len())
        .filter_map(|index| observed.sites().site(index))
        .filter(|site| std::ptr::eq(site.native(), node))
        .inspect(|site| {
            assert!(site.source() == source, "an1_site_source_changed");
            assert!(
                site.constraint() == constraint,
                "an1_site_constraint_changed"
            );
            assert!(Some(site.text()) == node.as_str(), "an1_site_text_changed");
        })
        .count();
    assert!(count == 1, "an1_exact_native_site_missing_or_duplicated");
}

struct RuntimeGuard(SecurityRuntimeConfig);

impl Drop for RuntimeGuard {
    fn drop(&mut self) {
        set_security_runtime_for_test(self.0.clone());
    }
}

fn native_lease(store: &DlpTokenStore) -> TestResult<(RuntimeGuard, DlpRequestLease)> {
    let guard = RuntimeGuard((*get_security_runtime()).clone());
    set_security_runtime_for_test(SecurityRuntimeConfig::default());
    let admission = admit_security_runtime()?;
    Ok((
        guard,
        store.admit_scoped_published_policy(
            ContinuityScope::Native {
                client: ClientProtocol::new(Protocol::Anthropic),
                ingress: IngressNamespace::Direct,
            },
            "synthetic-an1-app",
            Some("synthetic-an1-session"),
            admission.restoration_policy_revision,
        )?,
    ))
}

fn runtime(action: AuditAction) -> TestResult<SecurityRuntimeConfig> {
    Ok(SecurityRuntimeConfig {
        custom_rules: vec![CompiledCustomRule {
            id: "synthetic-an1-rule".into(),
            name: "Synthetic AN1 rule".into(),
            rule_type: "regex".into(),
            pattern: MARKER.into(),
            regex: Some(regex::Regex::new(MARKER)?),
            risk_level: RiskLevel::High,
            action,
            enabled: true,
        }],
        ..Default::default()
    })
}

#[test]
fn an1_system_string_and_text_blocks_have_current_editable_sites() -> TestResult {
    for system in [
        json!(MARKER),
        json!([
            {"type":"text", "text":MARKER, "cache_control":{"type":"ephemeral"}},
            {"type":"text", "text":"Synthetic second system block"},
            {"type":"synthetic_future", "text":MARKER}
        ]),
    ] {
        let body = json!({"system":system, "messages":[], "unknown":MARKER});
        let original = body.clone();
        let budget = PayloadBudget::with_limit(BUDGET_BYTES);
        let observed = observation(&body, &budget)?;
        assert!(
            std::ptr::eq(observed.native(), &body),
            "an1_native_owner_lost"
        );
        assert!(
            observed.client().protocol() == Protocol::Anthropic,
            "an1_client_changed"
        );
        if body["system"].is_string() {
            assert!(observed.sites().len() == 1, "an1_system_string_site_count");
            assert_site(
                &observed,
                &body["system"],
                SourceClass::CurrentInput,
                EditConstraint::Editable,
            );
        } else {
            assert!(observed.sites().len() == 2, "an1_system_block_site_count");
            for index in 0..2 {
                assert_site(
                    &observed,
                    &body["system"][index]["text"],
                    SourceClass::CurrentInput,
                    EditConstraint::Editable,
                );
            }
        }
        assert!(budget.used() > 0, "an1_metadata_not_charged");
        assert!(
            observed.budget().used() == budget.used(),
            "an1_budget_not_shared"
        );
        assert!(body == original, "an1_observation_mutated_body");
        drop(observed);
        assert!(budget.used() == 0, "an1_metadata_charge_retained");
    }
    Ok(())
}

#[test]
fn an1_message_string_and_text_blocks_preserve_source_classes() -> TestResult {
    let body = json!({"messages":[
        {"role":"user", "content":MARKER},
        {"role":"assistant", "content":"Synthetic assistant history"},
        {"role":"user", "content":[{"type":"text", "text":MARKER}]},
        {"role":"assistant", "content":[{"type":"text", "text":"Synthetic assistant block"}]}
    ]});
    let budget = PayloadBudget::with_limit(BUDGET_BYTES);
    let store = DlpTokenStore::new();
    let (_runtime, lease) = native_lease(&store)?;
    let before = store.stats()?;
    let observed = observation(&body, &budget)?;
    assert!(observed.sites().len() == 4, "an1_message_site_count");
    for (node, source) in [
        (&body["messages"][0]["content"], SourceClass::CurrentInput),
        (&body["messages"][1]["content"], SourceClass::HistoricalText),
        (
            &body["messages"][2]["content"][0]["text"],
            SourceClass::CurrentInput,
        ),
        (
            &body["messages"][3]["content"][0]["text"],
            SourceClass::HistoricalText,
        ),
    ] {
        assert_site(&observed, node, source, EditConstraint::Editable);
    }
    // These are borrowed claims only; no exact-delivery or AN3 replay authority.
    for index in [1, 3] {
        assert!(observed.history().iter().any(|claim| matches!(claim,
            HistoryClaim::AssistantItem { native } if std::ptr::eq(*native, &body["messages"][index])
        )), "an1_assistant_inspection_claim_missing");
    }
    assert!(
        !observed
            .history()
            .iter()
            .any(|claim| matches!(claim, HistoryClaim::ResponsesCursor { .. })),
        "an1_native_cursor_invented"
    );
    assert!(
        lease.is_empty() && store.stats()? == before,
        "an1_observation_granted_authority"
    );
    Ok(())
}

#[test]
fn an1_tool_result_string_and_text_blocks_are_untrusted_input() -> TestResult {
    let body = json!({"messages":[
        {"role":"assistant", "content":[{"type":"tool_use", "id":"synthetic-call", "name":"synthetic-tool", "input":{}}]},
        {"role":"user", "content":[
            {"type":"tool_result", "tool_use_id":"synthetic-call", "content":MARKER},
            {"type":"tool_result", "tool_use_id":"synthetic-unverified-call", "content":[
                {"type":"text", "text":MARKER},
                {"type":"image", "source":{"type":"base64", "data":MARKER}},
                {"type":"synthetic_future", "text":MARKER}
            ]}
        ]}
    ]});
    let budget = PayloadBudget::with_limit(BUDGET_BYTES);
    let store = DlpTokenStore::new();
    let (_runtime, mut lease) = native_lease(&store)?;
    let before = store.stats()?;
    let observed = observation(&body, &budget)?;
    assert!(observed.sites().len() == 2, "an1_tool_result_site_count");
    for node in [
        &body["messages"][1]["content"][0]["content"],
        &body["messages"][1]["content"][1]["content"][0]["text"],
    ] {
        assert_site(
            &observed,
            node,
            SourceClass::ToolResult,
            EditConstraint::Editable,
        );
    }
    assert!(
        !observed.history().iter().any(|claim| matches!(claim,
            HistoryClaim::AssistantItem { native } if std::ptr::eq(*native, &body["messages"][1])
        )),
        "an1_result_attested_as_delivered_history"
    );
    assert!(
        lease.is_empty() && store.stats()? == before,
        "an1_pairing_granted_authority"
    );
    let prepared = prepare_observed_request(
        &mut lease,
        &observed,
        &runtime(AuditAction::RedactRestore)?,
        &[],
        &[],
        |candidate, _, proof| {
            assert!(
                store.stats()? == before,
                "an1_result_committed_before_validation"
            );
            assert!(proof.len() == 1, "an1_result_proof_count");
            assert!(
                candidate["messages"][1]["content"][0]["content"].as_str() == Some(proof[0])
                    && candidate["messages"][1]["content"][1]["content"][0]["text"].as_str()
                        == Some(proof[0]),
                "an1_untrusted_result_not_inspected"
            );
            assert!(
                candidate["messages"][1]["content"][1]["content"][1]
                    == body["messages"][1]["content"][1]["content"][1],
                "an1_result_media_changed"
            );
            Ok(())
        },
    )
    .map_err(|_| "an1_result_preparation_missing")?;
    assert!(
        prepared.reversible_occurrences == 2,
        "an1_result_occurrence_count"
    );
    Ok(())
}

#[test]
fn an1_tool_use_input_recursively_observes_only_native_string_values() -> TestResult {
    let body = json!({"messages":[{"role":"assistant", "content":[{
        "type":"tool_use", "id":MARKER, "name":MARKER,
        "input":{"plain":MARKER, "nested":[{"value":MARKER}, [MARKER, 7, true, null]],
            "SYNTHETIC_AN1_MARKER":false},
        "unknown":{"value":MARKER}
    }]}]});
    let budget = PayloadBudget::with_limit(BUDGET_BYTES);
    let observed = observation(&body, &budget)?;
    let input = &body["messages"][0]["content"][0]["input"];
    assert!(observed.sites().len() == 3, "an1_tool_input_site_count");
    for node in [
        &input["plain"],
        &input["nested"][0]["value"],
        &input["nested"][1][0],
    ] {
        assert_site(
            &observed,
            node,
            SourceClass::ToolArguments,
            EditConstraint::Editable,
        );
    }
    Ok(())
}

#[test]
fn an1_thinking_is_readable_immutable_before_signature_and_keeps_association() -> TestResult {
    let body = json!({"messages":[{"role":"assistant", "content":[
        {"type":"thinking", "thinking":"Synthetic unsigned thinking"},
        {"type":"thinking", "thinking":"Synthetic signed thinking", "signature":"synthetic-signature-one"},
        {"type":"thinking", "thinking":"Synthetic other thinking", "signature":"synthetic-signature-two"},
        {"type":"redacted_thinking", "data":MARKER}
    ]}]});
    let original = body.clone();
    let budget = PayloadBudget::with_limit(BUDGET_BYTES);
    let observed = observation(&body, &budget)?;
    assert!(observed.sites().len() == 3, "an1_thinking_site_count");
    for index in 0..3 {
        let node = &body["messages"][0]["content"][index]["thinking"];
        assert_site(
            &observed,
            node,
            SourceClass::HistoricalText,
            EditConstraint::Immutable,
        );
        let site = (0..observed.sites().len())
            .filter_map(|index| observed.sites().site(index))
            .find(|site| std::ptr::eq(site.native(), node))
            .ok_or("an1_thinking_site_missing")?;
        let plan = observed.sites().plan(&[TextPatch::new(
            site,
            0..site.text().len(),
            "Synthetic replacement",
        )]);
        assert!(
            matches!(
                plan,
                Err(crate::proxy::security::restoration_contract::ContractError::ImmutableSite)
            ),
            "an1_thinking_patch_escaped"
        );
    }
    assert!(body == original, "an1_signature_association_changed");
    Ok(())
}

#[test]
fn an1_opaque_signature_redacted_media_and_unknown_fields_have_no_sites() -> TestResult {
    let body = json!({"model":MARKER, "metadata":{"user_id":MARKER}, "unknown":{"text":MARKER},
    "system":[{"type":"synthetic_future", "text":MARKER}], "messages":[
        {"role":"assistant", "content":[
            {"type":"thinking", "thinking":"Synthetic readable thinking", "signature":MARKER, "unknown":MARKER},
            {"type":"redacted_thinking", "data":MARKER},
            {"type":"synthetic_future", "text":MARKER}
        ]},
        {"role":"user", "content":[
            {"type":"image", "source":{"type":"base64", "data":MARKER}},
            {"type":"document", "source":{"type":"text", "data":MARKER}},
            {"type":"text", "text":"Synthetic public text", "unknown":MARKER}
        ]}
    ]});
    let original = body.clone();
    let budget = PayloadBudget::with_limit(BUDGET_BYTES);
    let observed = observation(&body, &budget)?;
    assert!(
        observed.sites().len() == 2,
        "an1_opaque_fields_became_sites"
    );
    assert_site(
        &observed,
        &body["messages"][0]["content"][0]["thinking"],
        SourceClass::HistoricalText,
        EditConstraint::Immutable,
    );
    assert_site(
        &observed,
        &body["messages"][1]["content"][2]["text"],
        SourceClass::CurrentInput,
        EditConstraint::Editable,
    );
    let store = DlpTokenStore::new();
    let (_runtime, mut lease) = native_lease(&store)?;
    let before = store.stats()?;
    let baseline = budget.used();
    let prepared = prepare_observed_request(
        &mut lease,
        &observed,
        &runtime(AuditAction::Redact)?,
        &[],
        &[],
        |candidate, encoded, proof| {
            let decoded: Value =
                serde_json::from_slice(encoded).map_err(|_| TokenError::RedactionFailed)?;
            assert!(
                &decoded == candidate && candidate == &original,
                "an1_opaque_or_signature_changed"
            );
            assert!(proof.is_empty(), "an1_opaque_field_granted_proof");
            Ok(())
        },
    )
    .map_err(|_| "an1_unchanged_immutable_preparation_missing")?;
    assert!(
        prepared.body.as_value() == &original && body == original,
        "an1_signed_unit_changed"
    );
    assert!(
        lease.is_empty() && store.stats()? == before,
        "an1_signature_granted_authority"
    );
    drop(prepared);
    assert!(
        budget.used() == baseline,
        "an1_unchanged_prepared_charge_retained"
    );
    drop(observed);
    assert!(budget.used() == 0, "an1_unchanged_metadata_charge_retained");
    Ok(())
}

#[test]
fn an1_prepare_redacts_native_sites_atomically_and_preserves_types() -> TestResult {
    for action in [AuditAction::Redact, AuditAction::RedactRestore] {
        let body = json!({"model":"synthetic-model", "temperature":0.25, "max_tokens":256, "stream":false,
            "system":[{"type":"text", "text":MARKER, "cache_control":{"type":"ephemeral"}}],
            "messages":[
                {"role":"user", "content":MARKER},
                {"role":"assistant", "content":[{"type":"tool_use", "id":"synthetic-call", "name":"synthetic-tool",
                    "input":{"plain":MARKER, "nested":[{"value":MARKER}, [MARKER, 7, true, null]], "unknown_native":false},
                    "unknown":MARKER}]},
                {"role":"user", "content":[{"type":"tool_result", "tool_use_id":"synthetic-call", "is_error":false,
                    "content":[{"type":"text", "text":MARKER}, {"type":"image", "source":{"data":MARKER}}]}]}
            ], "unknown":{"same_value":MARKER, "types":[true, null, 7]}});
        let original = body.clone();
        let budget = PayloadBudget::with_limit(BUDGET_BYTES);
        let store = DlpTokenStore::new();
        let (_runtime, mut lease) = native_lease(&store)?;
        let before = store.stats()?;
        let observed = observation(&body, &budget)?;
        assert!(observed.sites().len() == 6, "an1_transaction_site_count");
        let baseline = budget.used();
        let called = Cell::new(false);
        let mapped = action == AuditAction::RedactRestore;
        let prepared = prepare_observed_request(
            &mut lease,
            &observed,
            &runtime(action)?,
            &[],
            &[],
            |candidate, encoded, proof| {
                called.set(true);
                assert!(
                    store.stats()? == before,
                    "an1_mapping_committed_before_validation"
                );
                assert!(
                    proof.len() == usize::from(mapped),
                    "an1_provisional_proof_count"
                );
                let decoded: Value =
                    serde_json::from_slice(encoded).map_err(|_| TokenError::RedactionFailed)?;
                assert!(&decoded == candidate, "an1_encoding_mismatch");
                let mut expected = original.clone();
                for path in [
                    "/system/0/text",
                    "/messages/0/content",
                    "/messages/1/content/0/input/plain",
                    "/messages/1/content/0/input/nested/0/value",
                    "/messages/1/content/0/input/nested/1/0",
                    "/messages/2/content/0/content/0/text",
                ] {
                    let replacement = candidate
                        .pointer(path)
                        .and_then(Value::as_str)
                        .ok_or(TokenError::RedactionFailed)?;
                    assert!(
                        replacement != MARKER && !replacement.contains(MARKER),
                        "an1_mandatory_site_not_redacted"
                    );
                    if mapped {
                        assert!(
                            Some(&replacement) == proof.first(),
                            "an1_native_mapping_not_shared"
                        );
                    }
                    *expected
                        .pointer_mut(path)
                        .ok_or(TokenError::RedactionFailed)? = Value::String(replacement.into());
                }
                assert!(
                    candidate == &expected,
                    "an1_native_types_or_untouched_values_changed"
                );
                Ok(())
            },
        )
        .map_err(|_| "an1_native_transaction_missing")?;
        assert!(called.get(), "an1_validator_not_called");
        assert!(
            prepared.reversible_occurrences == if mapped { 6 } else { 0 }
                && prepared.irreversible_occurrences == if mapped { 0 } else { 6 },
            "an1_occurrence_counts_changed"
        );
        let replacement = prepared.body.as_value()["system"][0]["text"]
            .as_str()
            .ok_or("an1_replacement_missing")?;
        if mapped {
            assert!(
                lease.resolve(replacement)? == MARKER,
                "an1_mapping_not_committed"
            );
            assert!(
                store.stats()?.entries == before.entries + 1,
                "an1_mapping_not_deduplicated"
            );
        } else {
            assert!(
                lease.is_empty() && store.stats()? == before,
                "an1_irreversible_redaction_granted_authority"
            );
        }
        assert!(
            body == original && budget.used() > baseline,
            "an1_native_owner_or_budget_lost"
        );
        drop(prepared);
        assert!(budget.used() == baseline, "an1_prepared_charge_retained");
        drop(observed);
        assert!(budget.used() == 0, "an1_observation_charge_retained");
    }
    Ok(())
}

#[test]
fn an1_prepare_immutable_mandatory_patch_rejects_before_commit() -> TestResult {
    for signed in [false, true] {
        for action in [AuditAction::Redact, AuditAction::RedactRestore] {
            let mut thinking = json!({"type":"thinking", "thinking":MARKER});
            if signed {
                thinking["signature"] = json!("synthetic-immutable-signature");
            }
            let body =
                json!({"system":MARKER, "messages":[{"role":"assistant", "content":[thinking]}]});
            let original = body.clone();
            let budget = PayloadBudget::with_limit(BUDGET_BYTES);
            let store = DlpTokenStore::new();
            let (_runtime, mut lease) = native_lease(&store)?;
            let before = store.stats()?;
            let observed = observation(&body, &budget)?;
            assert_site(
                &observed,
                &body["messages"][0]["content"][0]["thinking"],
                SourceClass::HistoricalText,
                EditConstraint::Immutable,
            );
            let baseline = budget.used();
            let called = Cell::new(false);
            let result = prepare_observed_request(
                &mut lease,
                &observed,
                &runtime(action)?,
                &[],
                &[],
                |_, _, _| {
                    called.set(true);
                    Ok(())
                },
            );
            assert!(
                matches!(result, Err(PlanError::ImmutableContent)),
                "an1_immutable_required_patch_not_rejected"
            );
            assert!(!called.get(), "an1_immutable_failure_reached_validator");
            assert!(
                lease.is_empty() && store.stats()? == before,
                "an1_immutable_failure_committed_mapping"
            );
            assert!(
                body == original && budget.used() == baseline,
                "an1_immutable_failure_not_atomic"
            );
            drop(observed);
            assert!(budget.used() == 0, "an1_failed_observation_charge_retained");
        }
    }
    Ok(())
}

#[test]
fn an1_prepare_explicit_block_in_thinking_remains_policy_block() -> TestResult {
    let body = json!({"messages":[{"role":"assistant", "content":[
        {"type":"thinking", "thinking":MARKER, "signature":"synthetic-block-signature"}
    ]}]});
    let budget = PayloadBudget::with_limit(BUDGET_BYTES);
    let store = DlpTokenStore::new();
    let (_runtime, mut lease) = native_lease(&store)?;
    let before = store.stats()?;
    let observed = observation(&body, &budget)?;
    let baseline = budget.used();
    let called = Cell::new(false);
    let result = prepare_observed_request(
        &mut lease,
        &observed,
        &runtime(AuditAction::Block)?,
        &[],
        &[],
        |_, _, _| {
            called.set(true);
            Ok(())
        },
    );
    assert!(
        matches!(result, Err(PlanError::Blocked)),
        "an1_explicit_block_projection_lost"
    );
    assert!(
        !called.get() && lease.is_empty() && store.stats()? == before,
        "an1_block_committed_or_validated"
    );
    assert!(budget.used() == baseline, "an1_block_charge_retained");
    Ok(())
}

#[test]
fn an1_prepare_validation_failure_has_no_commit_or_mutation() -> TestResult {
    let body = json!({"system":MARKER, "messages":[{"role":"user", "content":MARKER}],
        "unknown":{"same_value":MARKER, "types":[true, null, 7]}});
    let original = body.clone();
    let budget = PayloadBudget::with_limit(BUDGET_BYTES);
    let store = DlpTokenStore::new();
    let (_runtime, mut lease) = native_lease(&store)?;
    let before = store.stats()?;
    let observed = observation(&body, &budget)?;
    let baseline = budget.used();
    let called = Cell::new(false);
    let result = prepare_observed_request::<()>(
        &mut lease,
        &observed,
        &runtime(AuditAction::RedactRestore)?,
        &[],
        &[],
        |candidate, _, proof| {
            called.set(true);
            assert!(
                proof.len() == 1 && store.stats()? == before,
                "an1_failed_validation_precommit_lost"
            );
            assert!(
                candidate["system"].as_str() != Some(MARKER)
                    && candidate["unknown"] == original["unknown"],
                "an1_failed_validation_candidate_unsafe"
            );
            Err(TokenError::RedactionFailed)
        },
    );
    assert!(
        matches!(result, Err(PlanError::Token(TokenError::RedactionFailed))),
        "an1_validator_failure_not_propagated"
    );
    assert!(called.get(), "an1_failing_validator_not_called");
    assert!(
        lease.is_empty() && store.stats()? == before,
        "an1_failed_validation_committed"
    );
    assert!(
        body == original && budget.used() == baseline,
        "an1_failed_validation_not_atomic"
    );
    drop(observed);
    assert!(budget.used() == 0, "an1_validation_failure_charge_retained");
    Ok(())
}

// Resource characterizations of the existing observer, not evidence of a new RED.
// The 4 Mi work ceiling is intentionally not exercised with a huge native fixture.
// Immutable thinking/no-mutable-fallback remains covered by the preparation tests above.
#[test]
fn an1_resource_metadata_exact_bytes_accept_one_byte_short_refunds() -> TestResult {
    // One site and one history claim avoid vector-growth overlap in the measured charge.
    let body = json!({"messages":[{"role":"assistant", "content":[
        {"type":"text", "text":MARKER}
    ]}]});
    let original = body.clone();
    let store = DlpTokenStore::new();
    let (_runtime, lease) = native_lease(&store)?;
    let before = store.stats()?;
    let measuring_budget = PayloadBudget::with_limit(BUDGET_BYTES);
    let measured = observation(&body, &measuring_budget)?;
    let metadata_bytes = measuring_budget.used();
    assert!(metadata_bytes > 0, "an1_resource_metadata_not_charged");
    drop(measured);
    assert!(
        measuring_budget.used() == 0,
        "an1_resource_measured_metadata_not_refunded"
    );

    let exact_budget = PayloadBudget::with_limit(metadata_bytes);
    let observed = observation(&body, &exact_budget)?;
    assert!(
        std::ptr::eq(observed.native(), &body),
        "an1_resource_exact_native_owner_lost"
    );
    assert!(observed.sites().len() == 1, "an1_resource_exact_site_count");
    let native_text = &body["messages"][0]["content"][0]["text"];
    assert_site(
        &observed,
        native_text,
        SourceClass::HistoricalText,
        EditConstraint::Editable,
    );
    let site = observed
        .sites()
        .site(0)
        .ok_or("an1_resource_exact_site_missing")?;
    assert!(
        std::ptr::eq(
            site.text(),
            native_text
                .as_str()
                .ok_or("an1_resource_native_text_missing")?
        ),
        "an1_resource_text_not_borrowed"
    );
    assert!(
        matches!(observed.history(), [HistoryClaim::AssistantItem { native }]
            if std::ptr::eq(*native, &body["messages"][0])),
        "an1_resource_exact_history_owner_lost"
    );
    assert!(
        exact_budget.used() == metadata_bytes && observed.budget().used() == exact_budget.limit(),
        "an1_resource_exact_metadata_charge_changed"
    );
    assert!(
        lease.is_empty() && store.stats()? == before && body == original,
        "an1_resource_exact_observation_changed_state"
    );
    drop(observed);
    assert!(
        exact_budget.used() == 0,
        "an1_resource_exact_charge_retained"
    );

    let short_budget = PayloadBudget::with_limit(metadata_bytes - 1);
    let result = observe_request(&body, short_budget.clone());
    assert!(
        matches!(
            result,
            Err(ContractError::Resource(TokenError::CapacityExceeded))
        ),
        "an1_resource_one_byte_short_not_typed_capacity"
    );
    assert!(
        short_budget.used() == 0 && body == original,
        "an1_resource_one_byte_short_not_atomic"
    );
    assert!(
        lease.is_empty() && store.stats()? == before,
        "an1_resource_one_byte_short_changed_authority"
    );
    Ok(())
}

#[test]
fn an1_resource_4096_sites_accept_4097_typed_failure_refunds() -> TestResult {
    let store = DlpTokenStore::new();
    let (_runtime, lease) = native_lease(&store)?;
    let before = store.stats()?;
    for count in [4096, 4097] {
        let blocks: Vec<Value> = (0..count)
            .map(|_| json!({"type":"text", "text":MARKER}))
            .collect();
        let body = json!({"messages":[{"role":"user", "content":blocks}]});
        let original = body.clone();
        let budget = PayloadBudget::with_limit(BUDGET_BYTES);
        let result = observe_request(&body, budget.clone());
        if count == 4096 {
            let observed = result?;
            assert!(
                std::ptr::eq(observed.native(), &body) && observed.sites().len() == count,
                "an1_resource_4096_sites_not_borrowed"
            );
            assert!(
                observed.history().is_empty(),
                "an1_resource_user_history_invented"
            );
            // Check each native identity once; identical text must not deduplicate sites.
            for index in 0..count {
                let site = observed
                    .sites()
                    .site(index)
                    .ok_or("an1_resource_site_missing")?;
                assert!(
                    std::ptr::eq(
                        site.native(),
                        &body["messages"][0]["content"][index]["text"]
                    ) && site.text() == MARKER
                        && site.source() == SourceClass::CurrentInput
                        && site.constraint() == EditConstraint::Editable,
                    "an1_resource_site_identity_or_class_changed"
                );
            }
            assert!(budget.used() > 0, "an1_resource_sites_not_charged");
            drop(observed);
        } else {
            assert!(
                matches!(
                    result,
                    Err(ContractError::Resource(TokenError::CapacityExceeded))
                ),
                "an1_resource_4097_sites_not_typed_capacity"
            );
        }
        assert!(
            budget.used() == 0 && body == original,
            "an1_resource_site_limit_not_atomic_or_refunded"
        );
        assert!(
            lease.is_empty() && store.stats()? == before,
            "an1_resource_site_limit_changed_authority"
        );
    }
    Ok(())
}

#[test]
fn an1_resource_tool_input_inside_depth_limit_borrows_recursive_leaf() -> TestResult {
    // Twelve object/array pairs put the leaf at depth 29, comfortably inside 64.
    let mut input = json!(MARKER);
    for _ in 0..12 {
        input = json!({"synthetic_child":[input]});
    }
    let body = json!({"messages":[{"role":"assistant", "content":[
        {"type":"text", "text":"Synthetic prior resource text"},
        {"type":"tool_use", "id":"synthetic-resource-call", "name":"synthetic-resource-tool", "input":input}
    ]}]});
    let original = body.clone();
    let budget = PayloadBudget::with_limit(BUDGET_BYTES);
    let store = DlpTokenStore::new();
    let (_runtime, lease) = native_lease(&store)?;
    let before = store.stats()?;
    let observed = observation(&body, &budget)?;
    assert!(
        std::ptr::eq(observed.native(), &body) && observed.sites().len() == 2,
        "an1_resource_inside_depth_observation_incomplete"
    );
    assert_site(
        &observed,
        &body["messages"][0]["content"][0]["text"],
        SourceClass::HistoricalText,
        EditConstraint::Editable,
    );
    let mut leaf = &body["messages"][0]["content"][1]["input"];
    for _ in 0..12 {
        leaf = &leaf["synthetic_child"][0];
    }
    assert_site(
        &observed,
        leaf,
        SourceClass::ToolArguments,
        EditConstraint::Editable,
    );
    assert!(
        budget.used() > 0 && lease.is_empty() && store.stats()? == before && body == original,
        "an1_resource_inside_depth_changed_state"
    );
    drop(observed);
    assert!(
        budget.used() == 0,
        "an1_resource_inside_depth_charge_retained"
    );
    Ok(())
}

#[test]
fn an1_resource_excessive_tool_input_depth_after_text_is_atomic_typed_failure() -> TestResult {
    let mut body = json!({"messages":[{"role":"assistant", "content":[
        {"type":"text", "text":MARKER},
        {"type":"tool_use", "id":"synthetic-resource-call", "name":"synthetic-resource-tool", "input":{}}
    ]}]});
    let budget = PayloadBudget::with_limit(BUDGET_BYTES);
    let store = DlpTokenStore::new();
    let (_runtime, lease) = native_lease(&store)?;
    let before = store.stats()?;
    // Establish a real charged prior site and assistant claim before deepening the input.
    let prefix = observation(&body, &budget)?;
    assert!(
        prefix.sites().len() == 1 && prefix.history().len() == 1 && budget.used() > 0,
        "an1_resource_depth_prefix_not_observed"
    );
    assert_site(
        &prefix,
        &body["messages"][0]["content"][0]["text"],
        SourceClass::HistoricalText,
        EditConstraint::Editable,
    );
    drop(prefix);
    assert!(
        budget.used() == 0,
        "an1_resource_depth_prefix_charge_retained"
    );

    // Eighty native container levels clearly exceed 64 without a large work fixture.
    let mut input = json!(MARKER);
    for _ in 0..40 {
        input = json!({"synthetic_child":[input]});
    }
    body["messages"][0]["content"][1]["input"] = input;
    let original = body.clone();
    let result = observe_request(&body, budget.clone());
    assert!(
        matches!(
            result,
            Err(ContractError::Resource(TokenError::CapacityExceeded))
        ),
        "an1_resource_excessive_depth_not_typed_capacity"
    );
    assert!(
        budget.used() == 0 && body == original,
        "an1_resource_depth_failure_retained_partial_state"
    );
    assert!(
        lease.is_empty() && store.stats()? == before,
        "an1_resource_depth_failure_changed_authority"
    );
    Ok(())
}

// AN2/AN3 protocol-state table: one message owns sequential block indices.
// | Indexed state | Supported text carriers | Required terminal / failure boundary |
// | NotStarted -> Started | message_start with an empty native content array | Duplicate/missing start fails |
// | Started -> Open(n) -> Closed(n) | text/text_delta; thinking/thinking_delta (immutable at start); tool_use input leaves/input_json_delta | Only matching start/delta/stop kinds and indices; late signature_delta stays opaque and never makes earlier thinking editable |
// | Closed blocks -> Terminal -> Eof | message_delta stop_reason end_turn, stop_sequence, or structurally validated tool_use; message_stop | Truncation, max_tokens, refusal/pause/unknown reasons, error envelopes/events, unfinished JSON/SSE, transport error or cancellation grant nothing |
// | Terminal -> Delivered | Real asynchronous entity EOF, strict original/client/final proof, then sealed JSON activation or SSE continuation after the last yield | No success sentinel, partial privileged call, early publication, or terminal-only authority |
// | Delivered -> Replayed | Exact ordered native blocks/calls; client tool_result text remains untrusted on first and edited replay | Missing/evicted/mismatched session, provider, protocol, store, retry, or unknown references grant no history privilege |
// Unsupported: media/redacted payloads, signatures as readable text, unknown carriers/framing, cross-protocol conversion, WebSockets, and synthetic provider acceptance of signatures.

struct An2Fixture {
    store: DlpTokenStore,
    lease: DlpRequestLease,
    binding: crate::proxy::security::completed_context::ActualAttemptBinding,
    attempt: crate::proxy::security::restoration_core::AttemptProvenance,
    budget: PayloadBudget,
    reference: String,
}

fn an2_fixture(mapped: bool) -> TestResult<An2Fixture> {
    use crate::proxy::security::tokenization::{TokenCategory, TokenFragment};
    let store = DlpTokenStore::new();
    let mut lease = store.admit_scoped_published_policy(
        ContinuityScope::Native {
            client: ClientProtocol::new(Protocol::Anthropic),
            ingress: IngressNamespace::Direct,
        },
        "synthetic-an2-app",
        Some("synthetic-an2-session"),
        1,
    )?;
    let reference = if mapped {
        lease.prepare(
            &[TokenFragment {
                raw: "13900000000",
                whole_span_categories: &[TokenCategory::Phone],
            }],
            &[],
            |tokens| Ok(tokens[0].clone()),
        )?
    } else {
        "Synthetic public Anthropic text".to_owned()
    };
    let binding = lease.bind_actual_attempt(
        "synthetic-an2-provider",
        "anthropic",
        "https://synthetic.invalid/v1/messages",
        Some("synthetic-an2-principal"),
    )?;
    let budget = PayloadBudget::with_limit(4 * 1024 * 1024);
    let attempt = crate::proxy::security::restoration_core::AttemptProvenance::new(&budget)?;
    Ok(An2Fixture {
        store,
        lease,
        binding,
        attempt,
        budget,
        reference,
    })
}

fn an2_counts(fixture: &An2Fixture) -> TestResult<(usize, usize, usize)> {
    let (published, pending, pins, ..) =
        crate::proxy::security::tokenization::publication_test_support::counts(&fixture.store)?;
    Ok((published, pending, pins))
}

fn an2_json_source(reference: &str) -> Value {
    json!({"id":"msg_synthetic_an2", "type":"message", "role":"assistant",
        "model":"synthetic-model", "content":[{"type":"text", "text":reference}],
        "stop_reason":"end_turn", "stop_sequence":null,
        "usage":{"input_tokens":1,"output_tokens":2},
        "extension":{"same_value":reference,"types":[true,null,7,1.0]}})
}

#[test]
fn an2_lane_json_text_restores_editable_text_without_staging_publication() -> TestResult {
    let fixture = an2_fixture(true)?;
    let source = an2_json_source(&fixture.reference);
    let original = source.clone();
    let result = super::response::stage_json(
        &source,
        &fixture.lease,
        &fixture.binding,
        fixture.budget.clone(),
        &fixture.attempt,
    );
    assert!(
        result.is_ok(),
        "an2_json_native_text_staging_behavior_missing"
    );
    let staged = result.map_err(|_| "an2_json_native_text_stage_rejected")?;
    assert!(
        staged.as_value()["content"][0]["text"].as_str() == Some("13900000000")
            && staged.delivered() == &staged.as_value()["content"]
            && staged.restored_occurrences() == 1
            && staged.output_tokens().len() == 1
            && staged.as_value()["extension"] == source["extension"]
            && source == original
            && an2_counts(&fixture)? == (0, 0, 0),
        "an2_json_native_text_or_staging_boundary_invalid"
    );
    Ok(())
}

// Scoped-access reproducer: this must use the real HTTP driver and opaque proof.
// A Chat proof or a locally fabricated VerifiedNativeJson is not an acceptable substitute.
#[tokio::test]
async fn an2_reproducer_real_anthropic_json_proof_access() -> TestResult {
    let fixture = an2_fixture(false)?;
    let source = an2_json_source(&fixture.reference);
    let encoded = bytes::Bytes::from(serde_json::to_vec(&source)?);
    let proof =
        crate::proxy::security::http_restoration::test_transport::native_json_proof_for_pair(
            crate::proxy::security::restoration_contract::ProtocolPair::new(
                ClientProtocol::new(Protocol::Anthropic),
                crate::proxy::security::restoration_contract::UpstreamProtocol::new(
                    Protocol::Anthropic,
                ),
            ),
            &fixture.attempt,
            &fixture.binding,
            &fixture.budget,
            encoded,
            &source,
        )
        .await;
    assert!(
        proof.is_ok(),
        "an2_real_anthropic_proof_unavailable_from_chat_only_test_transport"
    );
    Ok(())
}

#[test]
fn an2_lane_json_tool_input_restores_recursive_string_leaves_preserving_types() -> TestResult {
    let fixture = an2_fixture(true)?;
    let mut source = an2_json_source(&fixture.reference);
    source["stop_reason"] = json!("tool_use");
    source["content"] = json!([{
        "type":"tool_use", "id":"toolu_synthetic_an2", "name":"synthetic_tool",
        "input":{
            "query":fixture.reference,
            "nested":[true,null,7,1.0,{"leaf":fixture.reference}],
            "opaque":{"same_value":fixture.reference,"types":[false,8]}
        }
    }]);
    let original = source.clone();
    let result = super::response::stage_json(
        &source,
        &fixture.lease,
        &fixture.binding,
        fixture.budget.clone(),
        &fixture.attempt,
    );
    assert!(
        result.is_ok(),
        "an2_json_tool_input_staging_behavior_missing"
    );
    let staged = result.map_err(|_| "an2_json_tool_input_stage_rejected")?;
    let expected = json!({
        "query":"13900000000",
        "nested":[true,null,7,1.0,{"leaf":"13900000000"}],
        "opaque":{"same_value":"13900000000","types":[false,8]}
    });
    assert!(
        staged.as_value()["content"][0]["input"] == expected
            && staged.delivered() == &staged.as_value()["content"]
            && staged.restored_occurrences() == 3
            && staged.output_tokens().len() == 1
            && source == original
            && an2_counts(&fixture)? == (0, 0, 0),
        "an2_json_tool_input_native_leaves_types_or_staging_invalid"
    );
    Ok(())
}

#[test]
fn an2_lane_json_thinking_is_immutable_while_signature_and_redaction_stay_opaque() -> TestResult {
    let fixture = an2_fixture(true)?;
    let baseline = fixture.budget.used();
    let mut mapped = an2_json_source("Synthetic public Anthropic text");
    mapped["content"] = json!([{
        "type":"thinking", "thinking":fixture.reference, "signature":fixture.reference
    }]);
    let original = mapped.clone();
    assert!(
        super::response::stage_json(
            &mapped,
            &fixture.lease,
            &fixture.binding,
            fixture.budget.clone(),
            &fixture.attempt,
        )
        .is_err(),
        "an2_json_mapped_immutable_thinking_was_silently_ignored_or_edited"
    );
    assert!(
        mapped == original
            && fixture.budget.used() == baseline
            && an2_counts(&fixture)? == (0, 0, 0),
        "an2_json_immutable_failure_mutated_or_published"
    );

    let mut opaque = an2_json_source("Synthetic public Anthropic text");
    opaque["content"] = json!([
        {"type":"thinking", "thinking":"Synthetic public reasoning", "signature":fixture.reference},
        {"type":"redacted_thinking", "data":fixture.reference}
    ]);
    let staged = super::response::stage_json(
        &opaque,
        &fixture.lease,
        &fixture.binding,
        fixture.budget.clone(),
        &fixture.attempt,
    )
    .map_err(|_| "an2_json_opaque_thinking_payload_rejected")?;
    assert!(
        !staged.changed()
            && staged.as_value()["content"] == opaque["content"]
            && staged.restored_occurrences() == 0
            && staged.output_tokens().is_empty()
            && an2_counts(&fixture)? == (0, 0, 0),
        "an2_json_signature_or_redacted_payload_became_readable"
    );
    Ok(())
}

#[test]
fn an2_lane_json_negative_reason_shape_error_budget_and_binding_fail_atomically() -> TestResult {
    let fixture = an2_fixture(true)?;
    let baseline = fixture.budget.used();
    for mode in 0..16 {
        let mut source = an2_json_source(&fixture.reference);
        match mode {
            0 => source["stop_reason"] = json!("max_tokens"),
            1 => source["stop_reason"] = json!("refusal"),
            2 => source["stop_reason"] = json!("pause_turn"),
            3 => source["stop_reason"] = json!("model_context_window_exceeded"),
            4 => source["stop_reason"] = json!("synthetic_unknown"),
            5 => source["stop_reason"] = Value::Null,
            6 => source["type"] = json!("error"),
            7 => source["error"] = json!({"type":"synthetic_error"}),
            8 => source["id"] = Value::Null,
            9 => source["role"] = json!("user"),
            10 => source["content"] = json!("truncated"),
            11 => source["stop_reason"] = json!("tool_use"),
            12 => {
                source["content"] = json!([{"type":"tool_use","id":"toolu_an2",
                    "name":"synthetic_tool","input":{}}])
            }
            13 => {
                source["stop_reason"] = json!("tool_use");
                source["content"] = json!([{"type":"tool_use","id":"toolu_an2",
                    "name":"synthetic_tool","input":[]}])
            }
            14 => source["content"] = json!([{"type":"synthetic_future","text":"opaque"}]),
            _ => {
                source["stop_reason"] = json!("tool_use");
                source["content"] = json!([
                    {"type":"tool_use","id":"toolu_same","name":"a","input":{}},
                    {"type":"tool_use","id":"toolu_same","name":"b","input":{}}
                ])
            }
        }
        let original = source.clone();
        assert!(
            super::response::stage_json(
                &source,
                &fixture.lease,
                &fixture.binding,
                fixture.budget.clone(),
                &fixture.attempt,
            )
            .is_err(),
            "an2_json_invalid_native_response_accepted"
        );
        assert!(
            source == original
                && fixture.budget.used() == baseline
                && an2_counts(&fixture)? == (0, 0, 0),
            "an2_json_invalid_stage_not_atomic"
        );
    }

    let source = an2_json_source(&fixture.reference);
    let tiny = PayloadBudget::with_limit(1);
    assert!(
        super::response::stage_json(
            &source,
            &fixture.lease,
            &fixture.binding,
            tiny.clone(),
            &fixture.attempt,
        )
        .is_err()
            && tiny.used() == 0,
        "an2_json_budget_failure_not_refunded"
    );
    let other = an2_fixture(false)?;
    assert!(
        super::response::stage_json(
            &source,
            &fixture.lease,
            &other.binding,
            fixture.budget.clone(),
            &fixture.attempt,
        )
        .is_err()
            && fixture.budget.used() == baseline
            && an2_counts(&fixture)? == (0, 0, 0)
            && an2_counts(&other)? == (0, 0, 0),
        "an2_json_foreign_binding_accepted_or_changed_state"
    );
    Ok(())
}

#[tokio::test]
async fn an2_lane_json_real_pair_proof_sealed_activation_precedes_body() -> TestResult {
    use crate::proxy::security::{
        http_restoration::test_transport,
        response_restoration::finalize_native_json,
        restoration_contract::{ProtocolPair, UpstreamProtocol},
    };
    use bytes::Bytes;
    use futures::StreamExt;

    let fixture = an2_fixture(true)?;
    let mut source = an2_json_source(&fixture.reference);
    source["stop_reason"] = json!("tool_use");
    source["content"] = json!([
        {"type":"text","text":fixture.reference},
        {"type":"thinking","thinking":"Synthetic public reasoning","signature":fixture.reference},
        {"type":"tool_use","id":"toolu_an2_json","name":"synthetic_tool",
            "input":{"query":fixture.reference,"types":[true,null,7,1.0]}}
    ]);
    let wire = Bytes::from(serde_json::to_vec_pretty(&source)?);
    let pair = ProtocolPair::new(
        ClientProtocol::new(Protocol::Anthropic),
        UpstreamProtocol::new(Protocol::Anthropic),
    );
    let proof = test_transport::native_json_proof_for_pair(
        pair,
        &fixture.attempt,
        &fixture.binding,
        &fixture.budget,
        wire.clone(),
        &source,
    )
    .await
    .map_err(|_| "an2_json_real_pair_proof_rejected")?;
    let staged = super::response::stage_json(
        &source,
        &fixture.lease,
        &fixture.binding,
        fixture.budget.clone(),
        &fixture.attempt,
    )
    .map_err(|_| "an2_json_real_pair_stage_rejected")?;
    assert!(
        an2_counts(&fixture)? == (0, 0, 0),
        "an2_json_real_pair_staging_published"
    );
    let finalized = finalize_native_json(
        staged,
        &fixture.lease,
        proof,
        &fixture.attempt,
        wire,
        1024 * 1024,
    )
    .map_err(|_| "an2_json_real_pair_finalization_rejected")?;
    assert!(
        an2_counts(&fixture)? == (1, 0, 0),
        "an2_json_sealed_activation_did_not_precede_body"
    );
    let mut delivery = finalized.into_delivery_stream();
    let body = delivery.next().await.ok_or("an2_json_delivery_missing")??;
    assert!(
        delivery.next().await.is_none(),
        "an2_json_delivery_not_single_body"
    );
    let actual: Value = serde_json::from_slice(&body)?;
    assert!(
        actual["content"][0]["text"].as_str() == Some("13900000000")
            && actual["content"][2]["input"]["query"].as_str() == Some("13900000000")
            && actual["content"][2]["input"]["types"] == json!([true, null, 7, 1.0])
            && actual["content"][1]["signature"] == source["content"][1]["signature"]
            && actual["extension"] == source["extension"]
            && an2_counts(&fixture)? == (1, 0, 0),
        "an2_json_real_pair_final_native_delivery_invalid"
    );
    Ok(())
}

// AN2 SSE consumes real pair-aware EOF proof; publication remains pending through every yield.
fn an2_sse_wire(events: &[Value]) -> TestResult<String> {
    let mut wire = String::new();
    for event in events {
        let kind = event["type"]
            .as_str()
            .ok_or("an2_sse_fixture_type_missing")?;
        wire.push_str(&format!(
            "event: {kind}\ndata: {}\n\n",
            serde_json::to_string(event)?
        ));
    }
    Ok(wire)
}

fn an2_sse_text_events(reference: &str, split: usize) -> Vec<Value> {
    let (left, right) = reference.split_at(split);
    vec![
        json!({"type":"message_start","message":{"id":"msg_synthetic_an2_sse","type":"message","role":"assistant","model":"synthetic-model","content":[],"stop_reason":null,"stop_sequence":null,"usage":{"input_tokens":1,"output_tokens":0}}}),
        json!({"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}),
        json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":left}}),
        json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":right}}),
        json!({"type":"content_block_stop","index":0}),
        json!({"type":"message_delta","delta":{"stop_reason":"end_turn","stop_sequence":null},"usage":{"output_tokens":2}}),
        json!({"type":"message_stop"}),
    ]
}

fn an2_finalize_sse(
    fixture: &An2Fixture,
    wire: &str,
) -> TestResult<crate::proxy::security::response_restoration::FinalizedNativeSse> {
    use crate::proxy::security::{
        http_restoration::test_transport,
        restoration_contract::{ProtocolPair, UpstreamProtocol},
    };
    let proof_result = futures::executor::block_on(test_transport::native_sse_proof_for_pair(
        ProtocolPair::new(
            ClientProtocol::new(Protocol::Anthropic),
            UpstreamProtocol::new(Protocol::Anthropic),
        ),
        &fixture.attempt,
        &fixture.binding,
        &fixture.budget,
        bytes::Bytes::copy_from_slice(wire.as_bytes()),
        bytes::Bytes::copy_from_slice(wire.as_bytes()),
    ));
    assert!(
        proof_result.is_ok(),
        "an2_sse_fixture_real_pair_proof_rejected"
    );
    let proof = proof_result.map_err(|_| "an2_sse_real_pair_proof_rejected")?;
    super::response::finalize_sse(
        wire.as_bytes(),
        &fixture.lease,
        &fixture.binding,
        fixture.budget.clone(),
        &fixture.attempt,
        proof,
        1024 * 1024,
    )
    .map_err(|_| "an2_sse_native_consumer_behavior_missing".into())
}

#[test]
fn an2_lane_sse_text_splits_restore_only_with_last_yield_continuation() -> TestResult {
    use futures::StreamExt;
    let reference_len = an2_fixture(true)?.reference.len();
    for split in 0..=reference_len {
        let fixture = an2_fixture(true)?;
        let baseline = fixture.budget.used();
        let events = an2_sse_text_events(&fixture.reference, split);
        let wire = an2_sse_wire(&events)?;
        let result = an2_finalize_sse(&fixture, &wire);
        assert!(result.is_ok(), "an2_sse_native_consumer_behavior_missing");
        let mut stream = result?.into_delivery_stream();
        let mut restored = String::new();
        for _ in 0..events.len() {
            assert!(
                an2_counts(&fixture)? == (0, 1, 1),
                "an2_sse_published_before_yield"
            );
            let chunk =
                futures::executor::block_on(stream.next()).ok_or("an2_sse_frame_missing")??;
            for data in std::str::from_utf8(&chunk)?
                .lines()
                .filter_map(|line| line.strip_prefix("data: "))
            {
                let source: Value = serde_json::from_str(data)?;
                if source["delta"]["type"] == "text_delta" {
                    restored.push_str(
                        source["delta"]["text"]
                            .as_str()
                            .ok_or("an2_sse_text_missing")?,
                    );
                }
            }
            assert!(
                an2_counts(&fixture)? == (0, 1, 1),
                "an2_sse_published_after_yield"
            );
        }
        assert!(
            restored == "13900000000",
            "an2_sse_split_reference_not_restored"
        );
        assert!(
            futures::executor::block_on(stream.next()).is_none(),
            "an2_sse_extra_delivery"
        );
        drop(stream);
        assert!(
            an2_counts(&fixture)? == (1, 0, 0) && fixture.budget.used() == baseline,
            "an2_sse_continuation_or_budget_refund_invalid"
        );
    }
    Ok(())
}

fn an2_sse_block_events(reference: &str, split: usize) -> TestResult<Vec<Value>> {
    let mut events = an2_sse_text_events(reference, split);
    events.truncate(5);
    let arguments = serde_json::to_string(
        &json!({"query":reference,"nested":[true,null,7,1.0,{"leaf":reference}]}),
    )?;
    let at = arguments.len() / 2;
    events.extend([
        json!({"type":"content_block_start","index":1,"content_block":{"type":"thinking","thinking":"Synthetic ","signature":""}}),
        json!({"type":"content_block_delta","index":1,"delta":{"type":"thinking_delta","thinking":"public thinking"}}),
        json!({"type":"content_block_delta","index":1,"delta":{"type":"signature_delta","signature":reference}}),
        json!({"type":"content_block_stop","index":1}),
        json!({"type":"content_block_start","index":2,"content_block":{"type":"redacted_thinking","data":reference}}),
        json!({"type":"content_block_stop","index":2}),
        json!({"type":"content_block_start","index":3,"content_block":{"type":"tool_use","id":"toolu_synthetic_an2_streamed","name":"synthetic_tool","input":{}}}),
        json!({"type":"content_block_delta","index":3,"delta":{"type":"input_json_delta","partial_json":&arguments[..at]}}),
        json!({"type":"content_block_delta","index":3,"delta":{"type":"input_json_delta","partial_json":&arguments[at..]}}),
        json!({"type":"content_block_stop","index":3}),
        json!({"type":"content_block_start","index":4,"content_block":{"type":"tool_use","id":"toolu_synthetic_an2_object","name":"synthetic_object","input":{"query":reference,"types":[true,null,7,1.0],"nested":{"leaf":reference}}}}),
        json!({"type":"content_block_stop","index":4}),
        json!({"type":"message_delta","delta":{"stop_reason":"tool_use","stop_sequence":null},"usage":{"output_tokens":8}}),
        json!({"type":"message_stop"}),
    ]);
    Ok(events)
}

#[test]
fn an2_lane_sse_ordered_blocks_tools_and_late_signature_preserve_native_units() -> TestResult {
    use crate::proxy::security::{
        native_test_support::record_delivery, restoration_core::compose_anthropic_snapshots,
    };
    let fixture = an2_fixture(true)?;
    let baseline = fixture.budget.used();
    let events = an2_sse_block_events(&fixture.reference, fixture.reference.len() / 2)?;
    let wire = an2_sse_wire(&events)?;
    let result = an2_finalize_sse(&fixture, &wire);
    assert!(
        result.is_ok(),
        "an2_sse_ordered_block_consumer_behavior_missing"
    );
    assert!(
        an2_counts(&fixture)? == (0, 1, 1),
        "an2_sse_blocks_published_before_delivery"
    );
    let chunks = futures::executor::block_on(record_delivery(result?.into_delivery_stream()))?;
    let mut delivered = Vec::new();
    for chunk in &chunks {
        for data in std::str::from_utf8(chunk)?
            .lines()
            .filter_map(|line| line.strip_prefix("data: "))
        {
            delivered.push(serde_json::from_str::<Value>(data)?);
        }
    }
    let references: Vec<&Value> = delivered.iter().collect();
    let composed = compose_anthropic_snapshots(&references, &fixture.budget)
        .map_err(|_| "an2_sse_test_native_composition_failed")?;
    let mut expected = json!([
        {"type":"text","text":"13900000000"},
        {"type":"thinking","thinking":"Synthetic public thinking","signature":fixture.reference},
        {"type":"redacted_thinking","data":fixture.reference},
        {"type":"tool_use","id":"toolu_synthetic_an2_streamed","name":"synthetic_tool","input":{"query":"13900000000","nested":[true,null,7,1.0,{"leaf":"13900000000"}]}},
        {"type":"tool_use","id":"toolu_synthetic_an2_object","name":"synthetic_object","input":{"query":"13900000000","types":[true,null,7,1.0],"nested":{"leaf":"13900000000"}}}
    ]);
    assert!(
        composed.as_value() == &expected && an2_counts(&fixture)? == (1, 0, 0),
        "an2_sse_ordered_native_composition_or_opaque_units_changed"
    );
    expected = Value::Null;
    drop(expected);
    drop(composed);
    drop(chunks);
    assert!(
        fixture.budget.used() == baseline,
        "an2_sse_block_budget_not_refunded"
    );
    Ok(())
}

fn an2_sse_proof(
    fixture: &An2Fixture,
    original: &str,
    client: &str,
) -> Result<
    crate::proxy::security::http_restoration::VerifiedNativeSse,
    crate::proxy::security::response_restoration::RestoreError,
> {
    use crate::proxy::security::{
        http_restoration::test_transport,
        restoration_contract::{ProtocolPair, UpstreamProtocol},
    };
    futures::executor::block_on(test_transport::native_sse_proof_for_pair(
        ProtocolPair::new(
            ClientProtocol::new(Protocol::Anthropic),
            UpstreamProtocol::new(Protocol::Anthropic),
        ),
        &fixture.attempt,
        &fixture.binding,
        &fixture.budget,
        bytes::Bytes::copy_from_slice(original.as_bytes()),
        bytes::Bytes::copy_from_slice(client.as_bytes()),
    ))
}

#[test]
fn an2_lane_sse_invalid_states_and_original_client_final_mismatch_release_nothing() -> TestResult {
    let fixture = an2_fixture(true)?;
    let baseline = fixture.budget.used();
    let valid = an2_sse_text_events(&fixture.reference, fixture.reference.len() / 2);
    let wire = an2_sse_wire(&valid)?;
    drop(an2_finalize_sse(&fixture, &wire)?);
    assert!(
        an2_counts(&fixture)? == (0, 0, 0) && fixture.budget.used() == baseline,
        "an2_sse_negative_control_not_clean"
    );
    let mut cases = Vec::new();
    let mut events = valid.clone();
    events[1]["index"] = json!(1);
    cases.push(("start_index", events));
    let mut events = valid.clone();
    events[2]["index"] = json!(1);
    cases.push(("delta_index", events));
    let mut events = valid.clone();
    events[4]["index"] = json!(1);
    cases.push(("stop_index", events));
    let mut events = valid.clone();
    events[2]["delta"] = json!({"type":"thinking_delta","thinking":"public"});
    cases.push(("delta_kind", events));
    let mut events = valid.clone();
    events.insert(2, valid[1].clone());
    cases.push(("open_start", events));
    let mut events = valid.clone();
    events.insert(5, valid[4].clone());
    cases.push(("duplicate_stop", events));
    let mut events = valid.clone();
    events.remove(4);
    cases.push(("open_terminal", events));
    let mut events = valid.clone();
    events.remove(0);
    cases.push(("missing_start", events));
    let mut events = valid.clone();
    events.insert(1, valid[0].clone());
    cases.push(("duplicate_start", events));
    let mut events = valid.clone();
    events.pop();
    cases.push(("missing_message_stop", events));
    let mut events = valid.clone();
    events.remove(5);
    cases.push(("missing_reason", events));
    let mut events = valid.clone();
    events.insert(6, valid[5].clone());
    cases.push(("duplicate_reason", events));
    let mut events = valid.clone();
    events.push(valid[1].clone());
    cases.push(("post_terminal_content", events));
    let mut events = valid.clone();
    events.push(valid[6].clone());
    cases.push(("duplicate_terminal", events));
    let mut events = valid.clone();
    events[2]["delta"]["text"] = json!(7);
    cases.push(("malformed_text", events));
    let mut events = valid.clone();
    events[1]["content_block"]["extension"] = json!({"types":[true,null,7]});
    cases.push(("unsupported_carrier", events));
    let mut events = valid.clone();
    events.insert(
        3,
        json!({"type":"error","error":{"type":"synthetic_error","message":"Synthetic error"}}),
    );
    cases.push(("error_event", events));
    for reason in [
        "max_tokens",
        "pause_turn",
        "refusal",
        "unknown",
        "tool_use",
        "",
    ] {
        let mut events = valid.clone();
        events[5]["delta"]["stop_reason"] = json!(reason);
        cases.push(("invalid_reason", events));
    }
    let blocks = an2_sse_block_events(&fixture.reference, 0)?;
    let mut events = blocks.clone();
    events[13]["delta"]["partial_json"] = json!("}");
    cases.push(("malformed_partial_json", events));
    let mut events = blocks.clone();
    events[12]["delta"]["partial_json"] = json!("[]");
    events.remove(13);
    cases.push(("non_object_arguments", events));
    let mut events = blocks.clone();
    events[15]["content_block"]["id"] = json!("toolu_synthetic_an2_streamed");
    cases.push(("duplicate_call_id", events));
    let mut events = blocks.clone();
    events[17]["delta"]["stop_reason"] = json!("end_turn");
    cases.push(("tool_reason_mismatch", events));
    for (case, events) in cases {
        let malformed = an2_sse_wire(&events)?;
        assert!(
            an2_sse_proof(&fixture, &malformed, &malformed).is_err(),
            "an2_sse_invalid_http_state_accepted: {case}"
        );
        let proof = an2_sse_proof(&fixture, &wire, &wire)
            .map_err(|_| "an2_sse_valid_negative_control_failed")?;
        let result = super::response::finalize_sse(
            malformed.as_bytes(),
            &fixture.lease,
            &fixture.binding,
            fixture.budget.clone(),
            &fixture.attempt,
            proof,
            1024 * 1024,
        );
        assert!(
            result.is_err()
                && an2_counts(&fixture)? == (0, 0, 0)
                && fixture.budget.used() == baseline,
            "an2_sse_invalid_final_entity_retained_authority_or_budget: {case}"
        );
    }
    for malformed in [
        wire.trim_end().to_owned(),
        wire.replace("event: message_stop", "event: synthetic_mismatch"),
        format!("{wire}data: [DONE]\n\n"),
        format!("{wire}event: error\ndata: {{\"type\":\"error\"}}\n\n"),
    ] {
        assert!(
            an2_sse_proof(&fixture, &malformed, &malformed).is_err(),
            "an2_sse_framing_failure_accepted"
        );
        assert!(
            an2_counts(&fixture)? == (0, 0, 0) && fixture.budget.used() == baseline,
            "an2_sse_framing_failure_retained_authority_or_budget"
        );
    }
    let mut changed = valid.clone();
    changed[2]["delta"]["text"] = json!("Synthetic changed public text");
    let changed = an2_sse_wire(&changed)?;
    let proof = an2_sse_proof(&fixture, &wire, &changed);
    assert!(proof.is_err(), "an2_sse_original_client_mismatch_accepted");
    assert!(
        an2_counts(&fixture)? == (0, 0, 0) && fixture.budget.used() == baseline,
        "an2_sse_mismatch_retained_authority_or_budget"
    );
    Ok(())
}

#[test]
fn an2_lane_sse_thinking_is_immutable_before_any_late_signature() -> TestResult {
    let fixture = an2_fixture(true)?;
    let baseline = fixture.budget.used();
    for signed in [false, true] {
        let mut events = an2_sse_text_events(&fixture.reference, fixture.reference.len() / 2);
        events[1]["content_block"] = json!({"type":"thinking","thinking":""});
        events[2]["delta"] = json!({"type":"thinking_delta","thinking":&fixture.reference[..fixture.reference.len()/2]});
        events[3]["delta"] = json!({"type":"thinking_delta","thinking":&fixture.reference[fixture.reference.len()/2..]});
        if signed {
            events.insert(4, json!({"type":"content_block_delta","index":0,"delta":{"type":"signature_delta","signature":"synthetic-opaque-signature"}}));
        }
        let wire = an2_sse_wire(&events)?;
        let proof = an2_sse_proof(&fixture, &wire, &wire)
            .map_err(|_| "an2_sse_immutable_fixture_proof_failed")?;
        let result = super::response::finalize_sse(
            wire.as_bytes(),
            &fixture.lease,
            &fixture.binding,
            fixture.budget.clone(),
            &fixture.attempt,
            proof,
            1024 * 1024,
        );
        assert!(
            result.is_err()
                && an2_counts(&fixture)? == (0, 0, 0)
                && fixture.budget.used() == baseline,
            "an2_sse_thinking_reference_was_editable_or_retained_state"
        );
    }
    Ok(())
}

#[test]
fn an2_lane_sse_drop_at_each_yield_refunds_pending_publication() -> TestResult {
    use futures::StreamExt;
    let count = an2_sse_block_events("Synthetic public text", 0)?.len();
    for yields in 0..=count {
        let fixture = an2_fixture(true)?;
        let baseline = fixture.budget.used();
        let wire = an2_sse_wire(&an2_sse_block_events(
            &fixture.reference,
            fixture.reference.len() / 2,
        )?)?;
        let mut stream = an2_finalize_sse(&fixture, &wire)?.into_delivery_stream();
        for _ in 0..yields {
            drop(
                futures::executor::block_on(stream.next())
                    .ok_or("an2_sse_cancel_frame_missing")??,
            );
            assert!(
                an2_counts(&fixture)? == (0, 1, 1),
                "an2_sse_cancel_published_at_yield"
            );
        }
        drop(stream);
        assert!(
            an2_counts(&fixture)? == (0, 0, 0) && fixture.budget.used() == baseline,
            "an2_sse_cancel_retained_publication_or_budget"
        );
    }
    Ok(())
}

#[test]
fn an2_lane_sse_binding_attempt_entity_and_budget_isolation() -> TestResult {
    use crate::proxy::security::restoration_core::AttemptProvenance;
    let fixture = an2_fixture(true)?;
    let other = an2_fixture(true)?;
    let baseline = fixture.budget.used();
    let wire = an2_sse_wire(&an2_sse_text_events(&fixture.reference, 0))?;
    for limit in [0, 1, 64, 256, 1024] {
        let proof =
            an2_sse_proof(&fixture, &wire, &wire).map_err(|_| "an2_sse_budget_control_failed")?;
        let tiny = PayloadBudget::with_limit(limit);
        let result = super::response::finalize_sse(
            wire.as_bytes(),
            &fixture.lease,
            &fixture.binding,
            tiny.clone(),
            &fixture.attempt,
            proof,
            1024 * 1024,
        );
        assert!(
            result.is_err()
                && tiny.used() == 0
                && fixture.budget.used() == baseline
                && an2_counts(&fixture)? == (0, 0, 0),
            "an2_sse_budget_exhaustion_not_atomic"
        );
    }
    let proof =
        an2_sse_proof(&fixture, &wire, &wire).map_err(|_| "an2_sse_binding_control_failed")?;
    let result = super::response::finalize_sse(
        wire.as_bytes(),
        &fixture.lease,
        &other.binding,
        fixture.budget.clone(),
        &fixture.attempt,
        proof,
        1024 * 1024,
    );
    assert!(
        result.is_err()
            && fixture.budget.used() == baseline
            && an2_counts(&fixture)? == (0, 0, 0)
            && an2_counts(&other)? == (0, 0, 0),
        "an2_sse_foreign_binding_accepted"
    );
    let proof =
        an2_sse_proof(&fixture, &wire, &wire).map_err(|_| "an2_sse_attempt_control_failed")?;
    let different_attempt = AttemptProvenance::new(&fixture.budget)?;
    let result = super::response::finalize_sse(
        wire.as_bytes(),
        &fixture.lease,
        &fixture.binding,
        fixture.budget.clone(),
        &different_attempt,
        proof,
        1024 * 1024,
    );
    drop(different_attempt);
    assert!(
        result.is_err() && fixture.budget.used() == baseline && an2_counts(&fixture)? == (0, 0, 0),
        "an2_sse_foreign_attempt_accepted"
    );
    let proof =
        an2_sse_proof(&fixture, &wire, &wire).map_err(|_| "an2_sse_entity_control_failed")?;
    let mut changed = an2_sse_text_events(&fixture.reference, 0);
    changed[2]["delta"]["text"] = json!("Synthetic changed public text");
    let changed = an2_sse_wire(&changed)?;
    let result = super::response::finalize_sse(
        changed.as_bytes(),
        &fixture.lease,
        &fixture.binding,
        fixture.budget.clone(),
        &fixture.attempt,
        proof,
        1024 * 1024,
    );
    assert!(
        result.is_err() && fixture.budget.used() == baseline && an2_counts(&fixture)? == (0, 0, 0),
        "an2_sse_foreign_final_entity_accepted"
    );
    Ok(())
}

#[test]
fn an2_lane_sse_unchanged_preserves_crlf_comments_and_native_payload_bytes() -> TestResult {
    use crate::proxy::security::native_test_support::record_delivery;
    let fixture = an2_fixture(false)?;
    let baseline = fixture.budget.used();
    let wire = format!(
        ": synthetic-comment\r\nid: synthetic-id\r\n\r\n{}",
        an2_sse_wire(&an2_sse_block_events(&fixture.reference, 0)?)?.replace('\n', "\r\n")
    );
    let chunks = futures::executor::block_on(record_delivery(
        an2_finalize_sse(&fixture, &wire)?.into_delivery_stream(),
    ))?;
    let bytes: Vec<u8> = chunks
        .iter()
        .flat_map(|chunk| chunk.iter().copied())
        .collect();
    assert!(
        bytes == wire.as_bytes(),
        "an2_sse_unchanged_native_wire_changed"
    );
    assert!(
        an2_counts(&fixture)? == (1, 0, 0),
        "an2_sse_zero_map_publication_invalid"
    );
    assert!(
        fixture.budget.used() > baseline,
        "an2_sse_live_bytes_lost_budget_owner"
    );
    drop(chunks);
    assert!(
        fixture.budget.used() == baseline,
        "an2_sse_released_bytes_budget_not_refunded"
    );
    Ok(())
}

// AN3 state table: exact ordered delivered blocks/calls are lookup evidence only;
// first/edited tool_result text stays untrusted. Native tool_use_id is structural
// metadata, never execution proof. Thinking/signature retain original boundaries.
fn an3_next(
    fixture: &An2Fixture,
    session: Option<&str>,
    client: Protocol,
    provider: &str,
) -> TestResult<(
    DlpRequestLease,
    crate::proxy::security::completed_context::ActualAttemptBinding,
)> {
    let lease = fixture.store.admit_scoped_published_policy(
        ContinuityScope::Native {
            client: ClientProtocol::new(client),
            ingress: IngressNamespace::Direct,
        },
        "synthetic-an2-app",
        session,
        fixture.binding.policy_revision,
    )?;
    let binding = lease.bind_actual_attempt(
        provider,
        "anthropic",
        "https://synthetic.invalid/v1/messages",
        Some("synthetic-an2-principal"),
    )?;
    Ok((lease, binding))
}

fn an3_source(reference: &str) -> Value {
    let mut source = an2_json_source(reference);
    source["content"] = json!([
        {"type":"text","text":reference},
        {"type":"thinking","thinking":"Synthetic public reasoning","signature":"synthetic-opaque-signature"},
        {"type":"tool_use","id":"toolu_synthetic_an3","name":"synthetic_tool","input":{"query":reference,"types":[7,true,null]}}
    ]);
    source["stop_reason"] = json!("tool_use");
    source
}

fn an3_deliver_json(fixture: &An2Fixture, source: &Value) -> TestResult<Value> {
    use crate::proxy::security::{
        http_restoration::test_transport,
        native_test_support::record_delivery,
        response_restoration::finalize_native_json,
        restoration_contract::{ProtocolPair, UpstreamProtocol},
    };
    let wire = bytes::Bytes::from(serde_json::to_vec(source)?);
    let proof = futures::executor::block_on(test_transport::native_json_proof_for_pair(
        ProtocolPair::new(
            ClientProtocol::new(Protocol::Anthropic),
            UpstreamProtocol::new(Protocol::Anthropic),
        ),
        &fixture.attempt,
        &fixture.binding,
        &fixture.budget,
        wire.clone(),
        source,
    ))
    .map_err(|_| "an3_real_pair_proof_rejected")?;
    let stage = super::response::stage_json(
        source,
        &fixture.lease,
        &fixture.binding,
        fixture.budget.clone(),
        &fixture.attempt,
    )
    .map_err(|_| "an3_stage_rejected")?;
    let finalized = finalize_native_json(
        stage,
        &fixture.lease,
        proof,
        &fixture.attempt,
        wire,
        1024 * 1024,
    )
    .map_err(|_| "an3_finalizer_rejected")?;
    assert!(
        an2_counts(fixture)? == (1, 0, 0),
        "an3_json_not_active_before_body"
    );
    let chunks = futures::executor::block_on(record_delivery(finalized.into_delivery_stream()))?;
    let response: Value = serde_json::from_slice(&chunks[0])?;
    Ok(response["content"].clone())
}

#[test]
fn an3_lane_exact_ordered_block_call_and_signed_two_turn_replay() -> TestResult {
    let fixture = an2_fixture(true)?;
    let source = an3_source(&fixture.reference);
    let delivered = an3_deliver_json(&fixture, &source)?;
    let (mut next, binding) = an3_next(
        &fixture,
        Some("synthetic-an2-session"),
        Protocol::Anthropic,
        "synthetic-an2-provider",
    )?;
    assert!(
        next.resolve(&fixture.reference).is_err(),
        "an3_fresh_lease_has_authority"
    );
    let result = super::history::import_delivered(&mut next, &binding, &delivered);
    assert!(result.is_ok(), "an3_exact_delivered_import_missing");
    result?;
    assert!(
        next.resolve(&fixture.reference)? == "13900000000",
        "an3_exact_dependency_not_imported"
    );
    super::history::import_delivered(&mut next, &binding, &delivered)?;
    let request = json!({"messages":[{"role":"assistant","content":delivered},
        {"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_synthetic_an3","content":"Synthetic public result"}]}]});
    let budget = PayloadBudget::with_limit(BUDGET_BYTES);
    let observed = super::history::observe_request(&request, budget.clone())?;
    // The default irreversible phone policy conflicts with an imported dependency.
    // This is a policy control, not a production replay failure.
    let conflict = prepare_observed_request(
        &mut next,
        &observed,
        &SecurityRuntimeConfig::default(),
        &[],
        &[],
        |_, _, _| Ok(()),
    );
    assert!(
        matches!(conflict, Err(PlanError::PolicyConflict)),
        "an3_expected_policy_conflict_missing"
    );
    let baseline = budget.used();
    let prepared = prepare_observed_request(
        &mut next,
        &observed,
        &SecurityRuntimeConfig::default(),
        &["chinese_phone_number"],
        &[],
        |_, _, _| Ok(()),
    )
    .map_err(|_| "an3_signed_replay_preparation_rejected")?;
    let mut expected = request.clone();
    expected["messages"][0]["content"][0]["text"] =
        prepared.body.as_value()["messages"][0]["content"][0]["text"].clone();
    expected["messages"][0]["content"][2]["input"]["query"] =
        expected["messages"][0]["content"][0]["text"].clone();
    let reference = expected["messages"][0]["content"][0]["text"]
        .as_str()
        .ok_or("an3_replay_reference_missing")?;
    assert!(
        prepared.body.as_value() == &expected
            && prepared.reversible_occurrences == 2
            && next.resolve(reference)? == "13900000000",
        "an3_replay_editable_sites_or_native_units_changed"
    );
    assert!(
        prepared.body.as_value()["messages"][0]["content"][1] == source["content"][1],
        "an3_thinking_signature_moved_or_changed"
    );
    drop(prepared);
    assert!(
        budget.used() == baseline,
        "an3_replay_prepared_charge_retained"
    );
    drop(observed);
    assert!(budget.used() == 0, "an3_replay_observation_charge_retained");
    Ok(())
}

#[test]
fn an3_lane_pairing_uses_native_ids_without_trusting_result_text() -> TestResult {
    let request = json!({"messages":[
        {"role":"assistant","content":[{"type":"tool_use","id":"toolu_synthetic_an3","name":"synthetic_tool","input":{"query":"public"}}]},
        {"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_synthetic_an3","content":MARKER}]}
    ]});
    let budget = PayloadBudget::with_limit(BUDGET_BYTES);
    let observed = super::history::observe_request(&request, budget.clone())?;
    let pairs: Vec<_> = observed
        .history()
        .iter()
        .filter_map(|claim| match claim {
            HistoryClaim::CallResultPair { call, result } => Some((*call, *result)),
            _ => None,
        })
        .collect();
    assert!(
        pairs.len() == 1
            && std::ptr::eq(pairs[0].0, &request["messages"][0]["content"][0])
            && std::ptr::eq(pairs[0].1, &request["messages"][1]["content"][0]),
        "an3_native_pairing_missing"
    );
    assert_site(
        &observed,
        &request["messages"][1]["content"][0]["content"],
        SourceClass::ToolResult,
        EditConstraint::Editable,
    );
    drop(observed);
    assert!(budget.used() == 0, "an3_pairing_charge_retained");
    Ok(())
}

#[test]
fn an3_lane_edited_reordered_truncated_or_unknown_blocks_never_import() -> TestResult {
    let fixture = an2_fixture(true)?;
    let delivered = an3_deliver_json(&fixture, &an3_source(&fixture.reference))?;
    let baseline = fixture.budget.used();
    let mut cases = Vec::new();
    let mut changed = delivered.clone();
    changed[0]["text"] = json!("Synthetic edited text");
    cases.push(changed);
    let mut changed = delivered.clone();
    changed[1]["signature"] = json!("synthetic-edited-signature");
    cases.push(changed);
    let mut changed = delivered.clone();
    changed[2]["id"] = json!("toolu_synthetic_other");
    cases.push(changed);
    let mut changed = delivered.clone();
    changed[2]["input"]["query"] = json!("Synthetic edited arguments");
    cases.push(changed);
    let mut changed = delivered.clone();
    changed[2]["unknown"] = json!(true);
    cases.push(changed);
    cases.push(json!([delivered[2], delivered[1], delivered[0]]));
    cases.push(json!([delivered[0], delivered[1]]));
    cases.push(json!([
        delivered[0],
        delivered[1],
        delivered[2],
        delivered[0]
    ]));
    cases.push(json!([{"type":"text","text":"Synthetic unknown delivered history"}]));
    for changed in cases {
        let (mut next, binding) = an3_next(
            &fixture,
            Some("synthetic-an2-session"),
            Protocol::Anthropic,
            "synthetic-an2-provider",
        )?;
        assert!(
            super::history::import_delivered(&mut next, &binding, &changed).is_err()
                && next.resolve(&fixture.reference).is_err(),
            "an3_inexact_delivered_authorized"
        );
        assert!(
            fixture.budget.used() == baseline,
            "an3_rejected_lookup_retained_budget"
        );
    }
    let (mut next, binding) = an3_next(
        &fixture,
        Some("synthetic-an2-session"),
        Protocol::Anthropic,
        "synthetic-an2-provider",
    )?;
    super::history::import_delivered(&mut next, &binding, &delivered)?;
    assert!(
        next.resolve(&fixture.reference).is_ok(),
        "an3_exact_control_not_imported"
    );
    Ok(())
}

#[test]
fn an3_lane_absent_session_cross_binding_store_and_retry_have_no_history_privilege() -> TestResult {
    let fixture = an2_fixture(true)?;
    let delivered = an3_deliver_json(&fixture, &an3_source(&fixture.reference))?;
    for (session, client, provider) in [
        (None, Protocol::Anthropic, "synthetic-an2-provider"),
        (
            Some("synthetic-other-session"),
            Protocol::Anthropic,
            "synthetic-an2-provider",
        ),
        (
            Some("synthetic-an2-session"),
            Protocol::Anthropic,
            "synthetic-other-provider",
        ),
        (
            Some("synthetic-an2-session"),
            Protocol::Gemini,
            "synthetic-an2-provider",
        ),
    ] {
        let (mut next, binding) = an3_next(&fixture, session, client, provider)?;
        assert!(
            super::history::import_delivered(&mut next, &binding, &delivered).is_err()
                && next.resolve(&fixture.reference).is_err(),
            "an3_cross_binding_import_accepted"
        );
    }
    for (format, endpoint, principal) in [
        (
            "gemini",
            "https://synthetic.invalid/v1/messages",
            Some("synthetic-an2-principal"),
        ),
        (
            "anthropic",
            "https://synthetic-other.invalid/v1/messages",
            Some("synthetic-an2-principal"),
        ),
        (
            "anthropic",
            "https://synthetic.invalid/v1/messages",
            Some("synthetic-other-principal"),
        ),
    ] {
        let (mut next, _) = an3_next(
            &fixture,
            Some("synthetic-an2-session"),
            Protocol::Anthropic,
            "synthetic-an2-provider",
        )?;
        let binding =
            next.bind_actual_attempt("synthetic-an2-provider", format, endpoint, principal)?;
        assert!(
            super::history::import_delivered(&mut next, &binding, &delivered).is_err()
                && next.resolve(&fixture.reference).is_err(),
            "an3_actual_attempt_mismatch_imported"
        );
    }
    let other = an2_fixture(false)?;
    let (mut next, binding) = an3_next(
        &other,
        Some("synthetic-an2-session"),
        Protocol::Anthropic,
        "synthetic-an2-provider",
    )?;
    assert!(
        super::history::import_delivered(&mut next, &binding, &delivered).is_err()
            && next.resolve(&fixture.reference).is_err(),
        "an3_cross_store_imported"
    );
    let (next, _) = an3_next(
        &fixture,
        Some("synthetic-an2-session"),
        Protocol::Anthropic,
        "synthetic-an2-provider",
    )?;
    let mut retry = next.fork_attempt()?;
    let binding = retry.bind_actual_attempt(
        "synthetic-retry-provider",
        "anthropic",
        "https://synthetic-retry.invalid/v1/messages",
        Some("synthetic-an2-principal"),
    )?;
    assert!(
        super::history::import_delivered(&mut retry, &binding, &delivered).is_err()
            && retry.resolve(&fixture.reference).is_err(),
        "an3_retry_inherited_other_attempt_authority"
    );
    Ok(())
}

#[test]
fn an3_lane_first_and_edited_result_text_always_get_current_dlp() -> TestResult {
    for text in ["13900000000", "Synthetic edited result 13900000000"] {
        let fixture = an2_fixture(true)?;
        let delivered = an3_deliver_json(&fixture, &an3_source(&fixture.reference))?;
        let (mut next, binding) = an3_next(
            &fixture,
            Some("synthetic-an2-session"),
            Protocol::Anthropic,
            "synthetic-an2-provider",
        )?;
        super::history::import_delivered(&mut next, &binding, &delivered)?;
        let request = json!({"messages":[
            {"role":"assistant","content":delivered},
            {"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_synthetic_an3","content":text}]}
        ]});
        let original = request.clone();
        let budget = PayloadBudget::with_limit(BUDGET_BYTES);
        let observed = super::history::observe_request(&request, budget.clone())?;
        assert!(
            observed
                .history()
                .iter()
                .any(|claim| matches!(claim, HistoryClaim::CallResultPair { .. })),
            "an3_result_pair_not_observed"
        );
        let prepared = prepare_observed_request(
            &mut next,
            &observed,
            &SecurityRuntimeConfig::default(),
            &["chinese_phone_number"],
            &[],
            |_, _, _| Ok(()),
        )
        .map_err(|_| "an3_result_current_dlp_rejected")?;
        let result = prepared.body.as_value()["messages"][1]["content"][0]["content"]
            .as_str()
            .ok_or("an3_prepared_result_missing")?;
        assert!(
            prepared.reversible_occurrences == 3
                && !result.contains("13900000000")
                && prepared.body.as_value()["messages"][0]["content"][1]
                    == original["messages"][0]["content"][1]
                && request == original
                && an2_counts(&fixture)? == (1, 0, 0),
            "an3_result_text_trusted_or_signed_unit_changed"
        );
        drop(prepared);
        drop(observed);
        assert!(budget.used() == 0, "an3_result_dlp_budget_retained");
    }
    Ok(())
}

#[test]
fn an3_lane_authorized_reference_in_thinking_fails_immutably_and_atomically() -> TestResult {
    let fixture = an2_fixture(true)?;
    let delivered = an3_deliver_json(&fixture, &an3_source(&fixture.reference))?;
    for signed in [false, true] {
        let (mut next, binding) = an3_next(
            &fixture,
            Some("synthetic-an2-session"),
            Protocol::Anthropic,
            "synthetic-an2-provider",
        )?;
        super::history::import_delivered(&mut next, &binding, &delivered)?;
        let mut thinking = json!({"type":"thinking","thinking":fixture.reference});
        if signed {
            thinking["signature"] = json!("synthetic-opaque-signature");
        }
        let request = json!({"messages":[{"role":"assistant","content":[thinking]}]});
        let original = request.clone();
        let budget = PayloadBudget::with_limit(BUDGET_BYTES);
        let observed = super::history::observe_request(&request, budget.clone())?;
        let baseline = budget.used();
        let before = fixture.store.stats()?;
        let called = Cell::new(false);
        let result = prepare_observed_request(
            &mut next,
            &observed,
            &SecurityRuntimeConfig::default(),
            &["chinese_phone_number"],
            &[],
            |_, _, _| {
                called.set(true);
                Ok(())
            },
        );
        assert!(
            matches!(result, Err(PlanError::ImmutableContent))
                && !called.get()
                && request == original
                && fixture.store.stats()? == before
                && budget.used() == baseline,
            "an3_immutable_reference_mutated_or_committed"
        );
        drop(observed);
        assert!(
            budget.used() == 0,
            "an3_immutable_reference_budget_retained"
        );
    }
    Ok(())
}

#[test]
fn an3_lane_native_id_ambiguity_and_pair_metadata_capacity_are_fail_closed() -> TestResult {
    let call = json!({"type":"tool_use","id":"toolu_synthetic_an3","name":"synthetic_tool","input":{"query":"public"}});
    let result = json!({"type":"tool_result","tool_use_id":"toolu_synthetic_an3","content":MARKER});
    let mut malformed = call.clone();
    malformed["input"] = json!("not-a-native-object");
    let mut empty = call.clone();
    empty["id"] = json!("");
    let mut empty_result = result.clone();
    empty_result["tool_use_id"] = json!("");
    let oversized = "x".repeat(513);
    let mut long = call.clone();
    long["id"] = json!(oversized);
    let mut long_result = result.clone();
    long_result["tool_use_id"] = long["id"].clone();
    let mut unknown = result.clone();
    unknown["tool_use_id"] = json!("toolu_unknown");
    let cases = [
        json!({"messages":[{"role":"assistant","content":[call,call]},{"role":"user","content":[result]}]}),
        json!({"messages":[{"role":"assistant","content":[call]},{"role":"user","content":[result,result]}]}),
        json!({"messages":[{"role":"assistant","content":[call]},{"role":"user","content":[unknown]}]}),
        json!({"messages":[{"role":"assistant","content":[malformed]},{"role":"user","content":[result]}]}),
        json!({"messages":[{"role":"assistant","content":[empty]},{"role":"user","content":[empty_result]}]}),
        json!({"messages":[{"role":"assistant","content":[long]},{"role":"user","content":[long_result]}]}),
        json!({"messages":[{"role":"assistant","content":[call]},{"role":"user","content":"Synthetic gap"},{"role":"user","content":[result]}]}),
    ];
    for request in cases {
        let budget = PayloadBudget::with_limit(BUDGET_BYTES);
        let observed = super::history::observe_request(&request, budget.clone())?;
        assert!(
            !observed
                .history()
                .iter()
                .any(|claim| matches!(claim, HistoryClaim::CallResultPair { .. })),
            "an3_ambiguous_native_id_paired"
        );
        drop(observed);
        assert!(budget.used() == 0, "an3_ambiguous_pair_budget_retained");
    }
    let mut other_call = call.clone();
    other_call["id"] = json!("toolu_synthetic_second");
    let mut other_result = result.clone();
    other_result["tool_use_id"] = other_call["id"].clone();
    let request = json!({"messages":[{"role":"assistant","content":[call,other_call]},{"role":"user","content":[other_result,result]}]});
    let budget = PayloadBudget::with_limit(BUDGET_BYTES);
    let observed = super::history::observe_request(&request, budget.clone())?;
    let pairs: Vec<_> = observed
        .history()
        .iter()
        .filter_map(|claim| match claim {
            HistoryClaim::CallResultPair { call, result } => Some((*call, *result)),
            _ => None,
        })
        .collect();
    assert!(
        pairs.len() == 2
            && std::ptr::eq(pairs[0].0, &request["messages"][0]["content"][1])
            && std::ptr::eq(pairs[1].0, &request["messages"][0]["content"][0]),
        "an3_pairing_used_array_position_not_native_id"
    );
    let measured = budget.used();
    drop(observed);
    assert!(
        budget.used() == 0 && measured > 0,
        "an3_pair_metadata_not_charged_or_refunded"
    );
    for limit in [0, 1, measured - 1, measured] {
        let budget = PayloadBudget::with_limit(limit);
        let result = super::history::observe_request(&request, budget.clone());
        assert!(
            result.is_ok() == (limit == measured),
            "an3_pair_metadata_exact_capacity_wrong"
        );
        drop(result);
        assert!(
            budget.used() == 0,
            "an3_pair_metadata_capacity_refund_missing"
        );
    }
    Ok(())
}

#[test]
fn an3_lane_sse_replay_waits_for_post_last_yield_continuation() -> TestResult {
    use crate::proxy::security::restoration_core::compose_anthropic_snapshots;
    use futures::StreamExt;
    let fixture = an2_fixture(true)?;
    let baseline = fixture.budget.used();
    let events = an2_sse_block_events(&fixture.reference, fixture.reference.len() / 2)?;
    let wire = an2_sse_wire(&events)?;
    let mut stream = an2_finalize_sse(&fixture, &wire)?.into_delivery_stream();
    let mut chunks = Vec::new();
    let mut frames = Vec::new();
    let (mut next, binding) = an3_next(
        &fixture,
        Some("synthetic-an2-session"),
        Protocol::Anthropic,
        "synthetic-an2-provider",
    )?;
    for _ in 0..events.len() {
        let chunk = futures::executor::block_on(stream.next()).ok_or("an3_sse_frame_missing")??;
        for data in std::str::from_utf8(&chunk)?
            .lines()
            .filter_map(|line| line.strip_prefix("data: "))
        {
            frames.push(serde_json::from_str::<Value>(data)?);
        }
        chunks.push(chunk);
        assert!(
            an2_counts(&fixture)? == (0, 1, 1),
            "an3_sse_published_during_yields"
        );
    }
    let references: Vec<&Value> = frames.iter().collect();
    let composed = compose_anthropic_snapshots(&references, &fixture.budget)
        .map_err(|_| "an3_sse_composition_rejected")?;
    assert!(
        super::history::import_delivered(&mut next, &binding, composed.as_value()).is_err()
            && next.resolve(&fixture.reference).is_err(),
        "an3_pending_sse_history_imported"
    );
    assert!(
        futures::executor::block_on(stream.next()).is_none(),
        "an3_sse_extra_yield"
    );
    super::history::import_delivered(&mut next, &binding, composed.as_value())?;
    assert!(
        next.resolve(&fixture.reference)? == "13900000000" && an2_counts(&fixture)? == (1, 0, 0),
        "an3_sse_completed_history_not_imported"
    );
    let (mut other, binding) = an3_next(
        &fixture,
        Some("synthetic-an2-session"),
        Protocol::Anthropic,
        "synthetic-an2-provider",
    )?;
    assert!(
        super::history::import_delivered(&mut other, &binding, &json!([composed.as_value()[0]]))
            .is_err()
            && other.resolve(&fixture.reference).is_err(),
        "an3_sse_truncated_call_history_imported"
    );
    drop(composed);
    drop(stream);
    assert!(
        fixture.budget.used() > baseline,
        "an3_live_sse_bytes_lost_budget_owner"
    );
    drop(chunks);
    assert!(
        fixture.budget.used() == baseline,
        "an3_sse_delivery_budget_not_refunded"
    );
    Ok(())
}

#[test]
fn an3_lane_eviction_removes_history_privilege_but_keeps_current_dlp() -> TestResult {
    use std::time::{Duration, Instant};
    let fixture = an2_fixture(true)?;
    let delivered = an3_deliver_json(&fixture, &an3_source(&fixture.reference))?;
    let store = fixture.store.clone();
    let reference = fixture.reference.clone();
    let revision = fixture.binding.policy_revision;
    let budget = fixture.budget.clone();
    drop(fixture);
    assert!(budget.used() == 0, "an3_eviction_fixture_budget_retained");
    store.expire_idle_at(Instant::now() + Duration::from_secs(7 * 24 * 60 * 60))?;
    assert!(store.stats()?.namespaces == 0, "an3_namespace_not_evicted");
    let mut next = store.admit_scoped_published_policy(
        ContinuityScope::Native {
            client: ClientProtocol::new(Protocol::Anthropic),
            ingress: IngressNamespace::Direct,
        },
        "synthetic-an2-app",
        Some("synthetic-an2-session"),
        revision,
    )?;
    let binding = next.bind_actual_attempt(
        "synthetic-an2-provider",
        "anthropic",
        "https://synthetic.invalid/v1/messages",
        Some("synthetic-an2-principal"),
    )?;
    assert!(
        super::history::import_delivered(&mut next, &binding, &delivered).is_err()
            && next.resolve(&reference).is_err(),
        "an3_evicted_history_authorized"
    );
    let request =
        json!({"messages":[{"role":"user","content":"Synthetic current input 13900000000"}]});
    let observed = super::history::observe_request(&request, budget.clone())?;
    let prepared = prepare_observed_request(
        &mut next,
        &observed,
        &SecurityRuntimeConfig::default(),
        &["chinese_phone_number"],
        &[],
        |_, _, _| Ok(()),
    )
    .map_err(|_| "an3_eviction_broke_current_dlp")?;
    assert!(
        prepared.reversible_occurrences == 1
            && !prepared.body.as_value()["messages"][0]["content"]
                .as_str()
                .ok_or("an3_current_input_missing")?
                .contains("13900000000"),
        "an3_current_dlp_not_applied_after_eviction"
    );
    drop(prepared);
    drop(observed);
    assert!(budget.used() == 0, "an3_eviction_current_budget_retained");
    Ok(())
}

fn an3_m1_labels() -> Vec<(String, bool)> {
    vec![
        ("synthetic_valid".to_owned(), true),
        ("x".repeat(256), true),
        ("é".repeat(128), true),
        (String::new(), false),
        ("x".repeat(257), false),
        ("é".repeat(129), false),
        ("synthetic label".to_owned(), false),
        ("synthetic\tlabel".to_owned(), false),
        ("synthetic\nlabel".to_owned(), false),
        ("synthetic\0label".to_owned(), false),
        ("synthetic\u{007f}label".to_owned(), false),
        ("synthetic\u{00a0}label".to_owned(), false),
        ("synthetic\u{2003}label".to_owned(), false),
    ]
}

#[test]
fn an3_lane_m1_native_ids_match_frozen_shared_label_contract() -> TestResult {
    for (id, valid) in an3_m1_labels() {
        let request = json!({"messages":[
            {"role":"assistant","content":[{"type":"tool_use","id":id,"name":"synthetic_tool","input":{"query":"public"}}]},
            {"role":"user","content":[{"type":"tool_result","tool_use_id":id,"content":MARKER}]}
        ]});
        let original = request.clone();
        let budget = PayloadBudget::with_limit(BUDGET_BYTES);
        let observed = super::history::observe_request(&request, budget.clone())?;
        let paired = observed
            .history()
            .iter()
            .filter(|claim| matches!(claim, HistoryClaim::CallResultPair { .. }))
            .count();
        assert!(
            paired == usize::from(valid),
            "an3_m1_native_id_contract_mismatch"
        );
        assert!(
            observed.sites().len() == 2 && request == original,
            "an3_m1_native_id_changed_text_observation"
        );
        assert_site(
            &observed,
            &request["messages"][1]["content"][0]["content"],
            SourceClass::ToolResult,
            EditConstraint::Editable,
        );
        drop(observed);
        assert!(budget.used() == 0, "an3_m1_native_id_budget_retained");
    }
    Ok(())
}

#[test]
fn an3_lane_m1_tool_names_match_frozen_shared_label_contract() -> TestResult {
    for (name, valid) in an3_m1_labels() {
        let request = json!({"messages":[
            {"role":"assistant","content":[{"type":"tool_use","id":"toolu_synthetic_m1","name":name,"input":{"query":"public"}}]},
            {"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_synthetic_m1","content":MARKER}]}
        ]});
        let original = request.clone();
        let budget = PayloadBudget::with_limit(BUDGET_BYTES);
        let observed = super::history::observe_request(&request, budget.clone())?;
        let paired = observed
            .history()
            .iter()
            .filter(|claim| matches!(claim, HistoryClaim::CallResultPair { .. }))
            .count();
        assert!(
            paired == usize::from(valid),
            "an3_m1_native_name_contract_mismatch"
        );
        assert!(
            observed.sites().len() == 2 && request == original,
            "an3_m1_native_name_changed_text_observation"
        );
        assert_site(
            &observed,
            &request["messages"][1]["content"][0]["content"],
            SourceClass::ToolResult,
            EditConstraint::Editable,
        );
        drop(observed);
        assert!(budget.used() == 0, "an3_m1_native_name_budget_retained");
    }
    Ok(())
}
