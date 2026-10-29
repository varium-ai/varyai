//! Unit tests for post-consumer Codex JSON/Compact/SSE response restoration,
//! 30-byte token boundary retention, delta/snapshot consistency, and the
//! 8 MiB / 128-tool commit barrier.

use serde_json::{json, Value};

use super::{
    completed_context::CompletionWitness,
    payload_budget::PayloadBudget,
    response_restoration::{
        restore_and_publish_json_response, split_safe_prefix_and_tail, ArgumentNormalizationMode,
        RestoreError, SseRestorationState,
    },
    tokenization::{DlpTokenStore, TokenCategory, TokenFragment},
};

#[test]
fn split_safe_prefix_retains_up_to_30_byte_token_prefix_and_rejects_malformed() {
    let token = "vary_ph_0123456789abcdef0123_ai";
    assert_eq!(token.len(), 31);

    for split_len in 1..31 {
        let input = format!("hello {}", &token[..split_len]);
        let (safe, tail) = split_safe_prefix_and_tail(&input).expect("valid token prefix split");
        assert_eq!(safe, "hello ");
        assert_eq!(tail, &token[..split_len]);
    }

    let complete = format!("hello {token} world");
    let (safe, tail) = split_safe_prefix_and_tail(&complete).expect("complete token is safe");
    assert_eq!(safe, complete.as_str());
    assert_eq!(tail, "");

    let malformed = "hello vary_ph_0123456789abcdef012Z_ai";
    assert_eq!(
        split_safe_prefix_and_tail(malformed),
        Err(RestoreError::RestoreFailed)
    );
}

#[test]
fn restore_json_and_compact_responses_publish_context_and_reject_unsupported_carriers() {
    let store = DlpTokenStore::new();
    let budget = PayloadBudget::with_limit(4 * 1024 * 1024);
    let mut lease = store
        .admit("codex", Some("01961e3f-7f8a-7b00-8000-000000000101"), 1)
        .expect("admit lease");
    lease
        .prepare_with_budget(
            &[TokenFragment {
                raw: "13800138000",
                whole_span_categories: &[TokenCategory::Phone],
            }],
            &[],
            budget.clone(),
            |_| Ok(()),
        )
        .expect("prepare lease");
    let phone_tok = lease.authorized_tokens().expect("tokens").as_slice()[0].to_owned();
    let binding = lease
        .bind_actual_attempt(
            "prov-1",
            "openai_responses",
            "http://127.0.0.1:19999/v1/responses",
            Some("acct-1"),
        )
        .expect("bind attempt");
    let witness = CompletionWitness::from_verified_completion(true).expect("witness");

    let response = json!({
        "id": "resp_json_1",
        "object": "response",
        "status": "completed",
        "output": [
            {
                "id": "msg_1",
                "type": "message",
                "role": "assistant",
                "status": "completed",
                "content": [
                    {
                        "type": "output_text",
                        "text": format!("Dial {phone_tok}"),
                        "annotations": []
                    }
                ]
            },
            {
                "id": "fc_1",
                "type": "function_call",
                "call_id": "call_1",
                "name": "lookup_user",
                "arguments": format!("{{\"phone\":\"{phone_tok}\"}}"),
                "status": "completed"
            }
        ]
    });

    let count =
        restore_and_publish_json_response(&response, false, &lease, &binding, &witness, &budget)
            .expect("restore json response");
    assert_eq!(count.restored_occurrences(), 2);
    let response = count.as_value();
    assert_eq!(
        response["output"][0]["content"][0]["text"],
        Value::String("Dial 13800138000".to_string())
    );
    assert_eq!(
        response["output"][1]["arguments"],
        Value::String("{\"phone\":\"13800138000\"}".to_string())
    );

    // Unsupported carrier: token in function_call `name` must return RestoreUnsupported.
    let unsupported = json!({
        "id": "resp_json_bad",
        "object": "response",
        "status": "completed",
        "output": [
            {
                "id": "fc_bad",
                "type": "function_call",
                "call_id": "call_bad",
                "name": phone_tok,
                "arguments": "{}",
                "status": "completed"
            }
        ]
    });
    assert_eq!(
        restore_and_publish_json_response(
            &unsupported,
            false,
            &lease,
            &binding,
            &witness,
            &budget,
        )
        .err(),
        Some(RestoreError::RestoreUnsupported)
    );

    // Compact response: message + reasoning + compaction succeeds, but token in compaction encrypted_content is rejected.
    let compact = json!({
        "id": "resp_compact_1",
        "object": "response.compaction",
        "output": [
            {
                "id": "msg_c1",
                "type": "message",
                "role": "assistant",
                "content": [
                    {
                        "type": "output_text",
                        "text": format!("Contact {phone_tok}")
                    }
                ]
            },
            {
                "id": "comp_1",
                "type": "compaction",
                "encrypted_content": "opaque_blob_v1"
            }
        ]
    });
    let compact_count =
        restore_and_publish_json_response(&compact, true, &lease, &binding, &witness, &budget)
            .expect("restore compact response");
    assert_eq!(compact_count.restored_occurrences(), 1);
    let compact = compact_count.as_value();
    assert_eq!(
        compact["output"][0]["content"][0]["text"],
        Value::String("Contact 13800138000".to_string())
    );
}

