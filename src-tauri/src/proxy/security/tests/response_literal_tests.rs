//! In-memory response compatibility; unmatched text never gains restore authority.
use super::{
    completed_context::CompletionWitness,
    payload_budget::PayloadBudget,
    response_restoration::{
        restore_and_publish_json_response, stage_json_response, ArgumentNormalizationMode,
        RestoreError, SseRestorationState,
    },
    restore_diagnostics::RestoreReason,
    tokenization::{DlpRequestLease, DlpTokenStore, TokenCategory, TokenFragment},
};
use serde_json::{json, Value};
type TestResult = Result<(), Box<dyn std::error::Error>>;
fn literals() -> Vec<String> {
    vec![
        ["rredact-", "custom-"].concat(),
        ["rredact-", "custom-invalid"].concat(),
        format!("rredact-custom-{}-ai", "a".repeat(20)),
        ["vary_", "cu_"].concat(),
    ]
}
fn fixture() -> Result<
    (
        DlpRequestLease,
        super::completed_context::ActualAttemptBinding,
        PayloadBudget,
        String,
    ),
    Box<dyn std::error::Error>,
> {
    let store = DlpTokenStore::new();
    let mut lease = store.admit("codex", Some("synthetic-response-literal"), 1)?;
    let refs = lease.prepare(
        &[TokenFragment {
            raw: "synthetic-restored",
            whole_span_categories: &[TokenCategory::Custom],
        }],
        &[],
        |refs| Ok(refs.to_vec()),
    )?;
    let binding = lease.bind_actual_attempt(
        "synthetic-provider",
        "openai_responses",
        "https://synthetic.invalid/v1/responses",
        Some("synthetic-principal"),
    )?;
    Ok((
        lease,
        binding,
        PayloadBudget::with_limit(64 * 1024 * 1024),
        refs[0].clone(),
    ))
}
fn response(text: &str, arguments: &str) -> Value {
    json!({"id":"synthetic-response","object":"response","status":"completed","output":[
        {"id":"synthetic-message","type":"message","role":"assistant","status":"completed","content":[{"type":"output_text","text":text}]},
        {"id":"synthetic-function","type":"function_call","call_id":"synthetic-call","name":"synthetic-tool","arguments":arguments,"status":"completed"}
    ]})
}
#[test]
fn placeholder_passthrough_response_json_text_and_tool_arguments() -> TestResult {
    for literal in literals() {
        let (lease, binding, budget, reference) = fixture()?;
        let text = format!("{literal} {reference}");
        let arguments = serde_json::to_string(&json!({"value":text}))?;
        let raw = response(&text, &arguments);
        let witness = CompletionWitness::from_verified_completion(true)
            .map_err(|_| "synthetic_witness_missing")?;
        let restored =
            restore_and_publish_json_response(&raw, false, &lease, &binding, &witness, &budget)
                .map_err(|_| "literal_response_rejected")?;
        let expected = format!("{literal} synthetic-restored");
        assert!(
            restored.as_value()["output"][0]["content"][0]["text"].as_str()
                == Some(expected.as_str()),
            "response_literal_changed"
        );
        let args: Value = serde_json::from_str(
            restored.as_value()["output"][1]["arguments"]
                .as_str()
                .ok_or("arguments_missing")?,
        )?;
        assert!(
            args["value"].as_str() == Some(expected.as_str()),
            "tool_literal_changed"
        );
        assert!(
            restored.restored_occurrences() == 2,
            "unmatched_counted_as_restored"
        );
    }
    Ok(())
}
fn frame(event: &str, value: Value) -> String {
    format!("event: {event}\ndata: {value}\n\n")
}
#[test]
fn placeholder_passthrough_response_sse_every_split_and_partial_tail() -> TestResult {
    for literal in literals() {
        for split in 1..literal.len() {
            let (lease, binding, budget, _) = fixture()?;
            let mut state =
                SseRestorationState::new(lease, binding, budget, ArgumentNormalizationMode::Exact)
                    .map_err(|_| "sse_state_failed")?;
            for delta in [&literal[..split], &literal[split..]] {
                state.process_sse_block(&frame("response.output_text.delta",json!({"type":"response.output_text.delta","output_index":0,"content_index":0,"delta":delta}))).map_err(|_| "literal_delta_rejected")?;
            }
            state.process_sse_block(&frame("response.output_text.done",json!({"type":"response.output_text.done","output_index":0,"content_index":0,"text":literal}))).map_err(|_| "literal_done_rejected")?;
            let raw = json!({"id":"synthetic-stream","object":"response","status":"completed","output":[{"id":"synthetic-message","type":"message","role":"assistant","status":"completed","content":[{"type":"output_text","text":literal}]}]});
            state
                .process_sse_block(&frame(
                    "response.completed",
                    json!({"type":"response.completed","response":raw}),
                ))
                .map_err(|_| "literal_completion_rejected")?;
            state
                .finalize_and_flush(true)
                .map_err(|_| "literal_flush_rejected")?;
        }
    }
    Ok(())
}
#[test]
fn placeholder_passthrough_response_sse_refusal_partial_tail() -> TestResult {
    let literal = literals().remove(0);
    let (lease, binding, budget, _) = fixture()?;
    let mut state =
        SseRestorationState::new(lease, binding, budget, ArgumentNormalizationMode::Exact)
            .map_err(|_| "sse_state_failed")?;
    state.process_sse_block(&frame("response.refusal.delta",json!({"type":"response.refusal.delta","output_index":0,"content_index":0,"delta":literal}))).map_err(|_| "refusal_delta_rejected")?;
    state.process_sse_block(&frame("response.refusal.done",json!({"type":"response.refusal.done","output_index":0,"content_index":0,"refusal":literal}))).map_err(|_| "refusal_done_rejected")?;
    let raw = json!({"id":"synthetic-refusal","object":"response","status":"completed","output":[{"id":"synthetic-message","type":"message","role":"assistant","status":"completed","content":[{"type":"refusal","refusal":literal}]}]});
    state
        .process_sse_block(&frame(
            "response.completed",
            json!({"type":"response.completed","response":raw}),
        ))
        .map_err(|_| "refusal_completion_rejected")?;
    state
        .finalize_and_flush(true)
        .map_err(|_| "refusal_flush_rejected")?;
    Ok(())
}

