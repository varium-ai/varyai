//! Synthetic-only history proof foundation tests. No request exemptions are installed.
use super::{
    completed_context::ActualAttemptBinding,
    history_provenance::{
        DlpHistoryLeafField as Field, DlpHistorySeparator as Sep, DlpHistoryTarget as Target,
    },
    tokenization::{DlpRequestLease, DlpTokenStore, TokenError},
};
use serde_json::{json, Value};
use std::time::{Duration, Instant};

fn admitted(store: &DlpTokenStore) -> Result<(DlpRequestLease, ActualAttemptBinding), TokenError> {
    let lease = store.admit("codex", Some("synthetic-history"), 1)?;
    let binding = lease.bind_actual_attempt_extracted(
        "synthetic-provider",
        "responses",
        "https://synthetic.invalid/v1/responses",
        Some("synthetic-principal"),
    )?;
    Ok((lease, binding))
}

fn delivered() -> Value {
    json!({"id":"synthetic-response","status":"completed","output":[
        {"id":"synthetic-message","type":"message","role":"assistant","status":"completed","content":[
            {"type":"output_text","text":"synthetic output","annotations":[]}]},
        {"id":"synthetic-reasoning","type":"reasoning","summary":[
            {"type":"summary_text","text":"synthetic first"},
            {"type":"summary_text","text":"synthetic second"}],
            "content":[{"type":"reasoning_text","text":"synthetic detailed"}]},
        {"id":"synthetic-call-item","type":"function_call","call_id":"synthetic-call","name":"synthetic_tool","arguments":"{}","reasoning_content":"synthetic call reasoning"}
    ]})
}

fn replay(response: &Value) -> Value {
    let mut input = vec![json!({"type":"message","role":"user","content":"synthetic prefix"})];
    if let Some(output) = response["output"].as_array() {
        input.extend(output.iter().cloned());
    }
    input.push(json!({"type":"message","role":"user","content":"synthetic suffix"}));
    json!({"input":input})
}

fn publish(lease: &DlpRequestLease, binding: &ActualAttemptBinding, response: &Value) -> bool {
    lease
        .capture_delivered_history(binding, response)
        .is_some_and(|pending| lease.publish_delivered_history(binding, pending))
}

#[test]
fn history_provenance_exact_typed_items_survive_top_level_offsets() -> Result<(), TokenError> {
    let store = DlpTokenStore::new();
    let (lease, binding) = admitted(&store)?;
    let response = delivered();
    let request = replay(&response);
    assert!(lease.history_projection(&binding, &request).is_none());
    assert!(publish(&lease, &binding, &response));
    let projection = lease
        .history_projection(&binding, &request)
        .ok_or(TokenError::Unavailable)?;
    for (index, field, part) in [
        (1, Field::OutputText, 0),
        (2, Field::ReasoningSummary, 0),
        (2, Field::ReasoningSummary, 1),
        (2, Field::ReasoningContent, 0),
        (3, Field::CallReasoning, 0),
    ] {
        assert!(projection
            .source_for_responses_leaf(index, field, part)
            .is_some());
    }
    assert!(projection
        .source_for_responses_leaf(0, Field::OutputText, 0)
        .is_none());
    assert!(projection
        .source_for_responses_leaf(3, Field::OutputText, 0)
        .is_none());
    assert!(projection
        .source_for_responses_leaf(2, Field::ReasoningContent, 1)
        .is_none());
    Ok(())
}

#[test]
fn history_provenance_never_matches_id_role_or_bytes_without_exact_item() -> Result<(), TokenError>
{
    let store = DlpTokenStore::new();
    let (lease, binding) = admitted(&store)?;
    let response = delivered();
    assert!(publish(&lease, &binding, &response));
    for change in 0..8 {
        let mut request = replay(&response);
        match change {
            0 => request["input"][2]["summary"][0]["text"] = json!("synthetic changed"),
            1 => request["input"][2]["id"] = json!("synthetic-other"),
            2 => request["input"][2]["summary"][0]["type"] = json!("reasoning_text"),
            3 => request["input"][2]["summary"]
                .as_array_mut()
                .map(|parts| parts.reverse())
                .ok_or(TokenError::Unavailable)?,
            4 => request["input"][2]["metadata"] = json!({"synthetic":true}),
            5 => request["input"][2]["summary"][0]["text"] = json!(false),
            6 => request["input"][2]["content"][0]["text"] = json!("synthetic first"),
            _ => request["input"][2]["type"] = json!("unknown"),
        }
        let projection = lease
            .history_projection(&binding, &request)
            .ok_or(TokenError::Unavailable)?;
        assert!(projection
            .source_for_responses_leaf(2, Field::ReasoningSummary, 0)
            .is_none());
    }
    let request = json!({"input":[{"type":"message","role":"user","content":[{"type":"output_text","text":"synthetic output"}]}]});
    assert!(lease.history_projection(&binding, &request).is_none());
    Ok(())
}

#[test]
fn history_provenance_unrelated_live_record_does_not_classify_independent_image_history(
) -> Result<(), TokenError> {
    let store = DlpTokenStore::new();
    let (lease, binding) = admitted(&store)?;
    assert!(publish(&lease, &binding, &delivered()));
    let request = json!({"input":[
        {"type":"message","role":"user","content":[
            {"type":"input_text","text":"synthetic unrelated question"},
            {"type":"input_image","image_url":"data:image/png;base64,synthetic-opaque","detail":"auto"}]},
        {"type":"function_call","call_id":"synthetic-independent-call","name":"synthetic_tool","arguments":"{}"},
        {"type":"function_call_output","call_id":"synthetic-independent-call","output":"synthetic independent result"}
    ]});
    let candidate = lease.history_admission_candidate_projection(&request);
    assert!(
        candidate.is_none(),
        "an unrelated live record must not create a request candidate"
    );
    assert!(lease.history_projection(&binding, &request).is_none());
    assert!(
        super::request_preparation::is_fresh_readable_history_with_history(
            &request,
            &super::payload_budget::PayloadBudget::default(),
            candidate.as_ref(),
        )?
    );
    Ok(())
}