#[test]
fn sse_restoration_streams_split_tokens_and_buffers_tool_commit_barrier() {
    let store = DlpTokenStore::new();
    let budget = PayloadBudget::with_limit(4 * 1024 * 1024);
    let mut lease = store
        .admit("codex", Some("01961e3f-7f8a-7b00-8000-000000000102"), 1)
        .expect("admit lease");
    lease
        .prepare_with_budget(
            &[TokenFragment {
                raw: "13800138000",
                whole_span_categories: &[TokenCategory::Phone],
            }],
            &[],
            budget.clone(),
            |_| Ok(()),
        )
        .expect("prepare lease");
    let phone_tok = lease.authorized_tokens().expect("tokens").as_slice()[0].to_owned();
    let binding = lease
        .bind_actual_attempt(
            "prov-1",
            "openai_responses",
            "http://127.0.0.1:19999/v1/responses",
            Some("acct-1"),
        )
        .expect("bind attempt");

    let mut state = SseRestorationState::new(
        lease.fork_attempt().expect("fork"),
        binding,
        budget,
        ArgumentNormalizationMode::ChatCanonical,
    )
    .expect("new sse state");

    // 1. Pre-barrier split token across two output_text.delta frames.
    let part1 = &phone_tok[..12];
    let part2 = &phone_tok[12..];
    let delta1 = format!(
        "event: response.output_text.delta\ndata: {}\n\n",
        json!({
            "type": "response.output_text.delta",
            "output_index": 0,
            "content_index": 0,
            "delta": format!("Tel: {part1}")
        })
    );
    let frames1 = state.process_sse_block(&delta1).expect("delta1");
    assert_eq!(frames1.len(), 1);
    let f1_str = std::str::from_utf8(&frames1[0]).expect("utf8");
    assert!(f1_str.contains("\"delta\":\"Tel: \""));

    let delta2 = format!(
        "event: response.output_text.delta\ndata: {}\n\n",
        json!({
            "type": "response.output_text.delta",
            "output_index": 0,
            "content_index": 0,
            "delta": format!("{part2}!")
        })
    );
    let frames2 = state.process_sse_block(&delta2).expect("delta2");
    assert_eq!(frames2.len(), 1);
    let f2_str = std::str::from_utf8(&frames2[0]).expect("utf8");
    assert!(f2_str.contains("\"delta\":\"13800138000!\""));

    let text_done = format!(
        "event: response.output_text.done\ndata: {}\n\n",
        json!({
            "type": "response.output_text.done",
            "output_index": 0,
            "content_index": 0,
            "text": format!("Tel: {phone_tok}!")
        })
    );
    let frames_done = state.process_sse_block(&text_done).expect("text_done");
    assert_eq!(frames_done.len(), 1);

    // 2. Tool call enters the commit barrier; no tool frames are released before finalize_and_flush.
    let tool_added = format!(
        "event: response.output_item.added\ndata: {}\n\n",
        json!({
            "type": "response.output_item.added",
            "output_index": 1,
            "item": {
                "id": "fc_1",
                "type": "function_call",
                "call_id": "call_1",
                "name": "dial_number",
                "arguments": "",
                "status": "in_progress"
            }
        })
    );
    assert!(state
        .process_sse_block(&tool_added)
        .expect("tool_added")
        .is_empty());

    // Raw deltas have whitespace while final arguments are canonical JSON (ChatCanonical mode).
    let tool_delta1 = format!(
        "event: response.function_call_arguments.delta\ndata: {}\n\n",
        json!({
            "type": "response.function_call_arguments.delta",
            "output_index": 1,
            "delta": format!("{{ \"phone\": \"{}", &phone_tok[..15])
        })
    );
    assert!(state
        .process_sse_block(&tool_delta1)
        .expect("tool_delta1")
        .is_empty());

    let tool_delta2 = format!(
        "event: response.function_call_arguments.delta\ndata: {}\n\n",
        json!({
            "type": "response.function_call_arguments.delta",
            "output_index": 1,
            "delta": format!("{}\" }}", &phone_tok[15..])
        })
    );
    assert!(state
        .process_sse_block(&tool_delta2)
        .expect("tool_delta2")
        .is_empty());

    let canonical_raw_args = format!("{{\"phone\":\"{phone_tok}\"}}");
    let tool_args_done = format!(
        "event: response.function_call_arguments.done\ndata: {}\n\n",
        json!({
            "type": "response.function_call_arguments.done",
            "output_index": 1,
            "arguments": canonical_raw_args
        })
    );
    assert!(state
        .process_sse_block(&tool_args_done)
        .expect("tool_args_done")
        .is_empty());

    let completed = format!(
        "event: response.completed\ndata: {}\n\n",
        json!({
            "type": "response.completed",
            "response": {
                "id": "resp_sse_1",
                "object": "response",
                "status": "completed",
                "output": [
                    {
                        "id": "msg_1",
                        "type": "message",
                        "role": "assistant",
                        "status": "completed",
                        "content": [
                            {
                                "type": "output_text",
                                "text": format!("Tel: {phone_tok}!"),
                                "annotations": []
                            }
                        ]
                    },
                    {
                        "id": "fc_1",
                        "type": "function_call",
                        "call_id": "call_1",
                        "name": "dial_number",
                        "arguments": canonical_raw_args,
                        "status": "completed"
                    }
                ]
            }
        })
    );
    assert!(state
        .process_sse_block(&completed)
        .expect("completed")
        .is_empty());

    let flushed = state.finalize_and_flush(true).expect("finalize and flush");
    assert_eq!(flushed.len(), 5);
    let joined = flushed
        .iter()
        .map(|b| std::str::from_utf8(b).expect("utf8"))
        .collect::<Vec<_>>()
        .join("");
    assert!(joined.contains("\"delta\":\"{\\\"phone\\\":\\\"13800138000\\\"}\""));
    assert!(joined.contains("\"delta\":\"\""));
    assert!(!joined.contains(&phone_tok));
}