#[test]
fn placeholder_passthrough_response_foreign_tool_snapshot_never_grants_authority() -> TestResult {
    let store = DlpTokenStore::new();
    let mut issuer = store.admit("codex", Some("synthetic-foreign-issuer"), 1)?;
    let reference = issuer.prepare(
        &[TokenFragment {
            raw: "synthetic-private",
            whole_span_categories: &[TokenCategory::Custom],
        }],
        &[],
        |refs| Ok(refs[0].clone()),
    )?;
    let lease = store.admit("codex", Some("synthetic-foreign-receiver"), 1)?;
    let binding = lease.bind_actual_attempt(
        "synthetic-provider",
        "openai_responses",
        "https://synthetic.invalid/v1/responses",
        Some("synthetic-principal"),
    )?;
    let budget = PayloadBudget::with_limit(64 * 1024 * 1024);
    let arguments = format!("{{ \"value\" : \"{reference}\" }}");
    let mut raw = response(&reference, &arguments);
    raw["output"].as_array_mut().ok_or("output_missing")?.push(json!({"id":"synthetic-custom","type":"custom_tool_call","call_id":"synthetic-custom-call","name":"synthetic-tool","input":reference,"status":"completed"}));
    raw["metadata"] = json!({"literal":reference});
    let witness = CompletionWitness::from_verified_completion(true)?;
    let restored =
        restore_and_publish_json_response(&raw, false, &lease, &binding, &witness, &budget)
            .map_err(|_| "foreign_snapshot_rejected")?;
    assert!(restored.as_value() == &raw, "foreign_snapshot_changed");
    assert!(
        restored.restored_occurrences() == 0,
        "foreign_snapshot_counted"
    );
    let mut next = store.admit("codex", Some("synthetic-foreign-receiver"), 1)?;
    let actual = next.bind_actual_attempt(
        "synthetic-provider",
        "openai_responses",
        "https://synthetic.invalid/v1/responses",
        Some("synthetic-principal"),
    )?;
    next.resume_completed_context(&actual, Some("synthetic-response"), &[], &[])?;
    assert!(
        next.is_empty() && next.resolve(&reference).is_err(),
        "foreign_snapshot_authorized"
    );
    assert!(
        next.resume_completed_tokens(&actual, &[&reference])
            .is_err(),
        "strict_resume_widened"
    );
    drop(restored);
    assert!(budget.used() == 0, "response_budget_retained");
    Ok(())
}
#[test]
fn placeholder_passthrough_response_independent_failures_stay_closed() -> TestResult {
    let (lease, binding, budget, _) = fixture()?;
    let raw = response(&literals()[0], "invalid-json");
    let witness = CompletionWitness::from_verified_completion(true)?;
    assert!(
        restore_and_publish_json_response(&raw, false, &lease, &binding, &witness, &budget)
            .is_err(),
        "invalid_json_accepted"
    );
    assert!(
        CompletionWitness::from_verified_completion(false).is_err(),
        "completion_witness_forged"
    );
    let small = PayloadBudget::with_limit(1);
    assert!(
        restore_and_publish_json_response(
            &response(&literals()[0], "{}"),
            false,
            &lease,
            &binding,
            &witness,
            &small
        )
        .is_err(),
        "response_budget_bypassed"
    );
    assert!(small.used() == 0, "failed_response_budget_retained");
    Ok(())
}
#[test]
fn placeholder_passthrough_response_sse_rejects_snapshot_mismatch_and_missing_completion(
) -> TestResult {
    let (lease, binding, budget, _) = fixture()?;
    let mut state =
        SseRestorationState::new(lease, binding, budget, ArgumentNormalizationMode::Exact)
            .map_err(|_| "sse_state_failed")?;
    let literal = &literals()[0];
    state.process_sse_block(&frame("response.output_text.delta",json!({"type":"response.output_text.delta","output_index":0,"content_index":0,"delta":literal}))).map_err(|_|"literal_delta_rejected")?;
    assert!(state.process_sse_block(&frame("response.output_text.done",json!({"type":"response.output_text.done","output_index":0,"content_index":0,"text":"synthetic-mismatch"}))).is_err(),"snapshot_mismatch_accepted");
    let (lease, binding, budget, _) = fixture()?;
    let mut state =
        SseRestorationState::new(lease, binding, budget, ArgumentNormalizationMode::Exact)
            .map_err(|_| "sse_state_failed")?;
    state.process_sse_block(&frame("response.output_text.delta",json!({"type":"response.output_text.delta","output_index":0,"content_index":0,"delta":literal}))).map_err(|_|"literal_delta_rejected")?;
    assert!(
        state.finalize_and_flush(true).is_err(),
        "missing_completion_accepted"
    );
    Ok(())
}