#[test]
fn history_provenance_exact_call_without_reasoning_still_requires_strict_resume(
) -> Result<(), TokenError> {
    let store = DlpTokenStore::new();
    let (lease, binding) = admitted(&store)?;
    for call in [
        json!({"type":"function_call","call_id":"synthetic-exact-call","name":"synthetic_tool","arguments":"{}"}),
        json!({"type":"custom_tool_call","call_id":"synthetic-exact-call","name":"synthetic_tool","input":"synthetic input"}),
    ] {
        assert!(publish(&lease, &binding, &json!({"output":[call.clone()]})));
        let request = json!({"input":[call.clone(),{
            "type":if call["type"] == "function_call" {"function_call_output"} else {"custom_tool_call_output"},
            "call_id":"synthetic-exact-call","output":"synthetic exact result"
        }]});
        let candidate = lease
            .history_admission_candidate_projection(&request)
            .ok_or(TokenError::Unavailable)?;
        assert!(candidate
            .source_for_responses_leaf(0, Field::CallReasoning, 0)
            .is_none());
        assert!(
            !super::request_preparation::is_fresh_readable_history_with_history(
                &request,
                &super::payload_budget::PayloadBudget::default(),
                Some(&candidate),
            )?
        );
    }
    Ok(())
}

#[test]
fn history_provenance_requires_full_live_binding_without_operation_transfer(
) -> Result<(), TokenError> {
    let store = DlpTokenStore::new();
    let (lease, binding) = admitted(&store)?;
    let response = delivered();
    assert!(publish(&lease, &binding, &response));
    let request = replay(&response);
    for change in 0..6 {
        let mut changed = binding.clone();
        match change {
            0 => changed.store_id ^= 1,
            1 => changed.namespace += 1,
            2 => changed.generation[0] ^= 1,
            3 => changed.policy_revision += 1,
            4 => changed.provider_fingerprint[0] ^= 1,
            _ => changed.reusable_principal = false,
        }
        assert!(lease.history_projection(&changed, &request).is_none());
    }
    for (provider, protocol, url, principal) in [
        (
            "other",
            "responses",
            "https://synthetic.invalid/v1/responses",
            Some("synthetic-principal"),
        ),
        (
            "synthetic-provider",
            "chat",
            "https://synthetic.invalid/v1/responses",
            Some("synthetic-principal"),
        ),
        (
            "synthetic-provider",
            "responses",
            "https://synthetic.invalid/v1/responses/compact",
            Some("synthetic-principal"),
        ),
        (
            "synthetic-provider",
            "responses",
            "https://synthetic.invalid/v1/responses",
            Some("synthetic-other"),
        ),
        (
            "synthetic-provider",
            "responses",
            "https://synthetic.invalid/v1/responses",
            None,
        ),
    ] {
        let changed = lease.bind_actual_attempt_extracted(provider, protocol, url, principal)?;
        assert!(lease.history_projection(&changed, &request).is_none());
    }
    let other_store = DlpTokenStore::new();
    let (other_lease, _) = admitted(&other_store)?;
    assert!(other_lease.history_projection(&binding, &request).is_none());
    let changed_policy = store.admit("codex", Some("synthetic-history"), 2)?;
    assert!(changed_policy
        .history_projection(&binding, &request)
        .is_none());
    let pending = lease
        .capture_delivered_history(&binding, &response)
        .ok_or(TokenError::Unavailable)?;
    let stale = binding.clone();
    drop(lease);
    store.expire_idle_at(Instant::now() + Duration::from_secs(7200))?;
    let (new_lease, _) = admitted(&store)?;
    assert!(!new_lease.publish_delivered_history(&stale, pending));
    assert!(new_lease.history_projection(&stale, &request).is_none());
    Ok(())
}

#[test]
fn history_provenance_projection_count_boundary_keeps_exact_match_and_releases_pins(
) -> Result<(), TokenError> {
    let store = DlpTokenStore::new();
    let (lease, binding) = admitted(&store)?;
    let response = json!({"output":[{"type":"reasoning","summary":[
        {"type":"summary_text","text":"s".repeat(220 * 1024)}]}]});
    assert!(publish(&lease, &binding, &response));
    let baseline = store.history_bytes_for_test()?;
    let mut items = vec![Value::Null; 4096];
    items[4095] = response["output"][0].clone();
    let mut request = json!({"input":items});
    for _ in 0..3 {
        let projection = lease
            .history_projection(&binding, &request)
            .ok_or(TokenError::Unavailable)?;
        assert!(
            projection
                .source_for_responses_leaf(4095, Field::ReasoningSummary, 0)
                .is_some(),
            "last permitted item must retain full exact matching near the response ceiling"
        );
        drop(projection);
        assert!(
            store.history_bytes_for_test()? == baseline,
            "bounded borrowed projection must release all pins and permits"
        );
    }
    request["input"]
        .as_array_mut()
        .ok_or(TokenError::Unavailable)?
        .push(Value::Null);
    assert!(
        lease.history_projection(&binding, &request).is_none(),
        "oversized input must not acquire optional proof"
    );
    assert!(lease
        .history_admission_candidate_projection(&request)
        .is_none());
    assert!(store.history_bytes_for_test()? == baseline);
    Ok(())
}