#[test]
fn sse_restoration_rejects_unmatched_pre_barrier_text_part_and_exceeding_128_tool_barrier() {
    let store = DlpTokenStore::new();
    let budget = PayloadBudget::with_limit(4 * 1024 * 1024);
    let mut lease = store
        .admit("codex", Some("01961e3f-7f8a-7b00-8000-000000000103"), 1)
        .expect("admit lease");
    lease
        .prepare_with_budget(
            &[TokenFragment {
                raw: "13800138000",
                whole_span_categories: &[TokenCategory::Phone],
            }],
            &[],
            budget.clone(),
            |_| Ok(()),
        )
        .expect("prepare lease");
    let phone_tok = lease.authorized_tokens().expect("tokens").as_slice()[0].to_owned();
    let binding = lease
        .bind_actual_attempt(
            "prov-1",
            "openai_responses",
            "http://127.0.0.1:19999/v1/responses",
            Some("acct-1"),
        )
        .expect("bind attempt");

    let mut state = SseRestorationState::new(
        lease.fork_attempt().expect("fork"),
        binding.clone(),
        budget.clone(),
        ArgumentNormalizationMode::Exact,
    )
    .expect("new sse state");

    // Stream a pre-barrier delta at output_index = 1, then send response.completed with only output_index = 0.
    let orphan_delta = format!(
        "event: response.output_text.delta\ndata: {}\n\n",
        json!({
            "type": "response.output_text.delta",
            "output_index": 1,
            "content_index": 0,
            "delta": format!("Orphan {phone_tok}")
        })
    );
    assert_eq!(
        state
            .process_sse_block(&orphan_delta)
            .expect("orphan delta")
            .len(),
        1
    );

    let completed = format!(
        "event: response.completed\ndata: {}\n\n",
        json!({
            "type": "response.completed",
            "response": {
                "id": "resp_orphan",
                "object": "response",
                "status": "completed",
                "output": [
                    {
                        "id": "msg_0",
                        "type": "message",
                        "role": "assistant",
                        "status": "completed",
                        "content": [
                            {
                                "type": "output_text",
                                "text": "Only index 0",
                                "annotations": []
                            }
                        ]
                    }
                ]
            }
        })
    );
    assert!(state
        .process_sse_block(&completed)
        .expect("completed")
        .is_empty());
    assert_eq!(
        state.finalize_and_flush(true),
        Err(RestoreError::RestoreFailed)
    );

    // Also verify 128-tool barrier cap rejects the 129th distinct tool output_index.
    let mut barrier_state = SseRestorationState::new(
        lease.fork_attempt().expect("fork"),
        binding,
        budget,
        ArgumentNormalizationMode::XaiWholeFloat,
    )
    .expect("barrier state");
    for idx in 0..128u64 {
        let added = format!(
            "event: response.output_item.added\ndata: {}\n\n",
            json!({
                "type": "response.output_item.added",
                "output_index": idx,
                "item": {
                    "id": format!("fc_{idx}"),
                    "type": "function_call",
                    "call_id": format!("call_{idx}"),
                    "name": "tool_fn",
                    "arguments": "",
                    "status": "in_progress"
                }
            })
        );
        assert!(barrier_state
            .process_sse_block(&added)
            .expect("tool <= 128")
            .is_empty());
    }
    let overflow_added = format!(
        "event: response.output_item.added\ndata: {}\n\n",
        json!({
            "type": "response.output_item.added",
            "output_index": 128u64,
            "item": {
                "id": "fc_128",
                "type": "function_call",
                "call_id": "call_128",
                "name": "tool_fn",
                "arguments": "",
                "status": "in_progress"
            }
        })
    );
    assert_eq!(
        barrier_state.process_sse_block(&overflow_added),
        Err(RestoreError::RestoreFailed)
    );
}