fn decoded_frames(frames: &[bytes::Bytes]) -> Result<Vec<Value>, Box<dyn std::error::Error>> {
    frames
        .iter()
        .map(|bytes| {
            let raw = std::str::from_utf8(bytes)?;
            let data = raw
                .lines()
                .find_map(|line| line.strip_prefix("data: "))
                .ok_or("frame_data_missing")?;
            Ok(serde_json::from_str(data)?)
        })
        .collect()
}
#[test]
fn placeholder_passthrough_response_sse_literals_delivered_once_at_every_terminal() -> TestResult {
    for refusal in [false, true] {
        for terminal in 0..4 {
            let literal = &literals()[0];
            let (lease, binding, budget, _) = fixture()?;
            let mut state =
                SseRestorationState::new(lease, binding, budget, ArgumentNormalizationMode::Exact)
                    .map_err(|_| "sse_state_failed")?;
            let mut frames = Vec::new();
            let delta_event = if refusal {
                "response.refusal.delta"
            } else {
                "response.output_text.delta"
            };
            frames.extend(state.process_sse_block(&frame(delta_event,json!({"type":delta_event,"output_index":0,"content_index":0,"delta":literal}))).map_err(|_|"literal_delta_rejected")?);
            let field = if refusal { "refusal" } else { "text" };
            let mut part = json!({"type":if refusal {"refusal"}else{"output_text"}});
            part[field] = json!(literal);
            let item = json!({"id":"synthetic-message","type":"message","role":"assistant","status":"completed","content":[part]});
            if terminal == 0 {
                let event = if refusal {
                    "response.refusal.done"
                } else {
                    "response.output_text.done"
                };
                let mut done = json!({"type":event,"output_index":0,"content_index":0});
                done[field] = json!(literal);
                frames.extend(
                    state
                        .process_sse_block(&frame(event, done))
                        .map_err(|_| "literal_done_rejected")?,
                );
            }
            if terminal <= 1 {
                frames.extend(state.process_sse_block(&frame("response.content_part.done",json!({"type":"response.content_part.done","output_index":0,"content_index":0,"part":part}))).map_err(|_|"literal_part_rejected")?);
            }
            if terminal <= 2 {
                frames.extend(state.process_sse_block(&frame("response.output_item.done",json!({"type":"response.output_item.done","output_index":0,"item":item}))).map_err(|_|"literal_item_rejected")?);
            }
            frames.extend(state.process_sse_block(&frame("response.completed",json!({"type":"response.completed","response":{"id":"synthetic-terminal","object":"response","status":"completed","output":[item]}}))).map_err(|_|"literal_completion_rejected")?);
            frames.extend(
                state
                    .finalize_and_flush(true)
                    .map_err(|_| "literal_flush_rejected")?,
            );
            let values = decoded_frames(&frames)?;
            let text = values
                .iter()
                .filter(|v| v["type"] == delta_event)
                .filter_map(|v| v["delta"].as_str())
                .collect::<String>();
            assert!(text == *literal, "literal_not_delivered_once");
            assert!(
                values
                    .iter()
                    .filter(|v| v["type"] == "response.completed")
                    .count()
                    == 1,
                "completion_duplicated"
            );
            if terminal != 0 {
                assert!(
                    !values
                        .iter()
                        .any(|v| v["type"] == "response.output_text.done"
                            || v["type"] == "response.refusal.done"),
                    "completion_event_forged"
                );
            }
        }
    }
    Ok(())
}