#[test]
fn history_provenance_chat_snapshot_failure_discards_all_targets_and_releases_charges(
) -> Result<(), TokenError> {
    for failure in 0..5 {
        let store = DlpTokenStore::new();
        let (lease, binding) = admitted(&store)?;
        let response = delivered();
        assert!(publish(&lease, &binding, &response));
        if failure == 4 {
            assert!(publish(&lease, &binding, &response));
            assert!(publish(&lease, &binding, &response));
        }
        let request = replay(&response);
        let projection = lease
            .history_projection(&binding, &request)
            .ok_or(TokenError::Unavailable)?;
        let baseline = store.history_bytes_for_test()?;
        let source = projection
            .source_for_responses_leaf(2, Field::ReasoningSummary, 0)
            .ok_or(TokenError::Unavailable)?;
        let output = projection
            .source_for_responses_leaf(1, Field::OutputText, 0)
            .ok_or(TokenError::Unavailable)?;
        let mut mapping = projection.mapping().ok_or(TokenError::Unavailable)?;
        let reasoning_target = Target::ChatReasoning { message_index: 0 };
        let content_target = Target::ChatContent { message_index: 0 };
        assert!(mapping.record(reasoning_target, &[source], Sep::None));
        assert!(mapping.record(content_target, &[output], Sep::None));
        let mut outbound = json!({"messages":[{"role":"assistant",
            "content":"synthetic output","reasoning_content":"synthetic first"}]});
        match failure {
            0 => outbound["messages"][0]["metadata"] = json!("s".repeat(256 * 1024)),
            1 => {
                let messages = outbound["messages"]
                    .as_array_mut()
                    .ok_or(TokenError::Unavailable)?;
                messages.resize(4097, Value::Null);
            }
            2 => {
                let mut nested = Value::Null;
                for _ in 0..65 {
                    nested = json!([nested]);
                }
                outbound["messages"][0]["metadata"] = nested;
            }
            3 => outbound["messages"][0]["metadata"] = json!(vec![Value::Null; 4096]),
            _ => (),
        }
        assert!(
            !mapping.seal_chat_targets(&outbound),
            "optional snapshot limits must fail closed"
        );
        assert!(
            mapping.failed(),
            "snapshot failure must latch the whole mapping"
        );
        for target in [reasoning_target, content_target] {
            assert!(
                !mapping.validates_target(&lease, &binding, &outbound, target),
                "no earlier target may survive snapshot failure"
            );
        }
        drop(mapping);
        assert!(
            store.history_bytes_for_test()? == baseline,
            "failed snapshot must release every temporary optional charge"
        );
    }
    Ok(())
}

#[test]
fn history_provenance_chat_snapshot_message_limit_and_lifetime_are_exact() -> Result<(), TokenError>
{
    let store = DlpTokenStore::new();
    let (lease, binding) = admitted(&store)?;
    let response = delivered();
    assert!(publish(&lease, &binding, &response));
    let request = replay(&response);
    let projection = lease
        .history_projection(&binding, &request)
        .ok_or(TokenError::Unavailable)?;
    let baseline = store.history_bytes_for_test()?;
    let source = projection
        .source_for_responses_leaf(2, Field::ReasoningSummary, 0)
        .ok_or(TokenError::Unavailable)?;
    let mut mapping = projection.mapping().ok_or(TokenError::Unavailable)?;
    let target = Target::ChatReasoning { message_index: 0 };
    assert!(mapping.record(target, &[source], Sep::None));
    let mut outbound =
        json!({"messages":[{"role":"assistant","reasoning_content":"synthetic first"}]});
    outbound["messages"]
        .as_array_mut()
        .ok_or(TokenError::Unavailable)?
        .resize(4096, Value::Null);
    assert!(mapping.seal_chat_targets(&outbound));
    assert!(mapping.validates_target(&lease, &binding, &outbound, target));
    assert!(
        store.history_bytes_for_test()? > baseline,
        "successful snapshot must retain optional charges with its owner"
    );
    drop(mapping);
    assert!(
        store.history_bytes_for_test()? == baseline,
        "successful snapshot must release all charges on drop"
    );
    Ok(())
}

#[test]
fn history_provenance_mapping_is_typed_ordered_exact_and_live() -> Result<(), TokenError> {
    let store = DlpTokenStore::new();
    let (lease, binding) = admitted(&store)?;
    let response = delivered();
    assert!(publish(&lease, &binding, &response));
    let request = replay(&response);
    let projection = lease
        .history_projection(&binding, &request)
        .ok_or(TokenError::Unavailable)?;
    let first = projection
        .source_for_responses_leaf(2, Field::ReasoningSummary, 0)
        .ok_or(TokenError::Unavailable)?;
    let second = projection
        .source_for_responses_leaf(2, Field::ReasoningSummary, 1)
        .ok_or(TokenError::Unavailable)?;
    let output = projection
        .source_for_responses_leaf(1, Field::OutputText, 0)
        .ok_or(TokenError::Unavailable)?;
    let mut mapping = projection.mapping().ok_or(TokenError::Unavailable)?;
    assert!(!mapping.record(
        Target::ChatContent { message_index: 0 },
        &[first.clone()],
        Sep::None
    ));
    assert!(!mapping.record(
        Target::ChatReasoning { message_index: 0 },
        &[second.clone(), first.clone()],
        Sep::Newline
    ));
    assert!(mapping.record(
        Target::ChatReasoning { message_index: 0 },
        &[first.clone(), second.clone()],
        Sep::Newline
    ));
    assert!(mapping.record(
        Target::ChatContent { message_index: 0 },
        &[output],
        Sep::None
    ));
    let mut target = json!({"messages":[{"role":"assistant","content":"synthetic output","reasoning_content":"synthetic first\nsynthetic second","tool_calls":[{"function":{"arguments":"synthetic first\nsynthetic second"}}]}]});
    assert!(mapping.seal_chat_targets(&target));
    assert!(mapping.validates_target(
        &lease,
        &binding,
        &target,
        Target::ChatReasoning { message_index: 0 }
    ));
    assert!(mapping.validates_target(
        &lease,
        &binding,
        &target,
        Target::ChatContent { message_index: 0 }
    ));
    assert!(!mapping.validates_target(
        &lease,
        &binding,
        &target,
        Target::ChatContent { message_index: 1 }
    ));
    target["messages"][0]["reasoning_content"] = json!("synthetic firstsynthetic second");
    assert!(!mapping.validates_target(
        &lease,
        &binding,
        &target,
        Target::ChatReasoning { message_index: 0 }
    ));
    target["messages"][0]["reasoning_content"] = json!("synthetic first\nsynthetic second");
    target["messages"][0]["role"] = json!("user");
    assert!(!mapping.validates_target(
        &lease,
        &binding,
        &target,
        Target::ChatReasoning { message_index: 0 }
    ));
    let mut changed = binding.clone();
    changed.policy_revision += 1;
    assert!(!mapping.validates_target(
        &lease,
        &changed,
        &target,
        Target::ChatReasoning { message_index: 0 }
    ));
    let mut exact = projection.mapping().ok_or(TokenError::Unavailable)?;
    assert!(exact.record(
        Target::ResponsesLeaf {
            input_index: 2,
            field: Field::ReasoningSummary,
            part_index: 0
        },
        &[first],
        Sep::None
    ));
    assert!(exact.validates_target(
        &lease,
        &binding,
        &request,
        Target::ResponsesLeaf {
            input_index: 2,
            field: Field::ReasoningSummary,
            part_index: 0
        }
    ));
    Ok(())
}