#[test]
fn private_sse_native_completion_rejects_noncomplete_status_and_embedded_errors() {
    for response in [
        json!({"id":"resp_fixture","status":"error","output":[]}),
        json!({"id":"resp_fixture","status":"incomplete","output":[]}),
        json!({"id":"resp_fixture","output":[]}),
        json!({"id":"resp_fixture","status":"completed","error":{"code":"fixture_error"},"output":[]}),
        json!({"id":"resp_fixture","status":"completed","incomplete_details":{"reason":"max_output_tokens"},"output":[]}),
    ] {
        let store = DlpTokenStore::new();
        let lease = store
            .admit("codex", Some("fixture-session"), 1)
            .expect("lease");
        let binding = lease
            .bind_actual_attempt(
                "fixture-provider",
                "openai_responses",
                "http://127.0.0.1:1/responses",
                Some("fixture-principal"),
            )
            .expect("binding");
        let mut state = SseRestorationState::new(
            lease,
            binding,
            PayloadBudget::with_limit(1024 * 1024),
            ArgumentNormalizationMode::Exact,
        )
        .expect("restorer");
        let block = format!(
            "event: response.completed\ndata: {}\n\n",
            json!({"type":"response.completed","response":response})
        );
        assert!(
            state.process_sse_block(&block).is_err(),
            "invalid native completion must fail before replay"
        );
        assert!(
            state.finalize_and_flush(true).is_err(),
            "completion failure remains sticky"
        );
    }
}

#[test]
fn private_request_local_lease_restores_without_publishing_continuation() {
    for (session, principal) in [
        (None, Some("fixture-account")),
        (Some("fixture-session"), None),
    ] {
        let store = DlpTokenStore::new();
        let budget = PayloadBudget::with_limit(1024 * 1024);
        let mut lease = store.admit("codex", session, 1).expect("lease");
        lease
            .prepare_with_budget(
                &[TokenFragment {
                    raw: "fixture-local-secret",
                    whole_span_categories: &[TokenCategory::Custom],
                }],
                &[],
                budget.clone(),
                |_| Ok(()),
            )
            .expect("mapping");
        let token = lease.authorized_tokens().expect("tokens").as_slice()[0].to_owned();
        let binding = lease
            .bind_actual_attempt(
                "fixture-provider",
                "openai_responses",
                "http://127.0.0.1:1/responses",
                principal,
            )
            .expect("binding");
        let mut attempt = lease.fork_attempt().expect("attempt");
        assert!(
            attempt
                .resume_completed_context(&binding, None, &[], &[])
                .is_ok(),
            "no history needs no reusable principal"
        );
        let witness = CompletionWitness::from_verified_completion(true).expect("completion");
        let response = json!({"id":"resp_local","status":"completed","output":[{"id":"msg_local","type":"message","role":"assistant","content":[{"type":"output_text","text":token}]}]});
        let restored = restore_and_publish_json_response(
            &response, false, &attempt, &binding, &witness, &budget,
        )
        .expect("own request restoration does not require reusable context");
        let response = restored.as_value();
        assert!(response["output"][0]["content"][0]["text"] == "fixture-local-secret");
        assert!(
            attempt
                .publish_completed_context(&binding, &witness, "resp_local", &[], &[], &[])
                .is_err(),
            "request-only response cannot publish context"
        );
        assert!(
            attempt
                .resume_completed_context(&binding, Some("resp_local"), &[], &[])
                .is_err(),
            "request-only cursor cannot continue"
        );
        let mut next = store.admit("codex", session, 1).expect("next lease");
        let next_binding = next
            .bind_actual_attempt(
                "fixture-provider",
                "openai_responses",
                "http://127.0.0.1:1/responses",
                Some("fixture-account"),
            )
            .expect("next binding");
        assert!(
            next.resume_completed_context(&next_binding, Some("resp_local"), &[], &[])
                .is_err(),
            "no reusable cursor was recorded"
        );
        assert!(
            next.resume_completed_tokens(&next_binding, &[&token])
                .is_err(),
            "next request cannot claim request-only token"
        );
        let mut sse =
            SseRestorationState::new(attempt, binding, budget, ArgumentNormalizationMode::Exact)
                .expect("sse");
        let block = format!(
            "event: response.completed\ndata: {}\n\n",
            json!({"type":"response.completed","response":{"id":"resp_local_sse","status":"completed","output":[{"id":"msg_local","type":"message","role":"assistant","content":[{"type":"output_text","text":token}]}]}})
        );
        assert!(sse
            .process_sse_block(&block)
            .expect("buffer complete")
            .is_empty());
        assert_eq!(
            sse.finalize_and_flush(true)
                .expect("request-only SSE restore")
                .len(),
            1
        );
    }
}

mod barrier_regressions {
    use super::super::{
        completed_context::ActualAttemptBinding, payload_json::PayloadJson,
        tokenization::DlpRequestLease,
    };
    use super::*;

    fn fixture(
        budget: &PayloadBudget,
    ) -> (DlpTokenStore, DlpRequestLease, ActualAttemptBinding, String) {
        let store = DlpTokenStore::new();
        let mut lease = store
            .admit("codex", Some("fixture-barrier-session"), 1)
            .unwrap();
        lease
            .prepare_with_budget(
                &[TokenFragment {
                    raw: "fixture-barrier-secret",
                    whole_span_categories: &[TokenCategory::Custom],
                }],
                &[],
                budget.clone(),
                |_| Ok(()),
            )
            .unwrap();
        let token = lease.authorized_tokens().unwrap().as_slice()[0].to_owned();
        let binding = lease
            .bind_actual_attempt(
                "fixture-provider",
                "openai_responses",
                "http://127.0.0.1:1/responses",
                Some("fixture-account"),
            )
            .unwrap();
        (store, lease, binding, token)
    }