#[test]
fn placeholder_passthrough_response_sse_authorized_reference_across_tool_barrier() -> TestResult {
    for authorized in [false, true] {
        let (_, _, _, sample) = fixture()?;
        for split in 1..sample.len() {
            let (lease, binding, budget, reference) = fixture()?;
            let candidate = if authorized {
                reference.clone()
            } else {
                format!("rredact-custom-{}-ai", "f".repeat(20))
            };
            let mut state =
                SseRestorationState::new(lease, binding, budget, ArgumentNormalizationMode::Exact)
                    .map_err(|_| "sse_state_failed")?;
            let mut frames=state.process_sse_block(&frame("response.output_text.delta",json!({"type":"response.output_text.delta","output_index":0,"content_index":0,"delta":&candidate[..split]}))).map_err(|_|"literal_delta_rejected")?;
            let added = json!({"id":"synthetic-tool-item","type":"function_call","call_id":"synthetic-call","name":"synthetic-tool","arguments":"","status":"in_progress"});
            let barrier = state
                .process_sse_block(&frame(
                    "response.output_item.added",
                    json!({"type":"response.output_item.added","output_index":1,"item":added}),
                ))
                .map_err(|_| "tool_barrier_rejected")?;
            assert!(barrier.is_empty(), "tool_barrier_released_early");
            frames.extend(state.process_sse_block(&frame("response.output_text.delta",json!({"type":"response.output_text.delta","output_index":0,"content_index":0,"delta":&candidate[split..]}))).map_err(|_|"literal_delta_rejected")?);
            let raw = response(&candidate, "{}");
            // Identity fields in the final tool snapshot must match the staged barrier item.
            let mut raw = raw;
            raw["output"][1]["id"] = json!("synthetic-tool-item");
            frames.extend(
                state
                    .process_sse_block(&frame(
                        "response.completed",
                        json!({"type":"response.completed","response":raw}),
                    ))
                    .map_err(|_| "literal_completion_rejected")?,
            );
            frames.extend(
                state
                    .finalize_and_flush(true)
                    .map_err(|_| "literal_flush_rejected")?,
            );
            let values = decoded_frames(&frames)?;
            let text = values
                .iter()
                .filter(|v| v["type"] == "response.output_text.delta")
                .filter_map(|v| v["delta"].as_str())
                .collect::<String>();
            let expected = if authorized {
                "synthetic-restored"
            } else {
                &candidate
            };
            assert!(text == expected, "barrier_restore_contract_changed");
        }
    }
    Ok(())
}

#[test]
fn placeholder_passthrough_response_sse_zero_replacements_preserve_raw_arguments() -> TestResult {
    let (lease, binding, budget, _) = fixture()?;
    let literal = &literals()[2];
    let encoded = serde_json::to_string(&json!({"value":literal}))?;
    let delta_raw = format!("  {encoded}  ");
    let done_raw = encoded.replace("rredact", "\\u0072redact");
    let item_raw = format!("\n{encoded}\n");
    let final_raw = encoded.clone();
    let tool = |arguments: &str| json!({"id":"synthetic-raw-tool","type":"function_call","call_id":"synthetic-raw-call","name":"synthetic-tool","arguments":arguments,"status":"completed"});
    let events = vec![
        json!({"type":"response.output_item.added","output_index":0,"item":tool("")}),
        json!({"type":"response.function_call_arguments.delta","output_index":0,"delta":&delta_raw[..delta_raw.len()/2]}),
        json!({"type":"response.function_call_arguments.delta","output_index":0,"delta":&delta_raw[delta_raw.len()/2..]}),
        json!({"type":"response.function_call_arguments.done","output_index":0,"arguments":done_raw}),
        json!({"type":"response.output_item.done","output_index":0,"item":tool(&item_raw)}),
        json!({"type":"response.completed","response":{"id":"synthetic-raw-response","object":"response","status":"completed","output":[tool(&final_raw)]}}),
    ];
    let mut state =
        SseRestorationState::new(lease, binding, budget, ArgumentNormalizationMode::Exact)
            .map_err(|_| "raw_sse_state_failed")?;
    for event in &events {
        let kind = event["type"].as_str().ok_or("raw_event_missing")?;
        assert!(
            state
                .process_sse_block(&frame(kind, event.clone()))
                .map_err(|_| "raw_sse_event_failed")?
                .is_empty(),
            "raw_arguments_released_before_completion"
        );
    }
    let frames = state
        .finalize_and_flush(true)
        .map_err(|_| "raw_sse_flush_failed")?;
    let delivered = decoded_frames(&frames)?;
    assert!(
        delivered == events,
        "zero_replacement_argument_bytes_changed"
    );
    assert!(
        state.restored_occurrences() == 0,
        "literal_argument_restored"
    );
    Ok(())
}