#[test]
fn history_provenance_capture_is_optional_atomic_bounded_and_releases() -> Result<(), TokenError> {
    let store = DlpTokenStore::new();
    let (lease, binding) = admitted(&store)?;
    let mandatory = store.allocation_ledger()?.used();
    let before = store.history_bytes_for_test()?;
    let mut response = delivered();
    response["output"]
        .as_array_mut()
        .ok_or(TokenError::Unavailable)?
        .push(json!({"type":"unknown","text":"synthetic unknown"}));
    assert!(!publish(&lease, &binding, &response));
    let after_init = store.history_bytes_for_test()?;
    assert!(after_init >= before && after_init < 4096);
    assert!(lease
        .history_projection(&binding, &replay(&delivered()))
        .is_none());
    let mut large = delivered();
    large["output"][1]["summary"][1]["text"] = json!("s".repeat(256 * 1024));
    assert!(!publish(&lease, &binding, &large));
    assert_eq!(store.history_bytes_for_test()?, after_init);
    assert!(lease
        .history_projection(&binding, &replay(&delivered()))
        .is_none());
    let pending = lease
        .capture_delivered_history(&binding, &delivered())
        .ok_or(TokenError::Unavailable)?;
    assert!(store.history_bytes_for_test()? >= after_init + 256 * 1024);
    drop(pending);
    assert_eq!(store.history_bytes_for_test()?, after_init);
    assert_eq!(store.allocation_ledger()?.used(), mandatory);
    Ok(())
}

#[test]
fn history_provenance_namespace_global_pending_and_replacement_peaks_are_charged(
) -> Result<(), TokenError> {
    let store = DlpTokenStore::new();
    let (lease, binding) = admitted(&store)?;
    let response = delivered();
    let mut held = Vec::new();
    while let Some(pending) = lease.capture_delivered_history(&binding, &response) {
        held.push(pending);
    }
    assert!(!held.is_empty() && held.len() <= 4);
    let used = store.history_bytes_for_test()?;
    assert!(used <= 1024 * 1024 + 4096);
    assert!(!publish(&lease, &binding, &response));
    drop(held);
    assert!(publish(&lease, &binding, &response));
    let request = replay(&response);
    let pinned = lease
        .history_projection(&binding, &request)
        .ok_or(TokenError::Unavailable)?;
    for _ in 0..8 {
        let _ = publish(&lease, &binding, &response);
    }
    assert!(pinned
        .source_for_responses_leaf(2, Field::ReasoningSummary, 0)
        .is_some());
    assert!(store.history_bytes_for_test()? <= 1024 * 1024 + 4096);
    drop(pinned);
    drop(lease);
    store.expire_idle_at(Instant::now() + Duration::from_secs(7200))?;
    assert!(store.history_bytes_for_test()? < 4096);
    let mut held = Vec::new();
    for index in 0..40 {
        let lease = store.admit("codex", Some(&format!("synthetic-global-{index}")), 1)?;
        let binding = lease.bind_actual_attempt_extracted(
            "synthetic-provider",
            "responses",
            "https://synthetic.invalid/v1/responses",
            Some("synthetic-principal"),
        )?;
        if let Some(pending) = lease.capture_delivered_history(&binding, &response) {
            held.push(pending);
        }
    }
    assert!(!held.is_empty() && held.len() < 40);
    assert!(store.history_bytes_for_test()? <= 8 * 1024 * 1024);
    drop(held);
    store.expire_idle_at(Instant::now() + Duration::from_secs(7200))?;
    assert!(store.history_bytes_for_test()? < 4096);
    Ok(())
}

#[test]
fn history_provenance_process_global_limit_spans_distinct_stores() -> Result<(), TokenError> {
    let response = delivered();
    let mut owners = Vec::new();
    let mut held = Vec::new();
    for _ in 0..40 {
        let store = DlpTokenStore::new();
        let (lease, binding) = admitted(&store)?;
        if let Some(pending) = lease.capture_delivered_history(&binding, &response) {
            held.push(pending);
        }
        owners.push((store, lease));
    }
    assert!(!held.is_empty() && held.len() < 32);
    drop(held);
    drop(owners);
    Ok(())
}

#[test]
fn history_provenance_source_coordinates_require_unchanged_consumed_body() -> Result<(), TokenError>
{
    let store = DlpTokenStore::new();
    let (lease, binding) = admitted(&store)?;
    let response = delivered();
    assert!(publish(&lease, &binding, &response));
    let original = replay(&response);
    let projection = lease
        .history_admission_candidate_projection(&original)
        .ok_or(TokenError::Unavailable)?;
    assert!(projection.consumed_body_matches(&original));
    assert!(projection
        .source_for_responses_leaf(2, Field::ReasoningSummary, 0)
        .is_some());
    let mut changed = original.clone();
    changed["input"][2]["summary"][0]["text"] = json!("synthetic changed");
    assert!(!projection.consumed_body_matches(&changed));
    let changed_projection = lease
        .history_admission_candidate_projection(&changed)
        .ok_or(TokenError::Unavailable)?;
    assert!(changed_projection
        .source_for_responses_leaf(2, Field::ReasoningSummary, 0)
        .is_none());
    let other = store.admit("codex", Some("synthetic-unrelated-history"), 1)?;
    assert!(other
        .history_admission_candidate_projection(&original)
        .is_none());
    let other_policy = store.admit("codex", Some("synthetic-history"), 2)?;
    assert!(other_policy
        .history_admission_candidate_projection(&original)
        .is_none());
    Ok(())
}

