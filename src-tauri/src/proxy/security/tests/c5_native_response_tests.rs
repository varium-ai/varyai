//! Test-only native Chat selections exercise shared staging, not an adapter or publisher.
use std::{cell::Cell, rc::Rc};

use serde_json::{json, Value};

use super::{
    completed_context::ActualAttemptBinding,
    payload_budget::PayloadBudget,
    payload_buffer::test_support as buffer_events,
    payload_json::test_support as json_events,
    payload_metadata::{test_support as metadata_events, PayloadVec},
    response_restoration::RestoreError,
    restoration_contract::{
        ClientProtocol, ContinuityScope, EditConstraint, IngressNamespace, SourceClass, TextSites,
    },
    restoration_core::{restore_text, stage_native_json, NativeJsonObservation},
    tokenization::{DlpRequestLease, DlpTokenStore, TokenCategory, TokenFragment},
};
use crate::proxy::protocol_bridge::Protocol;

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;
const CONTACT: &str = "13900000000";
const UNKNOWN: &str = "rredact-cn_phone-00000000000000000000-ai";
const BUDGET_BYTES: usize = 4 * 1024 * 1024;

fn require<T>(result: Result<T, RestoreError>) -> TestResult<T> {
    result.map_err(|_| "synthetic_native_staging_rejected".into())
}

fn native_fixture() -> TestResult<(DlpTokenStore, DlpRequestLease, ActualAttemptBinding, String)> {
    let store = DlpTokenStore::new();
    let mut lease = store.admit_scoped_published_policy(
        ContinuityScope::Native {
            client: ClientProtocol::new(Protocol::Chat),
            ingress: IngressNamespace::Direct,
        },
        "synthetic-c5-response-app",
        Some("synthetic-c5-response-session"),
        1,
    )?;
    let reference = lease.prepare(
        &[TokenFragment {
            raw: CONTACT,
            whole_span_categories: &[TokenCategory::Phone],
        }],
        &[],
        |tokens| Ok(tokens[0].clone()),
    )?;
    let binding = lease.bind_actual_attempt(
        "synthetic-c5-provider",
        "chat",
        "https://synthetic.invalid/v1/chat/completions",
        Some("synthetic-c5-principal"),
    )?;
    assert!(
        lease.resolve_if_authorized(UNKNOWN)?.is_none(),
        "synthetic_unknown_reference_collision"
    );
    Ok((store, lease, binding, reference))
}

// Selection paths intentionally live only in this fixture, never in production core.
fn chat_body(reference: &str) -> Value {
    json!({
        "id":"synthetic-chat-response", "object":"chat.completion",
        "created":7, "model":"synthetic-model",
        "choices":[
            {"index":0, "message":{
                "role":"assistant", "content":format!("前🔒{reference} / {reference}尾"),
                "tool_calls":[{"id":"synthetic-call", "type":"function", "function":{
                    "name":"synthetic-tool", "arguments":format!("{{\"contact\":\"{reference}\",\"n\":1.0}}"),
                    "unknown_argument_sibling":reference}}],
                "unknown_message_sibling":reference}, "finish_reason":"tool_calls"},
            {"index":1, "message":{"role":"assistant", "content":reference},
                "finish_reason":"stop"}
        ],
        "usage":{"prompt_tokens":1,"completion_tokens":2,"total_tokens":3},
        "unknown":{"same_value":reference,"types":[true,null,7,1.0,{"k":"opaque"}]}
    })
}

fn chat_observation<'a>(
    body: &'a Value,
    budget: &PayloadBudget,
    constraint: EditConstraint,
) -> TestResult<NativeJsonObservation<'a>> {
    let mut sites = TextSites::new(budget.clone())?;
    sites.observe(
        &body["choices"][0]["message"]["content"],
        SourceClass::HistoricalText,
        constraint,
    )?;
    sites.observe(
        &body["choices"][0]["message"]["tool_calls"][0]["function"]["arguments"],
        SourceClass::ToolArguments,
        EditConstraint::Editable,
    )?;
    require(NativeJsonObservation::new(
        body,
        ClientProtocol::new(Protocol::Chat),
        sites,
        &[
            &body["choices"][0]["message"],
            &body["choices"][0]["message"]["tool_calls"][0],
        ],
        budget.clone(),
    ))
}