fn assert_exact_tool_continuation(
    observer: &DlpRequestLease,
    binding: &super::completed_context::ActualAttemptBinding,
    response_id: &str,
    call_id: &str,
    arguments: &str,
) -> TestResult {
    let call = super::completed_context::CompletedCallSpec {
        call_id,
        item_type: "function_call",
        name: "synthetic-tool",
        payload: arguments,
        reasoning: None,
        encrypted_reasoning: None,
    };
    for previous in [None, Some(response_id)] {
        let mut resumed = observer.fork_attempt()?;
        assert!(
            resumed
                .resume_completed_context(binding, previous, &[call], &[])
                .is_ok(),
            "delivered_tool_snapshot_cannot_resume"
        );
    }
    let unobserved = format!("\n\n{arguments}\n\n");
    let mut resumed = observer.fork_attempt()?;
    assert!(
        resumed
            .resume_completed_context(
                binding,
                Some(response_id),
                &[super::completed_context::CompletedCallSpec {
                    payload: &unobserved,
                    ..call
                }],
                &[]
            )
            .is_err(),
        "unobserved_equivalent_snapshot_gained_authority"
    );
    let foreign = observer.bind_actual_attempt(
        "synthetic-other-provider",
        "openai_responses",
        "https://synthetic.invalid/v1/responses",
        Some("synthetic-principal"),
    )?;
    let mut resumed = observer.fork_attempt()?;
    assert!(
        resumed
            .resume_completed_context(&foreign, Some(response_id), &[call], &[])
            .is_err(),
        "delivered_snapshot_crossed_binding"
    );
    for changed in [
        super::completed_context::CompletedCallSpec {
            name: "synthetic-other-tool",
            ..call
        },
        super::completed_context::CompletedCallSpec {
            reasoning: Some("synthetic-unobserved"),
            ..call
        },
        super::completed_context::CompletedCallSpec {
            encrypted_reasoning: Some("synthetic-unobserved"),
            ..call
        },
    ] {
        let mut resumed = observer.fork_attempt()?;
        assert!(
            resumed
                .resume_completed_context(binding, Some(response_id), &[changed], &[])
                .is_err(),
            "delivered_snapshot_identity_changed"
        );
    }
    Ok(())
}

#[test]
fn delivered_tool_continuation_sse_exact_raw_carriers_resume() -> TestResult {
    let (lease, binding, budget, _) = fixture()?;
    let observer = lease.fork_attempt()?;
    let encoded = serde_json::to_string(&json!({"value": &literals()[2]}))?;
    let delta_raw = format!("  {encoded}  ");
    let done_raw = encoded.replace("rredact", "\\u0072redact");
    let item_raw = format!("\n{encoded}\n");
    let tool = |arguments: &str| json!({"id":"synthetic-delivered-tool","type":"function_call","call_id":"synthetic-delivered-call","name":"synthetic-tool","arguments":arguments,"status":"completed"});
    let events = [
        json!({"type":"response.output_item.added","output_index":0,"item":tool("")}),
        json!({"type":"response.function_call_arguments.delta","output_index":0,"delta":delta_raw}),
        json!({"type":"response.function_call_arguments.done","output_index":0,"arguments":done_raw}),
        json!({"type":"response.output_item.done","output_index":0,"item":tool(&item_raw)}),
        json!({"type":"response.completed","response":{"id":"synthetic-delivered-response","object":"response","status":"completed","output":[tool(&encoded)]}}),
    ];
    let mut state = SseRestorationState::new(
        lease,
        binding.clone(),
        budget,
        ArgumentNormalizationMode::Exact,
    )
    .map_err(|_| "delivered_state_failed")?;
    for event in &events {
        assert!(
            state
                .process_sse_block(&frame(
                    event["type"].as_str().ok_or("missing_type")?,
                    event.clone()
                ))
                .map_err(|_| "delivered_event_failed")?
                .is_empty(),
            "tool_released_before_completion"
        );
    }
    let frames = state
        .finalize_and_flush(true)
        .map_err(|_| "delivered_flush_failed")?;
    assert!(
        decoded_frames(&frames)? == events,
        "delivered_raw_carriers_changed"
    );
    for arguments in [&encoded, &delta_raw, &done_raw, &item_raw] {
        assert_exact_tool_continuation(
            &observer,
            &binding,
            "synthetic-delivered-response",
            "synthetic-delivered-call",
            arguments,
        )?;
    }
    Ok(())
}

#[test]
fn delivered_tool_continuation_json_restored_snapshot_resumes() -> TestResult {
    let (lease, binding, budget, reference) = fixture()?;
    let observer = lease.fork_attempt()?;
    let arguments = serde_json::to_string(&json!({"value":reference}))?;
    let raw = response("ordinary", &arguments);
    let witness =
        CompletionWitness::from_verified_completion(true).map_err(|_| "missing_witness")?;
    let restored =
        restore_and_publish_json_response(&raw, false, &lease, &binding, &witness, &budget)
            .map_err(|_| "delivered_json_failed")?;
    let delivered = restored.as_value()["output"][1]["arguments"]
        .as_str()
        .ok_or("missing_arguments")?;
    assert!(delivered != arguments, "synthetic_restore_not_exercised");
    for arguments in [&arguments, delivered] {
        assert_exact_tool_continuation(
            &observer,
            &binding,
            "synthetic-response",
            "synthetic-call",
            arguments,
        )?;
    }
    Ok(())
}