    fn frame(value: &Value) -> String {
        format!(
            "event: {}\ndata: {}\n\n",
            value["type"].as_str().unwrap(),
            value
        )
    }

    fn tool_item(arguments: &str) -> Value {
        json!({"id":"fc_fixture","type":"function_call","call_id":"call_fixture","name":"fixture_tool","namespace":"fixture_ns","arguments":arguments,"status":"completed"})
    }

    fn completed(item: Value) -> Value {
        json!({"type":"response.completed","response":{"id":"resp_barrier_fixture","object":"response","status":"completed","output":[item]}})
    }

    fn added() -> Value {
        let mut item = tool_item("");
        item["status"] = json!("in_progress");
        json!({"type":"response.output_item.added","output_index":0,"item":item})
    }

    fn cursor_exists(lease: &DlpRequestLease, binding: &ActualAttemptBinding) -> bool {
        lease
            .fork_attempt()
            .unwrap()
            .resume_completed_context(binding, Some("resp_barrier_fixture"), &[], &[])
            .is_ok()
    }

    #[test]
    fn private_barrier_retained_json_stays_charged_until_drop() {
        let budget = PayloadBudget::with_limit(4 * 1024 * 1024);
        let (_store, lease, binding, _) = fixture(&budget);
        let mut state = SseRestorationState::new(
            lease,
            binding,
            budget.clone(),
            ArgumentNormalizationMode::Exact,
        )
        .unwrap();
        let mut event = added();
        event["fixture_padding"] = json!("x".repeat(128 * 1024));
        let raw = serde_json::to_string(&event).unwrap();
        let before = budget.used();
        let parsed = PayloadJson::try_from_str(&raw, budget.clone(), budget.limit()).unwrap();
        let retained_json_charge = budget.used() - before;
        drop(parsed);
        assert!(state.process_sse_block(&frame(&event)).unwrap().is_empty());
        assert!(
            budget.used() >= before + retained_json_charge,
            "barrier must retain the parsed JSON charge, including unknown token-free fields"
        );
        drop(state);
        assert_eq!(budget.used(), 0);
    }

    #[test]
    fn private_barrier_replay_failure_never_publishes_cursor() {
        let budget = PayloadBudget::with_limit(4 * 1024 * 1024);
        let (_store, lease, binding, token) = fixture(&budget);
        let mut state = SseRestorationState::new(
            lease.fork_attempt().unwrap(),
            binding.clone(),
            budget.clone(),
            ArgumentNormalizationMode::Exact,
        )
        .unwrap();
        let mut event = added();
        event["unsupported_fixture_field"] = json!(token);
        // Reject the original unsupported carrier before it can be replayed or overwritten.
        assert_eq!(
            state.process_sse_block(&frame(&event)),
            Err(RestoreError::RestoreUnsupported)
        );
        assert!(
            !cursor_exists(&lease, &binding),
            "failed replay must not register a reusable cursor"
        );
    }

    #[test]
    fn private_barrier_replay_serialization_capacity_failure_never_releases_tools_or_cursor() {
        use super::super::payload_json::test_support::{self, Action, Phase};
        use std::{
            cell::{Cell, RefCell},
            rc::Rc,
        };

        const PADDING_BYTES: usize = 128 * 1024;
        for exhaust_replay in [false, true] {
            let budget = PayloadBudget::with_limit(4 * 1024 * 1024);
            let (_store, lease, binding, token) = fixture(&budget);
            let mut state = SseRestorationState::new(
                lease.fork_attempt().expect("attempt"),
                binding.clone(),
                budget.clone(),
                ArgumentNormalizationMode::Exact,
            )
            .expect("state");
            let mut event = added();
            event["fixture_padding"] = json!("x".repeat(PADDING_BYTES));
            let arguments = format!("{{\"value\":\"{token}\"}}");
            for event in [event, completed(tool_item(&arguments))] {
                assert!(state
                    .process_sse_block(&frame(&event))
                    .expect("buffer tool barrier")
                    .is_empty());
            }
            assert!(!cursor_exists(&lease, &binding));

            let replay_built = Rc::new(Cell::new(false));
            let pressure = Rc::new(RefCell::new(None));
            let observed_replay = Rc::clone(&replay_built);
            let held_pressure = Rc::clone(&pressure);
            let replay_budget = budget.clone();
            // Only the added-event replay owns this padding. Exhaust real capacity
            // after its JSON rebuild, so validation succeeds but serialization cannot.
            let hook = test_support::install(move |observation| {
                if observation.phase == Phase::AfterBuild && observation.charged >= PADDING_BYTES {
                    observed_replay.set(true);
                    if exhaust_replay {
                        *held_pressure.borrow_mut() = Some(
                            replay_budget
                                .reserve(replay_budget.limit() - replay_budget.used())
                                .expect("reserve remaining replay capacity"),
                        );
                        assert_eq!(replay_budget.used(), replay_budget.limit());
                    }
                }
                Action::Continue
            });
            let result = state.finalize_and_flush(true);
            drop(hook);
            assert!(
                replay_built.get(),
                "capacity denial must occur after a replay snapshot was rebuilt"
            );
            if exhaust_replay {
                assert_eq!(
                    result,
                    Err(RestoreError::RestoreFailed),
                    "no partial tool replay may escape"
                );
                assert!(
                    !cursor_exists(&lease, &binding),
                    "serialization failure must not publish continuity"
                );
                drop(pressure.borrow_mut().take());
                assert_eq!(
                    state.finalize_and_flush(true),
                    Err(RestoreError::RestoreFailed),
                    "capacity recovery cannot release a failed barrier"
                );
                assert!(!cursor_exists(&lease, &binding));
            } else {
                let frames = result.expect("same valid barrier succeeds without capacity pressure");
                assert_eq!(frames.len(), 2);
                assert!(cursor_exists(&lease, &binding));
            }
            drop(state);
            assert_eq!(
                budget.used(),
                0,
                "all replay and barrier owners release their charges"
            );
        }
    }