#[test]
fn c5_native_current_binding_accepts_admitted_published_policy_without_history_gate() -> TestResult
{
    let (_store, lease, binding, _reference) = native_fixture()?;
    assert!(
        lease
            .require_native_attempt_binding(ClientProtocol::new(Protocol::Chat), &binding)
            .is_ok(),
        "current_request_binding_rejected"
    );
    Ok(())
}

#[test]
fn c5_native_current_binding_allows_request_local_identity_and_principal_absence() -> TestResult {
    let store = DlpTokenStore::new();
    let lease = store.admit_scoped_published_policy(
        ContinuityScope::Native {
            client: ClientProtocol::new(Protocol::Chat),
            ingress: IngressNamespace::Direct,
        },
        "synthetic-local-binding-app",
        None,
        1,
    )?;
    let binding = lease.bind_actual_attempt(
        "synthetic-provider",
        "chat",
        "https://synthetic.invalid/v1/chat/completions",
        None,
    )?;
    assert!(
        !lease.history_binding_live(&binding),
        "optional_history_unexpectedly_available"
    );
    assert!(
        lease
            .require_native_attempt_binding(ClientProtocol::new(Protocol::Chat), &binding)
            .is_ok(),
        "request_local_binding_rejected"
    );
    Ok(())
}

#[test]
fn c5_native_current_binding_rejects_wrong_store_namespace_generation_policy_and_client(
) -> TestResult {
    let (_store, lease, binding, _reference) = native_fixture()?;
    for altered in [
        ActualAttemptBinding {
            store_id: binding.store_id.wrapping_add(1),
            ..binding.clone()
        },
        ActualAttemptBinding {
            namespace: binding.namespace.wrapping_add(1),
            ..binding.clone()
        },
        ActualAttemptBinding {
            generation: [0x7f; 16],
            ..binding.clone()
        },
        ActualAttemptBinding {
            policy_revision: binding.policy_revision.wrapping_add(1),
            ..binding.clone()
        },
    ] {
        assert!(
            lease
                .require_native_attempt_binding(ClientProtocol::new(Protocol::Chat), &altered)
                .is_err(),
            "foreign_binding_accepted"
        );
    }
    for protocol in [Protocol::Anthropic, Protocol::Gemini, Protocol::Responses] {
        assert!(
            lease
                .require_native_attempt_binding(ClientProtocol::new(protocol), &binding)
                .is_err(),
            "foreign_client_binding_accepted"
        );
    }
    Ok(())
}

#[test]
fn c3_responses_current_binding_accepts_admitted_revision_without_optional_history() -> TestResult {
    let store = DlpTokenStore::new();
    let lease = store.admit_published_policy("synthetic-responses-current", None, 1)?;
    let binding = lease.bind_actual_attempt(
        "synthetic-provider",
        "responses",
        "https://synthetic.invalid/v1/responses",
        None,
    )?;
    assert!(
        !lease.history_binding_live(&binding),
        "responses_optional_history_unexpected"
    );
    assert!(
        lease
            .require_current_attempt_binding(ClientProtocol::new(Protocol::Responses), &binding)
            .is_ok(),
        "responses_current_binding_rejected"
    );
    Ok(())
}