#[test]
fn delivered_tool_continuation_sse_restored_snapshot_resumes() -> TestResult {
    let (lease, binding, budget, reference) = fixture()?;
    let observer = lease.fork_attempt()?;
    let arguments = serde_json::to_string(&json!({"value":reference}))?;
    let raw = response("ordinary", &arguments);
    let tool = raw["output"][1].clone();
    let events = [
        json!({"type":"response.output_item.added","output_index":1,"item":tool}),
        json!({"type":"response.function_call_arguments.delta","output_index":1,"delta":arguments}),
        json!({"type":"response.function_call_arguments.done","output_index":1,"arguments":arguments}),
        json!({"type":"response.output_item.done","output_index":1,"item":tool}),
        json!({"type":"response.completed","response":raw}),
    ];
    let mut state = SseRestorationState::new(
        lease,
        binding.clone(),
        budget,
        ArgumentNormalizationMode::Exact,
    )
    .map_err(|_| "delivered_state_failed")?;
    for event in &events {
        assert!(
            state
                .process_sse_block(&frame(
                    event["type"].as_str().ok_or("missing_type")?,
                    event.clone()
                ))
                .map_err(|_| "delivered_event_failed")?
                .is_empty(),
            "tool_released_before_completion"
        );
    }
    let frames = state
        .finalize_and_flush(true)
        .map_err(|_| "delivered_flush_failed")?;
    let values = decoded_frames(&frames)?;
    let delivered = values
        .last()
        .and_then(|value| value.pointer("/response/output/1/arguments"))
        .and_then(Value::as_str)
        .ok_or("missing_arguments")?;
    assert!(delivered != arguments, "synthetic_restore_not_exercised");
    assert_exact_tool_continuation(
        &observer,
        &binding,
        "synthetic-response",
        "synthetic-call",
        delivered,
    )?;
    Ok(())
}

#[test]
fn delivered_tool_continuation_missing_completion_never_publishes() -> TestResult {
    let (lease, binding, budget, _) = fixture()?;
    let observer = lease.fork_attempt()?;
    let tool = json!({"id":"synthetic-incomplete-tool","type":"function_call","call_id":"synthetic-incomplete-call","name":"synthetic-tool","arguments":"{}","status":"completed"});
    let mut state = SseRestorationState::new(
        lease,
        binding.clone(),
        budget,
        ArgumentNormalizationMode::Exact,
    )
    .map_err(|_| "delivered_state_failed")?;
    state
        .process_sse_block(&frame(
            "response.output_item.done",
            json!({"type":"response.output_item.done","output_index":0,"item":tool}),
        ))
        .map_err(|_| "delivered_event_failed")?;
    assert!(
        state.finalize_and_flush(true).is_err(),
        "missing_completion_accepted"
    );
    let mut resumed = observer.fork_attempt()?;
    assert!(
        resumed
            .resume_completed_context(
                &binding,
                Some("synthetic-incomplete-response"),
                &[super::completed_context::CompletedCallSpec {
                    call_id: "synthetic-incomplete-call",
                    item_type: "function_call",
                    name: "synthetic-tool",
                    payload: "{}",
                    reasoning: None,
                    encrypted_reasoning: None
                }],
                &[]
            )
            .is_err(),
        "incomplete_tool_published_authority"
    );
    Ok(())
}

#[test]
fn delivered_tool_continuation_record_is_bounded_immutable_and_idempotent() -> TestResult {
    use super::completed_context::{CompletedCallPublications, CompletedCallSpec};
    let (lease, binding, _, _) = fixture()?;
    let witness = CompletionWitness::from_verified_completion(true)?;
    let original = CompletedCallSpec {
        call_id: "synthetic-record-call",
        item_type: "function_call",
        name: "synthetic-tool",
        payload: "{}",
        reasoning: None,
        encrypted_reasoning: None,
    };
    let texts = [" {}", "{} ", "\n{}", "{}\n"];
    let variants: Vec<_> = texts
        .iter()
        .map(|payload| CompletedCallSpec {
            payload,
            ..original
        })
        .collect();
    let publish =
        |response_id, originals: &[CompletedCallSpec<'_>], delivered: &[CompletedCallSpec<'_>]| {
            lease.publish_completed_context_variants(
                &binding,
                &witness,
                response_id,
                &[],
                CompletedCallPublications {
                    originals,
                    delivered,
                },
                &[],
            )
        };
    publish("synthetic-record-response", &[original], &variants)?;
    let mut reversed = variants.clone();
    reversed.reverse();
    publish("synthetic-record-response", &[original], &reversed)?;
    let changed = CompletedCallSpec {
        payload: "\t{}",
        ..original
    };
    assert!(
        publish("synthetic-record-response", &[original], &[changed]).is_err(),
        "record_variants_mutated"
    );
    assert!(
        publish("synthetic-record-response", &[changed], &variants).is_err(),
        "record_original_mutated"
    );
    for payload in texts {
        assert_exact_tool_continuation(
            &lease,
            &binding,
            "synthetic-record-response",
            original.call_id,
            payload,
        )?;
    }
    let mut excessive = variants.clone();
    excessive.push(changed);
    assert!(
        publish("synthetic-overflow-response", &[original], &excessive).is_err(),
        "record_limit_bypassed"
    );
    let mut resumed = lease.fork_attempt()?;
    assert!(
        resumed
            .resume_completed_context(
                &binding,
                Some("synthetic-overflow-response"),
                &[original],
                &[]
            )
            .is_err(),
        "overflow_partially_published"
    );
    Ok(())
}