#[test]
fn history_provenance_trace_semantic_invalidation_is_target_local() -> Result<(), TokenError> {
    let store = DlpTokenStore::new();
    let (lease, binding) = admitted(&store)?;
    let response = delivered();
    assert!(publish(&lease, &binding, &response));
    let request = replay(&response);
    let projection = lease
        .history_projection(&binding, &request)
        .ok_or(TokenError::Unavailable)?;
    let reasoning = projection
        .source_for_responses_leaf(2, Field::ReasoningSummary, 0)
        .ok_or(TokenError::Unavailable)?;
    let output = projection
        .source_for_responses_leaf(1, Field::OutputText, 0)
        .ok_or(TokenError::Unavailable)?;
    let mut mapping = projection.mapping().ok_or(TokenError::Unavailable)?;
    assert!(mapping.record(
        Target::ChatContent { message_index: 0 },
        &[output],
        Sep::None
    ));
    let mut trace = projection.trace().ok_or(TokenError::Unavailable)?;
    assert!(trace.push(&reasoning, Sep::None));
    trace.invalidate();
    assert!(!trace.failed() && !trace.valid());
    assert!(!mapping.record_trace(Target::ChatReasoning { message_index: 0 }, trace));
    assert!(mapping.valid() && !mapping.failed());
    let outbound = json!({"messages":[{"role":"assistant","content":"synthetic output","reasoning_content":"synthetic first"}]});
    assert!(mapping.seal_chat_targets(&outbound));
    assert!(mapping.validates_target(
        &lease,
        &binding,
        &outbound,
        Target::ChatContent { message_index: 0 }
    ));
    assert!(!mapping.validates_target(
        &lease,
        &binding,
        &outbound,
        Target::ChatReasoning { message_index: 0 }
    ));
    Ok(())
}

#[test]
fn history_provenance_trace_growth_failure_invalidates_entire_mapping() -> Result<(), TokenError> {
    let store = DlpTokenStore::new();
    let (lease, binding) = admitted(&store)?;
    let response = delivered();
    assert!(publish(&lease, &binding, &response));
    let mut request = replay(&response);
    for _ in 0..512 {
        request["input"]
            .as_array_mut()
            .ok_or(TokenError::Unavailable)?
            .push(response["output"][1].clone());
    }
    let projection = lease
        .history_projection(&binding, &request)
        .ok_or(TokenError::Unavailable)?;
    let output = projection
        .source_for_responses_leaf(1, Field::OutputText, 0)
        .ok_or(TokenError::Unavailable)?;
    let mut mapping = projection.mapping().ok_or(TokenError::Unavailable)?;
    assert!(mapping.record(
        Target::ChatContent { message_index: 0 },
        &[output],
        Sep::None
    ));
    let mut trace = projection.trace().ok_or(TokenError::Unavailable)?;
    for index in 5..517 {
        let source = projection
            .source_for_responses_leaf(index, Field::ReasoningSummary, 0)
            .ok_or(TokenError::Unavailable)?;
        if !trace.push(&source, if index == 5 { Sep::None } else { Sep::Newline }) {
            break;
        }
    }
    assert!(trace.failed() && !trace.valid());
    assert!(!mapping.record_trace(Target::ChatReasoning { message_index: 0 }, trace));
    assert!(mapping.failed() && !mapping.valid());
    let outbound = json!({"messages":[{"role":"assistant","content":"synthetic output"}]});
    assert!(!mapping.validates_target(
        &lease,
        &binding,
        &outbound,
        Target::ChatContent { message_index: 0 }
    ));
    Ok(())
}

#[test]
fn history_provenance_namespace_control_stays_charged_until_last_trace() -> Result<(), TokenError> {
    let store = DlpTokenStore::new();
    let (lease, binding) = admitted(&store)?;
    let response = delivered();
    assert!(publish(&lease, &binding, &response));
    let request = replay(&response);
    let projection = lease
        .history_projection(&binding, &request)
        .ok_or(TokenError::Unavailable)?;
    let trace = projection.trace().ok_or(TokenError::Unavailable)?;
    let control = super::token_store_owners::StoreAllocationLedger::control_bytes()?;
    drop(lease);
    store.expire_idle_at(Instant::now() + Duration::from_secs(7200))?;
    drop(projection);
    assert!(store.history_bytes_for_test()? >= 16 * 1024 + 2 * control);
    drop(trace);
    assert!(store.history_bytes_for_test()? < 4096);
    Ok(())
}