#[test]
fn c3_responses_current_binding_keeps_native_scope_independent() -> TestResult {
    let (_store, lease, binding, _reference) = native_fixture()?;
    assert!(
        lease
            .require_current_attempt_binding(ClientProtocol::new(Protocol::Chat), &binding)
            .is_ok(),
        "shared_native_current_binding_rejected"
    );
    assert!(
        lease
            .require_current_attempt_binding(ClientProtocol::new(Protocol::Responses), &binding)
            .is_err(),
        "native_scope_became_responses"
    );
    assert!(
        lease
            .require_native_attempt_binding(ClientProtocol::new(Protocol::Responses), &binding)
            .is_err(),
        "native_validator_widened_to_responses"
    );
    Ok(())
}

#[test]
fn c3_responses_current_binding_rejects_foreign_identity_and_native_clients() -> TestResult {
    let store = DlpTokenStore::new();
    let lease = store.admit_published_policy(
        "synthetic-responses-current-negative",
        Some("synthetic-session"),
        1,
    )?;
    let binding = lease.bind_actual_attempt(
        "synthetic-provider",
        "responses",
        "https://synthetic.invalid/v1/responses",
        Some("synthetic-principal"),
    )?;
    for altered in [
        ActualAttemptBinding {
            store_id: binding.store_id.wrapping_add(1),
            ..binding.clone()
        },
        ActualAttemptBinding {
            namespace: binding.namespace.wrapping_add(1),
            ..binding.clone()
        },
        ActualAttemptBinding {
            generation: [0x7f; 16],
            ..binding.clone()
        },
        ActualAttemptBinding {
            policy_revision: binding.policy_revision.wrapping_add(1),
            ..binding.clone()
        },
    ] {
        assert!(
            lease
                .require_current_attempt_binding(ClientProtocol::new(Protocol::Responses), &altered)
                .is_err(),
            "responses_foreign_current_binding_accepted"
        );
    }
    for protocol in [Protocol::Chat, Protocol::Anthropic, Protocol::Gemini] {
        assert!(
            lease
                .require_current_attempt_binding(ClientProtocol::new(protocol), &binding)
                .is_err(),
            "responses_scope_became_native"
        );
    }
    Ok(())
}

#[test]
fn c5_native_response_observation_retains_exact_borrowed_owner_and_charged_order() -> TestResult {
    let (store, _lease, _binding, reference) = native_fixture()?;
    let body = chat_body(&reference);
    let budget = PayloadBudget::with_limit(BUDGET_BYTES);
    let before = store.stats()?;
    let observed = chat_observation(&body, &budget, EditConstraint::Editable)?;
    assert!(std::ptr::eq(observed.source(), &body), "source_owner_lost");
    assert!(
        observed.client().protocol() == Protocol::Chat,
        "client_lost"
    );
    let site = observed.sites().site(0).ok_or("synthetic_site_missing")?;
    assert!(
        std::ptr::eq(site.native(), &body["choices"][0]["message"]["content"])
            && site.constraint() == EditConstraint::Editable,
        "site_identity_or_constraint_lost"
    );
    assert!(
        observed.delivered_nodes().len() == 2
            && std::ptr::eq(
                observed.delivered_nodes()[0],
                &body["choices"][0]["message"]
            )
            && std::ptr::eq(
                observed.delivered_nodes()[1],
                &body["choices"][0]["message"]["tool_calls"][0]
            ),
        "snapshot_identity_or_order_lost"
    );
    assert!(
        budget.used() > 0 && observed.budget().used() == budget.used(),
        "observation_metadata_not_charged"
    );
    assert!(store.stats()? == before, "observation_published");
    drop(observed);
    assert!(budget.used() == 0, "observation_charge_retained");
    Ok(())
}