#[test]
fn delivered_tool_continuation_budget_failure_is_atomic() -> TestResult {
    use super::{
        completed_context::{CompletedCallPublications, CompletedCallSpec},
        token_store_owners::AllocationCharge,
    };
    let store = DlpTokenStore::new();
    let lease = store.admit("codex", Some("synthetic-budget-context"), 1)?;
    let binding = lease.bind_actual_attempt(
        "synthetic-provider",
        "openai_responses",
        "https://synthetic.invalid/v1/responses",
        Some("synthetic-principal"),
    )?;
    let ledger = store.allocation_ledger_for_test()?;
    let before = ledger.used();
    let blocker = ledger.reserve(AllocationCharge {
        plaintext: 0,
        metadata: ledger.limits_for_test().metadata - before.metadata,
    })?;
    let original = CompletedCallSpec {
        call_id: "synthetic-budget-call",
        item_type: "function_call",
        name: "synthetic-tool",
        payload: "{}",
        reasoning: None,
        encrypted_reasoning: None,
    };
    let variant = CompletedCallSpec {
        payload: " {} ",
        ..original
    };
    let witness = CompletionWitness::from_verified_completion(true)?;
    let publish = || {
        lease.publish_completed_context_variants(
            &binding,
            &witness,
            "synthetic-budget-response",
            &[],
            CompletedCallPublications {
                originals: &[original],
                delivered: &[variant],
            },
            &[],
        )
    };
    assert!(publish().is_err(), "exhausted_budget_accepted");
    drop(blocker);
    assert!(
        ledger.used() == before,
        "failed_publication_leaked_ownership"
    );
    let mut resumed = lease.fork_attempt()?;
    assert!(
        resumed
            .resume_completed_context(&binding, Some("synthetic-budget-response"), &[variant], &[])
            .is_err(),
        "budget_failure_published_authority"
    );
    publish()?;
    assert_exact_tool_continuation(
        &lease,
        &binding,
        "synthetic-budget-response",
        original.call_id,
        variant.payload,
    )?;
    Ok(())
}

#[test]
fn delivered_tool_continuation_multiple_tools_remain_isolated() -> TestResult {
    let (lease, binding, budget, reference) = fixture()?;
    let arguments = serde_json::to_string(&json!({"value": reference}))?;
    let mut raw = response("ordinary", &arguments);
    raw["output"]
        .as_array_mut()
        .ok_or("missing_output")?
        .push(json!({
            "type":"custom_tool_call", "id":"synthetic-custom-item", "status":"completed",
            "call_id":"synthetic-custom-call", "name":"synthetic-custom-tool", "input":reference,
            "reasoning_content":"synthetic-reasoning", "encrypted_reasoning":"synthetic-opaque"
        }));
    let witness = CompletionWitness::from_verified_completion(true)?;
    let restored =
        restore_and_publish_json_response(&raw, false, &lease, &binding, &witness, &budget)
            .map_err(|_| "multiple_tools_failed")?;
    let output = restored.as_value()["output"]
        .as_array()
        .ok_or("missing_output")?;
    let function = super::completed_context::CompletedCallSpec {
        call_id: "synthetic-call",
        item_type: "function_call",
        name: "synthetic-tool",
        payload: output[1]["arguments"].as_str().ok_or("missing_arguments")?,
        reasoning: None,
        encrypted_reasoning: None,
    };
    let custom = super::completed_context::CompletedCallSpec {
        call_id: "synthetic-custom-call",
        item_type: "custom_tool_call",
        name: "synthetic-custom-tool",
        payload: output[2]["input"].as_str().ok_or("missing_input")?,
        reasoning: Some("synthetic-reasoning"),
        encrypted_reasoning: Some("synthetic-opaque"),
    };
    let mut resumed = lease.fork_attempt()?;
    assert!(
        resumed
            .resume_completed_context(
                &binding,
                Some("synthetic-response"),
                &[function, custom],
                &[]
            )
            .is_ok(),
        "multiple_delivered_tools_cannot_resume"
    );
    for changed in [
        super::completed_context::CompletedCallSpec {
            payload: custom.payload,
            ..function
        },
        super::completed_context::CompletedCallSpec {
            payload: function.payload,
            ..custom
        },
        super::completed_context::CompletedCallSpec {
            reasoning: None,
            ..custom
        },
    ] {
        let mut resumed = lease.fork_attempt()?;
        assert!(
            resumed
                .resume_completed_context(&binding, Some("synthetic-response"), &[changed], &[])
                .is_err(),
            "delivered_tool_identity_mixed"
        );
    }
    Ok(())
}