#[test]
fn history_provenance_chat_snapshot_each_fallible_copy_failure_revokes_all_targets(
) -> Result<(), TokenError> {
    use super::token_store_owners::test_support::{install, Kind, Phase};
    use std::{cell::Cell, rc::Rc};

    let store = DlpTokenStore::new();
    let (lease, binding) = admitted(&store)?;
    let response = delivered();
    assert!(publish(&lease, &binding, &response));
    let request = replay(&response);
    let projection = lease
        .history_projection(&binding, &request)
        .ok_or(TokenError::Unavailable)?;
    let baseline = store.history_bytes_for_test()?;
    let mandatory = store.allocation_ledger()?.used();
    let reasoning = projection
        .source_for_responses_leaf(2, Field::ReasoningSummary, 0)
        .ok_or(TokenError::Unavailable)?;
    let output = projection
        .source_for_responses_leaf(1, Field::OutputText, 0)
        .ok_or(TokenError::Unavailable)?;
    let targets = [
        Target::ChatReasoning { message_index: 0 },
        Target::ChatContent { message_index: 0 },
    ];
    let mapping = || {
        let mut mapping = projection.mapping().ok_or(TokenError::Unavailable)?;
        assert!(mapping.record(targets[0], &[reasoning.clone()], Sep::None));
        assert!(mapping.record(targets[1], &[output.clone()], Sep::None));
        Ok::<_, TokenError>(mapping)
    };
    let outbound = json!({"messages":[{"role":"assistant",
        "content":"synthetic output","reasoning_content":"synthetic first",
        "metadata":{"synthetic":["synthetic nested",{"synthetic":true}]},
        "tool_calls":[{"id":"synthetic-call","type":"function",
            "function":{"name":"synthetic_tool","arguments":"{}"}}]}]});
    let unchanged = outbound.clone();
    let count = Rc::new(Cell::new(0));
    let kinds = Rc::new(Cell::new(0_usize));
    let mut successful = mapping()?;
    let observed = count.clone();
    let observed_kinds = kinds.clone();
    let guard = install(move |observation| {
        if matches!(observation.kind, Kind::Vector | Kind::Text)
            && matches!(
                observation.phase,
                Phase::BeforeAllocation | Phase::AfterAllocation | Phase::BeforeCopy
            )
        {
            observed.set(observed.get() + 1);
            observed_kinds.set(
                observed_kinds.get()
                    | match observation.kind {
                        Kind::Vector => 1,
                        _ => 2,
                    },
            );
        }
        Ok(())
    });
    assert!(successful.seal_chat_targets(&outbound));
    drop(guard);
    assert!(count.get() > 5 && kinds.get() == 3);
    for target in targets {
        assert!(successful.validates_target(&lease, &binding, &outbound, target));
    }
    drop(successful);
    assert!(store.history_bytes_for_test()? == baseline);

    for failure in 0..count.get() {
        let mut failed = mapping()?;
        let seen = Rc::new(Cell::new(0));
        let observed = seen.clone();
        let guard = install(move |observation| {
            if matches!(observation.kind, Kind::Vector | Kind::Text)
                && matches!(
                    observation.phase,
                    Phase::BeforeAllocation | Phase::AfterAllocation | Phase::BeforeCopy
                )
            {
                let ordinal = observed.get();
                observed.set(ordinal + 1);
                if ordinal == failure {
                    return Err(TokenError::Unavailable);
                }
            }
            Ok(())
        });
        let sealed = failed.seal_chat_targets(&outbound);
        drop(guard);
        assert!(!sealed && failed.failed());
        assert!(seen.get() == failure + 1);
        for target in targets {
            assert!(!failed.validates_target(&lease, &binding, &outbound, target));
        }
        assert!(outbound == unchanged);
        drop(failed);
        assert!(store.history_bytes_for_test()? == baseline);
        assert_eq!(store.allocation_ledger()?.used(), mandatory);
    }
    Ok(())
}

#[test]
fn history_provenance_chat_snapshot_depth_64_65_boundary_is_exact() -> Result<(), TokenError> {
    let store = DlpTokenStore::new();
    let (lease, binding) = admitted(&store)?;
    let response = delivered();
    assert!(publish(&lease, &binding, &response));
    let request = replay(&response);
    let projection = lease
        .history_projection(&binding, &request)
        .ok_or(TokenError::Unavailable)?;
    let baseline = store.history_bytes_for_test()?;
    let source = projection
        .source_for_responses_leaf(2, Field::ReasoningSummary, 0)
        .ok_or(TokenError::Unavailable)?;
    let target = Target::ChatReasoning { message_index: 0 };
    for depth in [64, 65] {
        let mut nested = Value::Null;
        for _ in 1..depth {
            nested = json!([nested]);
        }
        let outbound = json!({"messages":[{"role":"assistant",
            "reasoning_content":"synthetic first","metadata":nested}]});
        let mut mapping = projection.mapping().ok_or(TokenError::Unavailable)?;
        assert!(mapping.record(target, &[source.clone()], Sep::None));
        assert_eq!(mapping.seal_chat_targets(&outbound), depth == 64);
        assert_eq!(mapping.failed(), depth == 65);
        assert_eq!(
            mapping.validates_target(&lease, &binding, &outbound, target),
            depth == 64
        );
        drop(mapping);
        assert!(store.history_bytes_for_test()? == baseline);
    }
    Ok(())
}

#[test]
fn history_provenance_chat_snapshot_multibyte_exact_byte_ceiling_releases_charges(
) -> Result<(), TokenError> {
    use super::token_store_owners::test_support::{install, Kind, Phase};
    use std::{cell::Cell, rc::Rc};

    let store = DlpTokenStore::new();
    let (lease, binding) = admitted(&store)?;
    let response = delivered();
    assert!(publish(&lease, &binding, &response));
    let request = replay(&response);
    let projection = lease
        .history_projection(&binding, &request)
        .ok_or(TokenError::Unavailable)?;
    let baseline = store.history_bytes_for_test()?;
    let source = projection
        .source_for_responses_leaf(2, Field::ReasoningSummary, 0)
        .ok_or(TokenError::Unavailable)?;
    let target = Target::ChatReasoning { message_index: 0 };
    let mapping = || {
        let mut mapping = projection.mapping().ok_or(TokenError::Unavailable)?;
        assert!(mapping.record(target, &[source.clone()], Sep::None));
        Ok::<_, TokenError>(mapping)
    };
    let mut outbound = json!({"messages":[{"role":"assistant",
        "reasoning_content":"synthetic first","metadata":"界"}]});
    let peak = Rc::new(Cell::new(0_usize));
    let observed = peak.clone();
    let mut measured = mapping()?;
    let guard = install(move |observation| {
        if matches!(observation.kind, Kind::Vector | Kind::Text)
            && matches!(observation.phase, Phase::AfterAllocation)
        {
            observed.set(
                observed
                    .get()
                    .max(observation.used.plaintext + observation.used.metadata),
            );
        }
        Ok(())
    });
    assert!(measured.seal_chat_targets(&outbound));
    drop(guard);
    drop(measured);
    assert!(store.history_bytes_for_test()? == baseline);
    let overhead = peak
        .get()
        .checked_sub("界".len())
        .ok_or(TokenError::Unavailable)?;
    let text_bytes = (256_usize * 1024)
        .checked_sub(overhead)
        .ok_or(TokenError::Unavailable)?;
    assert!(text_bytes > 250 * 1024);
    let text = "界".repeat(text_bytes / 3) + &"s".repeat(text_bytes % 3);
    assert!(text.len() == text_bytes && text.chars().count() < text_bytes / 2);
    outbound["messages"][0]["metadata"] = json!(text);
    let mut exact = mapping()?;
    assert!(exact.seal_chat_targets(&outbound));
    assert!(exact.validates_target(&lease, &binding, &outbound, target));
    drop(exact);
    assert!(store.history_bytes_for_test()? == baseline);
    let mut one_more = outbound["messages"][0]["metadata"]
        .as_str()
        .ok_or(TokenError::Unavailable)?
        .to_owned();
    one_more.push('s');
    outbound["messages"][0]["metadata"] = json!(one_more);
    let mut excess = mapping()?;
    assert!(!excess.seal_chat_targets(&outbound) && excess.failed());
    assert!(!excess.validates_target(&lease, &binding, &outbound, target));
    drop(excess);
    assert!(store.history_bytes_for_test()? == baseline);
    Ok(())
}