#[test]
fn c5_native_response_chat_stages_only_selected_content_and_arguments_atomically() -> TestResult {
    let (store, lease, binding, reference) = native_fixture()?;
    let body = chat_body(&reference);
    let original = body.clone();
    let budget = PayloadBudget::with_limit(BUDGET_BYTES);
    let observed = chat_observation(&body, &budget, EditConstraint::Editable)?;
    let baseline = budget.used();
    let before = store.stats()?;
    let staged = require(stage_native_json(&observed, &lease, &binding))?;
    let mut expected = body.clone();
    expected["choices"][0]["message"]["content"] = json!(format!("前🔒{CONTACT} / {CONTACT}尾"));
    expected["choices"][0]["message"]["tool_calls"][0]["function"]["arguments"] =
        json!(format!("{{\"contact\":\"{CONTACT}\",\"n\":1.0}}"));
    assert!(
        staged.as_value() == &expected,
        "native_candidate_changed_unselected_data"
    );
    assert!(
        staged.changed() && staged.restored_occurrences() == 3,
        "restoration_ledger_lost"
    );
    assert!(
        staged.output_tokens().len() == 1 && staged.output_tokens()[0].as_str() == reference,
        "dependencies_not_unique"
    );
    assert!(
        staged.delivered().is_array()
            && staged
                .delivered()
                .as_array()
                .is_some_and(|items| items.len() == 2)
            && staged.delivered()[0] == expected["choices"][0]["message"]
            && staged.delivered()[1] == expected["choices"][0]["message"]["tool_calls"][0],
        "delivered_snapshots_not_restored_or_ordered"
    );
    assert!(
        std::ptr::eq(staged.source(), &body)
            && staged.client().protocol() == Protocol::Chat
            && staged.binding().store_id == binding.store_id
            && staged.binding().namespace == binding.namespace
            && staged.binding().generation == binding.generation
            && staged.binding().provider_fingerprint == binding.provider_fingerprint,
        "staged_provenance_changed"
    );
    assert!(
        budget.used() > baseline && staged.budget().used() == budget.used(),
        "candidate_or_snapshot_not_charged"
    );
    assert!(
        body == original && store.stats()? == before,
        "staging_mutated_source_or_published"
    );
    // The staged owner must survive the observation without unguarded extraction.
    drop(observed);
    assert!(
        staged.as_value() == &expected,
        "staging_borrowed_observation_metadata"
    );
    drop(staged);
    assert!(
        budget.used() == 0 && store.stats()? == before,
        "staged_drop_not_atomic"
    );
    Ok(())
}

#[test]
fn c5_native_response_immutable_required_restore_rejects_without_publication() -> TestResult {
    let (store, lease, binding, reference) = native_fixture()?;
    let body = chat_body(&reference);
    let original = body.clone();
    let budget = PayloadBudget::with_limit(BUDGET_BYTES);
    let observed = chat_observation(&body, &budget, EditConstraint::Immutable)?;
    let baseline = budget.used();
    let before = store.stats()?;
    assert!(
        stage_native_json(&observed, &lease, &binding).is_err(),
        "immutable_restore_escaped"
    );
    assert!(
        body == original && budget.used() == baseline && store.stats()? == before,
        "immutable_failure_not_atomic"
    );
    Ok(())
}

#[test]
fn c5_native_response_unknown_immutable_literals_remain_unchanged() -> TestResult {
    let (store, lease, binding, _reference) = native_fixture()?;
    let body = chat_body(UNKNOWN);
    let budget = PayloadBudget::with_limit(BUDGET_BYTES);
    let observed = chat_observation(&body, &budget, EditConstraint::Immutable)?;
    let baseline = budget.used();
    let before = store.stats()?;
    let staged = require(stage_native_json(&observed, &lease, &binding))?;
    assert!(
        staged.as_value() == &body
            && !staged.changed()
            && staged.restored_occurrences() == 0
            && staged.output_tokens().is_empty(),
        "unknown_literal_gained_authority"
    );
    assert!(
        staged.delivered()[0] == body["choices"][0]["message"],
        "unknown_snapshot_changed"
    );
    drop(staged);
    assert!(
        budget.used() == baseline && store.stats()? == before,
        "literal_staging_published"
    );
    Ok(())
}