#[test]
fn delivered_tool_continuation_sse_never_synthesizes_item_reasoning() -> TestResult {
    for variant in 0..4 {
        let (lease, binding, budget, _) = fixture()?;
        let observer = lease.fork_attempt()?;
        let final_args = "{}";
        let item_args = " {} ";
        let final_item = json!({"id":"synthetic-fields-item", "type":"function_call", "call_id":"synthetic-fields-call", "name":"synthetic-tool", "status":"completed", "arguments":final_args, "reasoning_content":"synthetic-reasoning", "encrypted_reasoning":"synthetic-opaque"});
        let mut item = final_item.clone();
        item["arguments"] = json!(item_args);
        match variant {
            0 => {
                item.as_object_mut()
                    .ok_or("missing_item")?
                    .remove("reasoning_content");
            }
            1 => {
                item["reasoning_content"] = Value::Null;
            }
            2 => {
                item["encrypted_reasoning"] = json!("synthetic-other-opaque");
            }
            _ => {}
        }
        let mut state = SseRestorationState::new(
            lease,
            binding.clone(),
            budget,
            ArgumentNormalizationMode::Exact,
        )
        .map_err(|_| "fields_state_failed")?;
        state
            .process_sse_block(&frame(
                "response.output_item.done",
                json!({"type":"response.output_item.done", "output_index":0, "item":item}),
            ))
            .map_err(|_| "fields_item_failed")?;
        state.process_sse_block(&frame("response.completed", json!({"type":"response.completed", "response":{"id":"synthetic-fields-response", "object":"response", "status":"completed", "output":[final_item]}}))).map_err(|_| "fields_completion_failed")?;
        let frames = state
            .finalize_and_flush(true)
            .map_err(|_| "fields_flush_failed")?;
        let values = decoded_frames(&frames)?;
        assert!(values[0]["item"] == item, "fields_item_rewritten");
        let spec = super::completed_context::CompletedCallSpec {
            call_id: "synthetic-fields-call",
            item_type: "function_call",
            name: "synthetic-tool",
            payload: item_args,
            reasoning: Some("synthetic-reasoning"),
            encrypted_reasoning: Some("synthetic-opaque"),
        };
        let mut resumed = observer.fork_attempt()?;
        assert!(
            resumed
                .resume_completed_context(&binding, Some("synthetic-fields-response"), &[spec], &[])
                .is_ok()
                == (variant == 3),
            "undelivered_item_fields_authorized"
        );
        let mut resumed = observer.fork_attempt()?;
        assert!(
            resumed
                .resume_completed_context(
                    &binding,
                    Some("synthetic-fields-response"),
                    &[super::completed_context::CompletedCallSpec {
                        payload: final_args,
                        ..spec
                    }],
                    &[]
                )
                .is_ok(),
            "final_fields_lost_authority"
        );
    }
    Ok(())
}

#[test]
fn response_unsupported_carrier_rejects_current_lease_authorized_reference() -> TestResult {
    let (lease, _, budget, reference) = fixture()?;
    assert!(
        lease.resolve_if_authorized(&reference)?.is_some(),
        "synthetic_reference_not_authorized"
    );
    for candidate in [reference.clone(), format!("{} {reference}", literals()[1])] {
        for as_key in [false, true] {
            let mut raw = response("synthetic-public", "{}");
            let mut metadata = serde_json::Map::new();
            if as_key {
                metadata.insert(candidate.clone(), json!("synthetic-metadata"));
            } else {
                metadata.insert("literal".to_owned(), json!([candidate]));
            }
            raw["metadata"] = Value::Object(metadata);
            let result = stage_json_response(&raw, false, &lease, &budget);
            let rejected = matches!(
                &result,
                Err(RestoreError::FailedBecause(RestoreReason::TokenContext))
            );
            drop(result);
            assert!(rejected, "authorized_immutable_reference_not_refused");
            assert!(budget.used() == 0, "rejected_metadata_budget_retained");
        }
    }
    Ok(())
}

#[test]
fn response_unsupported_carrier_preserves_unknown_malformed_and_partial_literals() -> TestResult {
    let (lease, binding, budget, reference) = fixture()?;
    let (_, _, _, unknown) = fixture()?;
    assert!(
        lease.resolve_if_authorized(&unknown)?.is_none(),
        "synthetic_unknown_reference_authorized"
    );
    let mut candidates = literals();
    candidates.push(unknown);
    candidates.push(format!("rredact-custom-{}-ai", "g".repeat(20)));
    candidates.extend((1..reference.len()).map(|end| reference[..end].to_owned()));
    let witness = CompletionWitness::from_verified_completion(true)?;
    for candidate in candidates {
        for as_key in [false, true] {
            let mut raw = response("synthetic-public", "{}");
            let mut metadata = serde_json::Map::new();
            if as_key {
                metadata.insert(candidate.clone(), json!("synthetic-metadata"));
            } else {
                metadata.insert("literal".to_owned(), json!([candidate]));
            }
            raw["metadata"] = Value::Object(metadata);
            let restored =
                restore_and_publish_json_response(&raw, false, &lease, &binding, &witness, &budget)
                    .map_err(|_| "unmatched_immutable_literal_rejected")?;
            assert!(restored.as_value() == &raw, "immutable_literal_changed");
            assert!(
                restored.restored_occurrences() == 0,
                "immutable_literal_gained_authority"
            );
            drop(restored);
            assert!(budget.used() == 0, "literal_metadata_budget_retained");
        }
    }
    Ok(())
}