    #[test]
    fn private_barrier_replay_bytes_hold_charge_through_last_clone() {
        let budget = PayloadBudget::with_limit(4 * 1024 * 1024);
        let (_store, lease, binding, token) = fixture(&budget);
        let mut state = SseRestorationState::new(
            lease.fork_attempt().unwrap(),
            binding.clone(),
            budget.clone(),
            ArgumentNormalizationMode::Exact,
        )
        .unwrap();
        assert!(state
            .process_sse_block(&frame(&added()))
            .unwrap()
            .is_empty());
        let args = format!("{{\"value\":\"{token}\"}}");
        assert!(state
            .process_sse_block(&frame(&completed(tool_item(&args))))
            .unwrap()
            .is_empty());
        assert!(!cursor_exists(&lease, &binding));
        let frames = state.finalize_and_flush(true).unwrap();
        assert!(cursor_exists(&lease, &binding));
        let retained = frames.last().unwrap().clone();
        let frame_bytes = frames.iter().map(bytes::Bytes::len).sum::<usize>();
        drop(state);
        assert!(
            budget.used() >= frame_bytes,
            "returned replay bytes remain budget-owned after state drop"
        );
        drop(frames);
        assert!(
            budget.used() >= retained.len(),
            "clones retain the same charged allocation"
        );
        drop(retained);
        assert_eq!(budget.used(), 0);
    }
    #[test]
    fn private_barrier_identity_mismatch_denies_all_tool_release_and_cursor() {
        let mut admitted = Vec::new();
        for custom in [false, true] {
            for phase in ["added-done", "done-completed", "delta", "argument-done"] {
                for key in ["id", "call_id", "name", "type", "namespace"] {
                    if matches!(phase, "delta" | "argument-done") && key != "id" {
                        continue;
                    }
                    let budget = PayloadBudget::with_limit(4 * 1024 * 1024);
                    let (_store, lease, binding, _) = fixture(&budget);
                    let mut state = SseRestorationState::new(
                        lease.fork_attempt().unwrap(),
                        binding.clone(),
                        budget,
                        ArgumentNormalizationMode::Exact,
                    )
                    .unwrap();
                    let mut start = added();
                    let mut final_item = tool_item("{}");
                    if custom {
                        start["item"]["type"] = json!("custom_tool_call");
                        start["item"].as_object_mut().unwrap().remove("arguments");
                        start["item"]["input"] = json!("");
                        final_item["type"] = json!("custom_tool_call");
                        final_item.as_object_mut().unwrap().remove("arguments");
                        final_item["input"] = json!("{}");
                    }
                    let mut done = json!({"type":"response.output_item.done","output_index":0,"item":final_item.clone()});
                    let mut events = vec![start];
                    match phase {
                        "added-done" => done["item"][key] = json!("fixture_other"),
                        "done-completed" => final_item[key] = json!("fixture_other"),
                        _ => {
                            let stem = if custom {
                                "custom_tool_call_input"
                            } else {
                                "function_call_arguments"
                            };
                            let mut event = json!({"type":format!("response.{stem}.{}", if phase == "delta" { "delta" } else { "done" }),"output_index":0,"item_id":"fc_other"});
                            event[if phase == "delta" {
                                "delta"
                            } else if custom {
                                "input"
                            } else {
                                "arguments"
                            }] = json!("{}");
                            events.push(event);
                        }
                    }
                    events.push(done);
                    events.push(completed(final_item));
                    let mut denied = false;
                    for event in events {
                        match state.process_sse_block(&frame(&event)) {
                            Ok(frames) => {
                                assert!(frames.is_empty(), "no partial tool frames may be released")
                            }
                            Err(_) => {
                                denied = true;
                                break;
                            }
                        }
                    }
                    if !denied {
                        denied = state.finalize_and_flush(true).is_err();
                    }
                    if !denied || cursor_exists(&lease, &binding) {
                        admitted.push((custom, phase, key));
                    }
                }
            }
        }
        assert!(
            admitted.is_empty(),
            "identity inconsistency admitted: {admitted:?}"
        );
    }