#[test]
fn c5_native_response_foreign_equal_site_is_rejected() -> TestResult {
    let (_store, _lease, _binding, reference) = native_fixture()?;
    let body = chat_body(&reference);
    let foreign = body.clone();
    let budget = PayloadBudget::with_limit(BUDGET_BYTES);
    let mut sites = TextSites::new(budget.clone())?;
    sites.observe(
        &foreign["choices"][0]["message"]["content"],
        SourceClass::HistoricalText,
        EditConstraint::Editable,
    )?;
    let result = NativeJsonObservation::new(
        &body,
        ClientProtocol::new(Protocol::Chat),
        sites,
        &[&body["choices"][0]["message"]],
        budget.clone(),
    );
    assert!(result.is_err(), "foreign_equal_site_accepted");
    drop(result);
    assert!(budget.used() == 0, "foreign_site_metadata_retained");
    Ok(())
}

#[test]
fn c5_native_response_foreign_equal_delivered_snapshot_is_rejected() -> TestResult {
    let (_store, _lease, _binding, reference) = native_fixture()?;
    let body = chat_body(&reference);
    let foreign = body.clone();
    let budget = PayloadBudget::with_limit(BUDGET_BYTES);
    let result = NativeJsonObservation::new(
        &body,
        ClientProtocol::new(Protocol::Chat),
        TextSites::new(budget.clone())?,
        &[&foreign["choices"][0]["message"]],
        budget.clone(),
    );
    assert!(result.is_err(), "foreign_equal_snapshot_accepted");
    drop(result);
    assert!(budget.used() == 0, "foreign_snapshot_metadata_retained");
    Ok(())
}

#[test]
fn c5_native_response_snapshot_count_is_nonempty_and_bounded() -> TestResult {
    let body = json!({"items": (0..129).map(|i| json!({"synthetic":i})).collect::<Vec<_>>()});
    let items = body["items"].as_array().ok_or("synthetic_items_missing")?;
    for count in [0, 129] {
        let budget = PayloadBudget::with_limit(BUDGET_BYTES);
        let nodes: Vec<&Value> = items.iter().take(count).collect();
        let result = NativeJsonObservation::new(
            &body,
            ClientProtocol::new(Protocol::Chat),
            TextSites::new(budget.clone())?,
            &nodes,
            budget.clone(),
        );
        assert!(result.is_err(), "invalid_snapshot_count_accepted");
        drop(result);
        assert!(budget.used() == 0, "invalid_count_metadata_retained");
    }
    let budget = PayloadBudget::with_limit(BUDGET_BYTES);
    let nodes: Vec<&Value> = items.iter().take(128).collect();
    let observed = require(NativeJsonObservation::new(
        &body,
        ClientProtocol::new(Protocol::Chat),
        TextSites::new(budget.clone())?,
        &nodes,
        budget.clone(),
    ))?;
    assert!(
        observed.delivered_nodes().len() == 128,
        "valid_snapshot_boundary_rejected"
    );
    drop(observed);
    assert!(budget.used() == 0, "boundary_metadata_retained");
    Ok(())
}

#[test]
fn c5_native_response_observation_snapshot_metadata_failure_refunds() -> TestResult {
    let body = json!({"message":{"role":"assistant","content":"synthetic"}});
    let budget = PayloadBudget::with_limit(BUDGET_BYTES);
    let denied = Rc::new(Cell::new(false));
    let seen = Rc::clone(&denied);
    let hook = metadata_events::install(move |event| {
        if event.kind == metadata_events::Kind::Vector
            && event.phase == metadata_events::Phase::BeforeAllocation
        {
            seen.set(true);
            assert!(
                event.bytes > 0 && event.used >= event.bytes,
                "snapshot_metadata_not_precharged"
            );
            metadata_events::Action::Fail
        } else {
            metadata_events::Action::Continue
        }
    });
    let result = NativeJsonObservation::new(
        &body,
        ClientProtocol::new(Protocol::Chat),
        TextSites::new(budget.clone())?,
        &[&body["message"]],
        budget.clone(),
    );
    drop(hook);
    assert!(denied.get(), "snapshot_metadata_allocation_not_reached");
    assert!(result.is_err(), "snapshot_metadata_failure_ignored");
    drop(result);
    assert!(
        budget.used() == 0,
        "snapshot_metadata_failure_retained_charge"
    );
    Ok(())
}