#[test]
fn history_provenance_record_control_unavailability_discards_pending_and_releases(
) -> Result<(), TokenError> {
    use super::token_store_owners::test_support::{install, Kind, Phase};
    let store = DlpTokenStore::new();
    let (lease, binding) = admitted(&store)?;
    let response = delivered();
    let initialized = lease
        .capture_delivered_history(&binding, &response)
        .ok_or(TokenError::Unavailable)?;
    drop(initialized);
    let baseline = store.history_bytes_for_test()?;
    let mandatory = store.allocation_ledger()?.used();
    let guard = install(|observation| {
        if observation.kind == Kind::Shared && observation.phase == Phase::BeforeAllocation {
            return Err(TokenError::Unavailable);
        }
        Ok(())
    });
    let pending = lease.capture_delivered_history(&binding, &response);
    drop(guard);
    assert!(pending.is_none());
    assert!(store.history_bytes_for_test()? == baseline);
    assert_eq!(store.allocation_ledger()?.used(), mandatory);
    assert!(lease
        .history_projection(&binding, &replay(&response))
        .is_none());
    assert!(publish(&lease, &binding, &response));
    Ok(())
}

fn concurrent_history_response(index: usize) -> Value {
    let mut response = delivered();
    for item in 0..3 {
        response["output"][item]["id"] = json!(format!("synthetic-concurrent-{index}-{item}"));
    }
    response["output"][2]["call_id"] = json!(format!("synthetic-concurrent-call-{index}"));
    response
}

#[test]
fn history_provenance_concurrent_pending_capture_publish_drop_is_atomic_and_bounded(
) -> Result<(), TokenError> {
    use super::{history_provenance::HistoryNamespace, token_store_owners::StoreAllocationLedger};
    use std::sync::Barrier;

    const WORKERS: usize = 8;
    const RESPONSE_BYTES: usize = 256 * 1024;
    let store = DlpTokenStore::new();
    let (lease, binding) = admitted(&store)?;
    let mandatory = store.allocation_ledger()?.used();
    // A private ledger makes namespace accounting independent of parallel tests.
    let metadata = StoreAllocationLedger::new(0, 8 * 1024 * 1024)?;
    let baseline = metadata.used();
    let namespace = HistoryNamespace::new(metadata.clone()).ok_or(TokenError::Unavailable)?;
    let control = metadata.used();
    let responses: Vec<_> = (0..WORKERS).map(concurrent_history_response).collect();
    let replays: Vec<_> = responses.iter().map(replay).collect();
    let barrier = Barrier::new(WORKERS + 1);
    let (pending_charge, unpublished, results) = std::thread::scope(|scope| {
        let mut workers = Vec::new();
        for (index, response) in responses.iter().enumerate() {
            let namespace = namespace.clone();
            let binding = &binding;
            let barrier = &barrier;
            workers.push(scope.spawn(move || {
                barrier.wait();
                let pending = namespace
                    .get()
                    .ok()
                    .and_then(|ns| ns.capture(binding, response));
                let captured = pending.is_some();
                barrier.wait();
                barrier.wait();
                let published = pending.is_some_and(|pending| {
                    if index % 2 == 0 {
                        namespace.get().is_ok_and(|ns| ns.publish(pending).is_ok())
                    } else {
                        drop(pending);
                        false
                    }
                });
                (captured, published)
            }));
        }
        barrier.wait();
        barrier.wait();
        let pending_charge = metadata.used();
        let unpublished = replays.iter().all(|request| {
            namespace.get().is_ok_and(|ns| {
                ns.projection(store.clone(), Some(&binding), request)
                    .is_none()
            })
        });
        barrier.wait();
        let results = workers
            .into_iter()
            .map(|worker| worker.join().map_err(|_| TokenError::Unavailable))
            .collect::<Result<Vec<_>, _>>();
        (pending_charge, unpublished, results)
    });
    let results = results?;
    let captures = results.iter().filter(|(captured, _)| *captured).count();
    let published = results.iter().filter(|(_, published)| *published).count();
    assert!(
        captures == 3
            && pending_charge.plaintext == 0
            && pending_charge.metadata == control.metadata + captures * RESPONSE_BYTES,
        "concurrent pending captures must charge the exact bounded response permits"
    );
    assert!(
        unpublished,
        "pending captures must not expose partial history proof"
    );
    assert!(
        metadata.used().metadata == control.metadata + published * RESPONSE_BYTES,
        "publication and cancellation must retain only published response charges"
    );
    for (request, (_, published)) in replays.iter().zip(&results) {
        let projection = namespace
            .get()?
            .projection(store.clone(), Some(&binding), request);
        assert!(
            projection.is_some() == *published,
            "a response proof must publish all or none"
        );
        if let Some(projection) = projection {
            for (index, field, part) in [
                (1, Field::OutputText, 0),
                (2, Field::ReasoningSummary, 0),
                (2, Field::ReasoningSummary, 1),
                (2, Field::ReasoningContent, 0),
                (3, Field::CallReasoning, 0),
            ] {
                assert!(
                    projection
                        .source_for_responses_leaf(index, field, part)
                        .is_some(),
                    "a published response must retain every complete typed item"
                );
            }
        }
    }
    let mut wrong_policy = binding.clone();
    wrong_policy.policy_revision += 1;
    assert!(
        replays
            .iter()
            .all(|request| namespace.get().is_ok_and(|ns| {
                ns.projection(store.clone(), Some(&wrong_policy), request)
                    .is_none()
            })),
        "concurrent publication must not transfer proof to a different policy"
    );
    assert!(
        lease.history_binding_live(&binding),
        "the exact fixture binding must remain live"
    );
    assert!(
        store.allocation_ledger()?.used() == mandatory,
        "optional proof must not consume mandatory headroom"
    );
    drop(namespace);
    assert!(
        metadata.used() == baseline,
        "the last local namespace owner must release every optional charge"
    );
    Ok(())
}