    #[test]
    fn private_barrier_noncomplete_tool_item_status_never_replays_or_publishes_cursor() {
        let mut admitted = Vec::new();
        for custom in [false, true] {
            for status in ["incomplete", "in_progress", "failed"] {
                for phase in ["done", "completed", "both"] {
                    let budget = PayloadBudget::with_limit(4 * 1024 * 1024);
                    let (_store, lease, binding, token) = fixture(&budget);
                    let mut state = SseRestorationState::new(
                        lease.fork_attempt().expect("attempt"),
                        binding.clone(),
                        budget,
                        ArgumentNormalizationMode::Exact,
                    )
                    .expect("state");
                    let mut start = added();
                    let mut item = tool_item(&format!("{{\"value\":\"{token}\"}}"));
                    if custom {
                        for item in [&mut start["item"], &mut item] {
                            item["type"] = json!("custom_tool_call");
                            item["input"] = item
                                .as_object_mut()
                                .expect("tool object")
                                .remove("arguments")
                                .expect("arguments");
                        }
                    }
                    let mut done =
                        json!({"type":"response.output_item.done","output_index":0,"item":item});
                    let mut terminal = completed(item);
                    if phase != "completed" {
                        done["item"]["status"] = json!(status);
                    }
                    if phase != "done" {
                        terminal["response"]["output"][0]["status"] = json!(status);
                    }
                    let mut denied = false;
                    for event in [start, done, terminal] {
                        match state.process_sse_block(&frame(&event)) {
                            Ok(frames) => assert!(
                                frames.is_empty(),
                                "unverified tool items stay behind the barrier"
                            ),
                            Err(RestoreError::RestoreFailed) => {
                                denied = true;
                                break;
                            }
                            Err(_) => panic!("noncomplete tool status must fail restoration"),
                        }
                    }
                    if !denied {
                        denied = state.finalize_and_flush(true) == Err(RestoreError::RestoreFailed);
                    }
                    let cursor_published = cursor_exists(&lease, &binding);
                    if !denied || cursor_published {
                        admitted.push((custom, status, phase, cursor_published));
                    }
                }
            }
        }
        assert!(
            admitted.is_empty(),
            "noncomplete tool items replayed or published continuity: {admitted:?}"
        );
    }

    #[test]
    fn private_barrier_native_null_completion_fields_are_successful() {
        let budget = PayloadBudget::with_limit(4 * 1024 * 1024);
        let (_store, lease, binding, _) = fixture(&budget);
        let mut state =
            SseRestorationState::new(lease, binding, budget, ArgumentNormalizationMode::Exact)
                .unwrap();
        assert!(state
            .process_sse_block(&frame(&added()))
            .unwrap()
            .is_empty());
        let mut event = completed(tool_item("{}"));
        event["error"] = Value::Null;
        event["response"]["error"] = Value::Null;
        event["response"]["incomplete_details"] = Value::Null;
        assert!(
            state.process_sse_block(&frame(&event)).is_ok(),
            "null is absence of native error evidence"
        );
        assert_eq!(state.finalize_and_flush(true).unwrap().len(), 2);
    }