#[test]
fn c5_native_response_stage_capacity_failure_leaves_observation_and_store_unchanged() -> TestResult
{
    let (store, lease, binding, reference) = native_fixture()?;
    let body = chat_body(&reference);
    let original = body.clone();
    let budget = PayloadBudget::with_limit(BUDGET_BYTES);
    let observed = chat_observation(&body, &budget, EditConstraint::Editable)?;
    let held = budget.reserve(budget.limit() - budget.used())?;
    let baseline = budget.used();
    let before = store.stats()?;
    assert!(
        stage_native_json(&observed, &lease, &binding).is_err(),
        "capacity_denial_ignored"
    );
    assert!(
        body == original && budget.used() == baseline && store.stats()? == before,
        "stage_capacity_failure_not_atomic"
    );
    drop(held);
    let staged = require(stage_native_json(&observed, &lease, &binding))?;
    assert!(
        staged.restored_occurrences() == 3,
        "failed_stage_committed_occurrence_ledger"
    );
    drop(staged);
    assert!(store.stats()? == before, "capacity_retry_published");
    Ok(())
}

#[test]
fn c5_native_response_candidate_and_snapshot_build_failures_roll_back_ledger() -> TestResult {
    for fail_at in [1, 2] {
        let (store, lease, binding, reference) = native_fixture()?;
        let body = chat_body(&reference);
        let original = body.clone();
        let budget = PayloadBudget::with_limit(BUDGET_BYTES);
        let observed = chat_observation(&body, &budget, EditConstraint::Editable)?;
        let baseline = budget.used();
        let before = store.stats()?;
        let builds = Rc::new(Cell::new(0));
        let seen = Rc::clone(&builds);
        let hook = json_events::install(move |event| {
            if event.phase == json_events::Phase::BeforeBuild {
                seen.set(seen.get() + 1);
                assert!(
                    event.charged > 0 && event.used >= event.charged,
                    "json_build_not_precharged"
                );
                if seen.get() == fail_at {
                    return json_events::Action::Fail;
                }
            }
            json_events::Action::Continue
        });
        let result = stage_native_json(&observed, &lease, &binding);
        drop(hook);
        assert!(
            builds.get() >= fail_at,
            "candidate_or_snapshot_build_not_reached"
        );
        assert!(result.is_err(), "native_json_build_failure_ignored");
        drop(result);
        assert!(
            body == original && budget.used() == baseline && store.stats()? == before,
            "native_build_failure_not_atomic"
        );
        let staged = require(stage_native_json(&observed, &lease, &binding))?;
        assert!(
            staged.output_tokens().len() == 1 && staged.restored_occurrences() == 3,
            "failed_build_committed_dependency_ledger"
        );
        drop(staged);
        assert!(
            budget.used() == baseline && store.stats()? == before,
            "retry_charge_or_publication_leak"
        );
    }
    Ok(())
}