// Process-global delta equality requires an isolated run with --test-threads=1.
#[test]
#[serial_test::serial]
fn history_provenance_concurrent_publication_and_last_shared_owner_release(
) -> Result<(), TokenError> {
    use super::history_provenance::global_history_allocation;
    use std::sync::Barrier;

    const WORKERS: usize = 3;
    const READERS: usize = 8;
    const RESPONSE_BYTES: usize = 256 * 1024;
    let global = global_history_allocation()?;
    let global_before = global.used();
    let store = DlpTokenStore::new();
    let mandatory_before = store.allocation_ledger()?.used();
    let (lease, binding) = admitted(&store)?;
    let responses: Vec<_> = (0..WORKERS).map(concurrent_history_response).collect();
    let replays: Vec<_> = responses.iter().map(replay).collect();
    let warmup = lease
        .capture_delivered_history(&binding, &responses[0])
        .ok_or(TokenError::Unavailable)?;
    drop(warmup);
    let control = global.used();
    let mandatory = store.allocation_ledger()?.used();
    let leases = (0..WORKERS)
        .map(|_| lease.fork_attempt_extracted())
        .collect::<Result<Vec<_>, _>>()?;
    let barrier = Barrier::new(WORKERS + 1);
    let (pending_charge, unpublished, results) = std::thread::scope(|scope| {
        let mut workers = Vec::new();
        for (index, worker_lease) in leases.into_iter().enumerate() {
            let binding = &binding;
            let response = &responses[index];
            let barrier = &barrier;
            workers.push(scope.spawn(move || {
                barrier.wait();
                let pending = worker_lease.capture_delivered_history(binding, response);
                let captured = pending.is_some();
                barrier.wait();
                barrier.wait();
                let published = pending.is_some_and(|pending| match index {
                    0 => worker_lease.publish_delivered_history(binding, pending),
                    1 => {
                        let mut wrong_policy = binding.clone();
                        wrong_policy.policy_revision += 1;
                        worker_lease.publish_delivered_history(&wrong_policy, pending)
                    }
                    _ => {
                        drop(pending);
                        false
                    }
                });
                (captured, published)
            }));
        }
        barrier.wait();
        barrier.wait();
        let pending_charge = global.used();
        let unpublished = replays
            .iter()
            .all(|request| lease.history_projection(&binding, request).is_none());
        barrier.wait();
        let results = workers
            .into_iter()
            .map(|worker| worker.join().map_err(|_| TokenError::Unavailable))
            .collect::<Result<Vec<_>, _>>();
        (pending_charge, unpublished, results)
    });
    let results = results?;
    assert!(
        results.iter().all(|(captured, _)| *captured)
            && pending_charge.plaintext == global_before.plaintext
            && pending_charge.metadata == control.metadata + WORKERS * RESPONSE_BYTES,
        "parallel pending captures must retain every response charge until resolved"
    );
    assert!(
        unpublished,
        "pending public captures must not authorize history"
    );
    assert!(
        results.iter().enumerate().all(|(index, (_, published))| *published == (index == 0)),
        "only the exact live policy binding may publish while other captures are rejected or dropped"
    );
    for (index, request) in replays.iter().enumerate() {
        let projection = lease.history_projection(&binding, request);
        assert!(
            projection.is_some() == (index == 0),
            "rejected and dropped captures must leave no partial proof"
        );
    }
    assert!(
        global.used().metadata == control.metadata + RESPONSE_BYTES
            && store.allocation_ledger()?.used() == mandatory,
        "parallel resolution must retain one optional record without mandatory charges"
    );
    let projection = lease
        .history_projection(&binding, &replays[0])
        .ok_or(TokenError::Unavailable)?;
    let source = projection
        .source_for_responses_leaf(2, Field::ReasoningSummary, 0)
        .ok_or(TokenError::Unavailable)?;
    drop(lease);
    store.expire_idle_at(Instant::now() + Duration::from_secs(7200))?;
    assert!(
        store.stats()?.namespaces == 0,
        "shared proof pins must not keep a namespace live"
    );
    assert!(
        projection
            .source_for_responses_leaf(2, Field::ReasoningSummary, 0)
            .is_none(),
        "a pinned reader must lose authority when its namespace expires"
    );
    drop(projection);
    let pinned = global.used();
    assert!(
        pinned.metadata > global_before.metadata + RESPONSE_BYTES,
        "record pins must retain the namespace controller charge"
    );
    let barrier = Barrier::new(READERS + 1);
    let last_release = Barrier::new(2);
    let (cloned, one_remaining, joined) = std::thread::scope(|scope| {
        let mut workers = Vec::new();
        for index in 0..READERS {
            let source = source.clone();
            let barrier = &barrier;
            let last_release = &last_release;
            workers.push(scope.spawn(move || {
                barrier.wait();
                barrier.wait();
                if index == 0 {
                    last_release.wait();
                }
                drop(source);
            }));
        }
        drop(source);
        barrier.wait();
        let cloned = global.used();
        barrier.wait();
        let last = workers.remove(0);
        let mut joined = true;
        for worker in workers {
            joined &= worker.join().is_ok();
        }
        let one_remaining = global.used();
        last_release.wait();
        joined &= last.join().is_ok();
        (cloned, one_remaining, joined)
    });
    assert!(
        joined,
        "shared history owner threads must finish without panic"
    );
    assert!(
        cloned == pinned && one_remaining == pinned,
        "shared clones and racing nonfinal drops must neither double-charge nor release the final owner"
    );
    assert!(
        global.used() == global_before && store.allocation_ledger()?.used() == mandatory_before,
        "final shared owner release must return the complete optional delta and local mandatory charges"
    );
    Ok(())
}