    #[test]
    fn private_barrier_short_ordinary_prefix_flushes_but_strict_header_fails() {
        let mut rejected = Vec::new();
        for suffix in [
            "v",
            "va",
            "var",
            "vary",
            "vary_",
            "vary_p",
            "vary_ph",
            "vary_zz",
            "vary_ph_",
            "vary_ph_0",
            "vary_ph_Z",
        ] {
            let strict = suffix.starts_with("vary_ph_");
            let budget = PayloadBudget::with_limit(4 * 1024 * 1024);
            let (_store, lease, binding, _) = fixture(&budget);
            let mut state =
                SseRestorationState::new(lease, binding, budget, ArgumentNormalizationMode::Exact)
                    .unwrap();
            let text = format!("fixture {suffix}");
            let delta = json!({"type":"response.output_text.delta","output_index":0,"content_index":0,"delta":text});
            let done = json!({"type":"response.output_text.done","output_index":0,"content_index":0,"text":text});
            let result = state
                .process_sse_block(&frame(&delta))
                .and_then(|mut frames| {
                    frames.extend(state.process_sse_block(&frame(&done))?);
                    Ok(frames)
                });
            if strict {
                assert!(
                    result.is_err(),
                    "strict truncated or malformed candidates fail"
                );
            } else {
                match result {
                    Err(_) => rejected.push(suffix),
                    Ok(frames) => {
                        let joined = frames
                            .iter()
                            .map(|b| std::str::from_utf8(b).unwrap())
                            .collect::<String>();
                        let values = joined
                            .split("\n\n")
                            .filter(|s| !s.is_empty())
                            .map(|s| {
                                serde_json::from_str::<Value>(s.split("data: ").nth(1).unwrap())
                                    .unwrap()
                            })
                            .collect::<Vec<_>>();
                        let deltas = values
                            .iter()
                            .filter_map(|v| v.get("delta").and_then(Value::as_str))
                            .collect::<String>();
                        assert_eq!(
                            deltas, text,
                            "ordinary prefix must not be lost or moved into the done snapshot only"
                        );
                        assert_eq!(values.len(), 2, "preserve existing event positions");
                    }
                }
            }
        }
        assert!(
            rejected.is_empty(),
            "ordinary short prefix rejected: {rejected:?}"
        );
    }
    #[test]
    fn private_barrier_short_prefix_interleaving_preserves_replay_order_and_snapshots() {
        for suffix in ["v", "va", "var", "vary", "vary_", "vary_p", "vary_ph"] {
            for resume_delta in [false, true] {
                let budget = PayloadBudget::with_limit(4 * 1024 * 1024);
                let (_store, lease, binding, token) = fixture(&budget);
                let mut state = SseRestorationState::new(
                    lease.fork_attempt().expect("attempt"),
                    binding.clone(),
                    budget.clone(),
                    ArgumentNormalizationMode::Exact,
                )
                .expect("state");
                let streamed = "already streamed. ";
                let initial = json!({"type":"response.output_text.delta","output_index":0,"content_index":0,"delta":streamed,"sequence_number":0});
                let released = state
                    .process_sse_block(&frame(&initial))
                    .expect("stream ordinary text");
                assert_eq!(released.len(), 1);
                assert_eq!(
                    std::str::from_utf8(&released[0]).expect("utf8"),
                    frame(&initial)
                );

                let prefix = format!("fixture {suffix}");
                let continuation = if resume_delta {
                    "! ordinary continuation"
                } else {
                    ""
                };
                let remaining_text = format!("{prefix}{continuation}");
                let full_text = format!("{streamed}{remaining_text}");
                let other_part = "interleaved second part";
                let arguments = format!("{{\"value\":\"{token}\"}}");
                let restored_arguments = "{\"value\":\"fixture-barrier-secret\"}";
                let mut raw_tool = tool_item(&arguments);
                raw_tool["fixture_marker"] = json!("preserve tool metadata");
                let mut restored_tool = raw_tool.clone();
                restored_tool["arguments"] = json!(restored_arguments);
                let mut start = added();
                start["output_index"] = json!(1);
                start["item"]["fixture_marker"] = raw_tool["fixture_marker"].clone();
                let message = json!({"id":"msg_fixture","type":"message","role":"assistant","status":"completed","content":[
                    {"type":"output_text","text":full_text},
                    {"type":"output_text","text":other_part}
                ]});
                let text_delta = |part, delta: &str| json!({"type":"response.output_text.delta","output_index":0,"content_index":part,"delta":delta});
                let tool_delta = |delta: &str| json!({"type":"response.function_call_arguments.delta","output_index":1,"item_id":"fc_fixture","delta":delta});
                let raw_prefix = text_delta(0, &prefix);
                let other_delta = text_delta(1, other_part);
                let mut events = vec![
                    (raw_prefix, text_delta(0, &remaining_text)),
                    (other_delta.clone(), other_delta),
                    (start.clone(), start),
                    (tool_delta(&arguments[..20]), tool_delta(restored_arguments)),
                ];
                if resume_delta {
                    events.push((text_delta(0, continuation), text_delta(0, "")));
                }
                events.push((tool_delta(&arguments[20..]), tool_delta("")));
                for (part, text) in [(0, full_text.as_str()), (1, other_part)] {
                    let done = json!({"type":"response.output_text.done","output_index":0,"content_index":part,"text":text});
                    events.push((done.clone(), done));
                }
                events.push((
                        json!({"type":"response.function_call_arguments.done","output_index":1,"item_id":"fc_fixture","arguments":arguments}),
                        json!({"type":"response.function_call_arguments.done","output_index":1,"item_id":"fc_fixture","arguments":restored_arguments}),
                    ));
                events.push((
                    json!({"type":"response.output_item.done","output_index":1,"item":raw_tool}),
                    json!({"type":"response.output_item.done","output_index":1,"item":restored_tool}),
                ));
                let mut terminal = completed(raw_tool.clone());
                terminal["response"]["output"] = json!([message, raw_tool]);
                let mut restored_terminal = terminal.clone();
                restored_terminal["response"]["output"][1] = restored_tool;
                events.push((terminal, restored_terminal));

                for (index, (raw, expected)) in events.iter_mut().enumerate() {
                    raw["sequence_number"] = json!(index + 1);
                    expected["sequence_number"] = json!(index + 1);
                    assert!(
                        state
                            .process_sse_block(&frame(raw))
                            .expect("defer interleaved event")
                            .is_empty(),
                        "neither deferred text nor tools release before completion"
                    );
                }
                assert!(!cursor_exists(&lease, &binding));
                let replay = state
                    .finalize_and_flush(true)
                    .expect("restore interleaved short prefix");
                assert_eq!(
                    replay.len(),
                    events.len(),
                    "preserve every deferred event position"
                );
                for (bytes, (_, expected)) in replay.iter().zip(&events) {
                    assert_eq!(
                        std::str::from_utf8(bytes).expect("utf8"),
                        frame(expected),
                        "preserve event type, identity, sequence and restored snapshots"
                    );
                }
                assert!(cursor_exists(&lease, &binding));
                drop(released);
                drop(replay);
                drop(state);
                assert_eq!(budget.used(), 0);
            }
        }
    }
}