#[test]
fn c5_native_response_snapshot_encoding_failure_is_charged_and_atomic() -> TestResult {
    let (store, lease, binding, reference) = native_fixture()?;
    let body = chat_body(&reference);
    let original = body.clone();
    let budget = PayloadBudget::with_limit(BUDGET_BYTES);
    let observed = chat_observation(&body, &budget, EditConstraint::Editable)?;
    let baseline = budget.used();
    let before = store.stats()?;
    let candidate_ready = Rc::new(Cell::new(false));
    let seen = Rc::clone(&candidate_ready);
    let json_hook = json_events::install(move |event| {
        if event.phase == json_events::Phase::AfterBuild {
            seen.set(true);
        }
        json_events::Action::Continue
    });
    let denied = Rc::new(Cell::new(false));
    let seen = Rc::clone(&denied);
    let buffer_hook = buffer_events::install(move |event| {
        if candidate_ready.get() && event.phase == buffer_events::Phase::BeforeAllocation {
            seen.set(true);
            assert!(
                event.requested > 0 && event.used > baseline,
                "snapshot_encoding_not_precharged"
            );
            buffer_events::Action::FailAllocation
        } else {
            buffer_events::Action::Continue
        }
    });
    let result = stage_native_json(&observed, &lease, &binding);
    drop(buffer_hook);
    drop(json_hook);
    assert!(denied.get(), "budgeted_snapshot_encoding_not_reached");
    assert!(result.is_err(), "snapshot_encoding_failure_ignored");
    drop(result);
    assert!(
        body == original && budget.used() == baseline && store.stats()? == before,
        "snapshot_encoding_failure_not_atomic"
    );
    let staged = require(stage_native_json(&observed, &lease, &binding))?;
    assert!(
        staged.restored_occurrences() == 3,
        "encoding_failure_committed_occurrences"
    );
    drop(staged);
    assert!(
        budget.used() == baseline && store.stats()? == before,
        "encoding_retry_leaked"
    );
    Ok(())
}

#[test]
fn c5_native_response_rejects_foreign_binding_and_wrong_client_scope() -> TestResult {
    let (store, lease, binding, reference) = native_fixture()?;
    let (_other_store, _other_lease, foreign_binding, _other_reference) = native_fixture()?;
    let body = chat_body(&reference);
    let budget = PayloadBudget::with_limit(BUDGET_BYTES);
    let observed = chat_observation(&body, &budget, EditConstraint::Editable)?;
    let baseline = budget.used();
    let before = store.stats()?;
    assert!(
        stage_native_json(&observed, &lease, &foreign_binding).is_err(),
        "foreign_binding_accepted"
    );
    assert!(
        budget.used() == baseline && store.stats()? == before,
        "binding_rejection_not_atomic"
    );
    drop(observed);
    let sites = TextSites::new(budget.clone())?;
    let wrong_client = require(NativeJsonObservation::new(
        &body,
        ClientProtocol::new(Protocol::Gemini),
        sites,
        &[&body["choices"][0]["message"]],
        budget.clone(),
    ))?;
    let baseline = budget.used();
    assert!(
        stage_native_json(&wrong_client, &lease, &binding).is_err(),
        "wrong_native_client_accepted"
    );
    assert!(
        budget.used() == baseline && store.stats()? == before,
        "scope_rejection_not_atomic"
    );
    Ok(())
}

#[test]
fn c5_native_response_shared_matcher_characterizes_exact_literals_and_unique_dependencies(
) -> TestResult {
    let (store, lease, _binding, reference) = native_fixture()?;
    let budget = PayloadBudget::with_limit(BUDGET_BYTES);
    let before = store.stats()?;
    let mut tokens = PayloadVec::new(budget.clone())?;
    let mut occurrences = 0;
    let source = format!("前🔒rredact-custom-bad{reference} / {UNKNOWN} / {reference}尾");
    let restored = require(restore_text(
        &source,
        &lease,
        &budget,
        &mut tokens,
        &mut occurrences,
    ))?;
    assert!(
        restored.as_str() == format!("前🔒rredact-custom-bad{CONTACT} / {UNKNOWN} / {CONTACT}尾"),
        "shared_matcher_changed_literal_or_utf8"
    );
    assert!(
        occurrences == 2 && tokens.len() == 1 && tokens.as_slice()[0].as_str() == reference,
        "shared_matcher_dependency_semantics_changed"
    );
    drop(restored);
    drop(tokens);
    assert!(
        budget.used() == 0 && store.stats()? == before,
        "shared_matcher_published_or_leaked"
    );
    Ok(())
}
