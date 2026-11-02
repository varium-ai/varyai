//! Unit tests for post-consumer Codex JSON/Compact/SSE response restoration,
//! variable-length reference boundary retention, delta/snapshot consistency, and the
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
fn split_safe_prefix_retains_each_reference_prefix_and_preserves_malformed() {
    let token = "rredact-cn_phone-0123456789abcdef0123-ai";
    assert!(
        token.len() == 32 + "cn_phone".len(),
        "approved phone reference length required"
    );

    for split_len in 1..token.len() {
        let input = format!("hello {}", &token[..split_len]);
        let (safe, tail) = split_safe_prefix_and_tail(&input).expect("valid token prefix split");
        assert!(
            safe == "hello ",
            "incomplete reference must retain the complete prefix"
        );
        assert!(
            tail == &token[..split_len],
            "incomplete reference tail must remain byte exact"
        );
    }

    let complete = format!("hello {token} world");
    let (safe, tail) = split_safe_prefix_and_tail(&complete).expect("complete token is safe");
    assert!(
        safe == complete.as_str(),
        "complete reference must remain safe for restoration"
    );
    assert_eq!(tail, "");

    let malformed = "hello rredact-cn_phone-0123456789abcdef012Z-ai";
    let (safe, tail) = split_safe_prefix_and_tail(malformed).expect("literal split");
    assert!(
        safe == malformed && tail.is_empty(),
        "malformed_literal_changed"
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

    // Current authorized references in immutable fields must be refused.
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
    let denied =
        restore_and_publish_json_response(&unsupported, false, &lease, &binding, &witness, &budget);
    assert!(matches!(
        denied,
        Err(RestoreError::FailedBecause(
            super::restore_diagnostics::RestoreReason::TokenContext
        ))
    ));
    let mut literal = unsupported.clone();
    literal["output"][0]["name"] = json!("rredact-custom-00000000000000000000-ai");
    let staged =
        restore_and_publish_json_response(&literal, false, &lease, &binding, &witness, &budget);
    assert!(
        staged.is_ok_and(|value| value.as_value() == &literal && value.restored_occurrences() == 0)
    );

    // Compact restoration preserves unrelated opaque fields.
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
    assert!(
        frames1.is_empty(),
        "a possible reference remains buffered until recognized"
    );

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
    assert_eq!(
        frames2.len(),
        2,
        "buffered event positions must be preserved"
    );
    let f2_str = std::str::from_utf8(&frames2[0]).expect("utf8");
    assert!(f2_str.contains("\"delta\":\"Tel: 13800138000!\""));

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

    // Shared optional-ledger equality requires --test-threads=1.
    #[test]
    fn reasoning_history_sse_successful_finalize_is_single_release(
    ) -> Result<(), Box<dyn std::error::Error>> {
        for publish_first in [false, true] {
            let budget = PayloadBudget::with_limit(4 * 1024 * 1024);
            let (store, lease, binding, _) = fixture(&budget);
            let mut state = SseRestorationState::new(
                lease.fork_attempt()?,
                binding.clone(),
                budget.clone(),
                ArgumentNormalizationMode::Exact,
            )
            .map_err(|error| error.code())?;
            let item = json!({
                "id": "synthetic-finalize-reasoning",
                "type": "reasoning",
                "summary": [{
                    "type": "summary_text",
                    "text": "synthetic reasoning rredact-custom-"
                }]
            });
            let event = completed(item.clone());
            assert!(
                state
                    .process_sse_block(&frame(&event))
                    .map_err(|error| error.code())?
                    .is_empty(),
                "terminal must remain behind barrier"
            );
            let frames = state
                .finalize_and_flush(true)
                .map_err(|error| error.code())?;
            assert!(!frames.is_empty(), "first finalize must release terminal");
            if publish_first {
                state.publish_released_history();
            }
            let before = budget.used();
            let history_before = store
                .history_metadata_bytes_for_test()
                .ok_or("synthetic history accounting unavailable")?;
            let duplicate = state
                .finalize_and_flush(true)
                .map_err(|error| error.code())?;
            assert!(
                duplicate.is_empty(),
                "successful finalize must return an empty success without replay or proof capture"
            );
            drop(duplicate);
            assert!(
                budget.used() <= before,
                "duplicate finalize must not allocate another release"
            );
            assert!(
                store.history_metadata_bytes_for_test() == Some(history_before),
                "duplicate finalize must retain no additional proof owners"
            );
            state.publish_released_history();
            state.publish_released_history();
            assert!(
                store.history_metadata_bytes_for_test() == Some(history_before),
                "repeated publication must not duplicate proof owners"
            );
            let replay = json!({"input": [item]});
            assert!(
                lease.history_projection(&binding, &replay).is_some(),
                "one legitimate release must publish once"
            );
            drop(frames);
            drop(state);
            assert!(budget.used() == 0, "all response owners must release");
        }
        Ok(())
    }

    #[test]
    fn reasoning_history_sse_failed_reentry_discards_pending_proof(
    ) -> Result<(), Box<dyn std::error::Error>> {
        for fail_on_frame in [false, true] {
            let budget = PayloadBudget::with_limit(4 * 1024 * 1024);
            let (store, lease, binding, _) = fixture(&budget);
            let mut state = SseRestorationState::new(
                lease.fork_attempt()?,
                binding.clone(),
                budget.clone(),
                ArgumentNormalizationMode::Exact,
            )
            .map_err(|error| error.code())?;
            let item = json!({
                "id": "synthetic-pending-reasoning",
                "type": "reasoning",
                "summary": [{
                    "type": "summary_text",
                    "text": "synthetic pending reasoning rredact-custom-"
                }]
            });
            let replay = json!({"input": [item.clone()]});
            let event = completed(item);
            // Allocate the namespace controller without publishing response authority.
            let pending = lease
                .capture_delivered_history(&binding, &event["response"])
                .ok_or("synthetic history capture unavailable")?;
            drop(pending);
            let history_before = store
                .history_metadata_bytes_for_test()
                .ok_or("synthetic history accounting unavailable")?;
            assert!(state
                .process_sse_block(&frame(&event))
                .map_err(|error| error.code())?
                .is_empty());
            let frames = state
                .finalize_and_flush(true)
                .map_err(|error| error.code())?;
            assert!(!frames.is_empty(), "terminal preparation must succeed");
            assert!(
                lease.history_projection(&binding, &replay).is_none(),
                "preparation alone must not publish history"
            );
            let pending_charge = store
                .history_metadata_bytes_for_test()
                .ok_or("synthetic history accounting unavailable")?;
            assert!(
                pending_charge > history_before,
                "terminal preparation must retain a charged pending proof"
            );
            drop(frames);
            let failure = if fail_on_frame {
                state.process_sse_block(&frame(&json!({
                    "type": "response.synthetic_after_terminal"
                })))
            } else {
                state.finalize_and_flush(false)
            };
            assert!(failure.is_err(), "failed reentry must release no frames");
            drop(failure);
            let immediately_after_failure = store.history_metadata_bytes_for_test();
            state.publish_released_history();
            state.publish_released_history();
            assert!(
                lease.history_projection(&binding, &replay).is_none(),
                "failed reentry must never publish the old pending proof"
            );
            assert!(
                immediately_after_failure == Some(history_before),
                "failed reentry must immediately release pending proof owners"
            );
            assert!(
                state.finalize_and_flush(true).is_err(),
                "failed reentry must remain closed"
            );
            let failed_charge = store
                .history_metadata_bytes_for_test()
                .ok_or("synthetic history accounting unavailable")?;
            assert!(
                failed_charge < pending_charge,
                "failed reentry must release pending proof owners"
            );
            drop(state);
            assert!(
                budget.used() == 0,
                "response owners must release after failure"
            );
        }
        Ok(())
    }

    #[test]
    fn reasoning_history_sse_processing_after_finalize_is_closed(
    ) -> Result<(), Box<dyn std::error::Error>> {
        for block in ["", ": synthetic comment\n\n", "data: [DONE]\n\n"] {
            let budget = PayloadBudget::with_limit(4 * 1024 * 1024);
            let (store, lease, binding, _) = fixture(&budget);
            let mut state = SseRestorationState::new(
                lease.fork_attempt()?,
                binding.clone(),
                budget.clone(),
                ArgumentNormalizationMode::Exact,
            )
            .map_err(|error| error.code())?;
            let item = json!({"id":"synthetic-late-frame", "type":"reasoning", "summary":[
                {"type":"summary_text", "text":"synthetic late frame reasoning"}
            ]});
            let replay = json!({"input":[item.clone()]});
            let event = completed(item);
            let pending = lease
                .capture_delivered_history(&binding, &event["response"])
                .ok_or("synthetic history capture unavailable")?;
            drop(pending);
            let baseline = store.history_metadata_bytes_for_test();
            assert!(state
                .process_sse_block(&frame(&event))
                .map_err(|error| error.code())?
                .is_empty());
            assert!(
                state
                    .process_sse_block(block)
                    .map_err(|error| error.code())?
                    .is_empty(),
                "pre-finalize comments and terminators must retain existing semantics"
            );
            let frames = state
                .finalize_and_flush(true)
                .map_err(|error| error.code())?;
            assert!(!frames.is_empty(), "first finalize must prepare terminal");
            drop(frames);
            let late = state.process_sse_block(block);
            assert!(
                late.is_err(),
                "processing after successful finalize must remain closed"
            );
            drop(late);
            let failed_charge = store.history_metadata_bytes_for_test();
            state.publish_released_history();
            assert!(
                lease.history_projection(&binding, &replay).is_none(),
                "post-finalize processing must invalidate unreleased history proof"
            );
            assert!(
                failed_charge == baseline,
                "post-finalize rejection must immediately release pending owners"
            );
            assert!(
                state.finalize_and_flush(true).is_err(),
                "post-finalize processing failure must not reopen completion"
            );
            drop(state);
            assert!(
                budget.used() == 0,
                "late processing must release response owners"
            );
        }
        Ok(())
    }

    #[test]
    fn reasoning_history_sse_first_finalize_requires_completion_permission(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let budget = PayloadBudget::with_limit(4 * 1024 * 1024);
        let (_store, lease, binding, _) = fixture(&budget);
        let mut state = SseRestorationState::new(
            lease.fork_attempt()?,
            binding.clone(),
            budget.clone(),
            ArgumentNormalizationMode::Exact,
        )
        .map_err(|error| error.code())?;
        let item = json!({"id":"synthetic-no-permission", "type":"reasoning", "summary":[
            {"type":"summary_text", "text":"synthetic unpermitted reasoning"}
        ]});
        let replay = json!({"input":[item.clone()]});
        assert!(state
            .process_sse_block(&frame(&completed(item)))
            .map_err(|error| error.code())?
            .is_empty());
        let rejected = state.finalize_and_flush(false);
        assert!(
            rejected.is_err(),
            "completion permission must precede terminal release"
        );
        drop(rejected);
        state.publish_released_history();
        assert!(
            lease.history_projection(&binding, &replay).is_none(),
            "missing completion permission must grant no history proof"
        );
        assert!(
            !cursor_exists(&lease, &binding),
            "missing completion permission must grant no cursor"
        );
        assert!(
            state.finalize_and_flush(true).is_err(),
            "completion failure must remain closed"
        );
        drop(state);
        assert!(
            budget.used() == 0,
            "unpermitted completion owners must release"
        );
        Ok(())
    }

    #[test]
    #[serial_test::serial]
    fn reasoning_history_sse_local_encoding_failure_never_publishes_proof(
    ) -> Result<(), Box<dyn std::error::Error>> {
        check_history_sse_local_encoding_boundary(Some(false))
    }

    #[test]
    #[serial_test::serial]
    fn reasoning_history_sse_final_frame_failure_never_publishes_proof(
    ) -> Result<(), Box<dyn std::error::Error>> {
        check_history_sse_local_encoding_boundary(Some(true))
    }

    #[test]
    #[serial_test::serial]
    fn reasoning_history_sse_local_encoding_success_controls(
    ) -> Result<(), Box<dyn std::error::Error>> {
        check_history_sse_local_encoding_boundary(None)
    }

    // Global optional-ledger equality requires --test-threads=1.
    fn check_history_sse_local_encoding_boundary(
        fail_final_frame: Option<bool>,
    ) -> Result<(), Box<dyn std::error::Error>> {
        use super::super::payload_buffer::test_support::{self, Action, Phase};
        use std::{cell::Cell, rc::Rc};

        for active in [false, true] {
            for reasoning in [false, true] {
                for mixed_tool in [false, true] {
                    let budget = PayloadBudget::with_limit(4 * 1024 * 1024);
                    let (store, lease, binding, _) = fixture(&budget);
                    let attempt = if active {
                        lease.fork_attempt()?
                    } else {
                        store.admit("codex", Some("fixture-barrier-session"), 1)?
                    };
                    let mut state = SseRestorationState::new(
                        attempt,
                        binding.clone(),
                        budget.clone(),
                        ArgumentNormalizationMode::Exact,
                    )
                    .map_err(|error| error.code())?;
                    let item = if reasoning {
                        json!({
                            "id": "synthetic-encoding-reasoning",
                            "type": "reasoning",
                            "summary": [{
                                "type": "summary_text",
                                "text": "synthetic reasoning rredact-custom-"
                            }]
                        })
                    } else {
                        json!({
                            "id": "synthetic-encoding-output",
                            "type": "message",
                            "role": "assistant",
                            "status": "completed",
                            "content": [{
                                "type": "output_text",
                                "text": "synthetic output rredact-custom-",
                                "annotations": []
                            }]
                        })
                    };
                    let mut event = completed(item.clone());
                    if mixed_tool {
                        assert!(
                            state
                                .process_sse_block(&frame(&added()))
                                .map_err(|error| error.code())?
                                .is_empty(),
                            "mixed fixture must enter the tool barrier"
                        );
                        event["response"]["output"] =
                            json!([tool_item("{\"value\":\"synthetic safe\"}"), item]);
                    }
                    event["synthetic_padding"] = json!("x".repeat(128 * 1024));
                    let json_len = serde_json::to_vec(&event)?.len();
                    let frame_len = frame(&event).len();
                    assert!(frame_len > json_len, "synthetic framing must add bytes");
                    assert!(
                        state
                            .process_sse_block(&frame(&event))
                            .map_err(|error| error.code())?
                            .is_empty(),
                        "terminal must wait for local encoding"
                    );
                    // Warm the namespace controller without granting response authority.
                    let pending = lease
                        .capture_delivered_history(&binding, &event["response"])
                        .ok_or("synthetic history capture unavailable")?;
                    drop(pending);
                    let history_before = store
                        .history_metadata_bytes_for_test()
                        .ok_or("synthetic history accounting unavailable")?;
                    let replay = json!({"input": event["response"]["output"]});
                    assert!(
                        lease.history_projection(&binding, &replay).is_none(),
                        "fixture preparation must not publish proof"
                    );
                    let injected = Rc::new(Cell::new(false));
                    let seen = injected.clone();
                    let encoded_bytes = Rc::new(Cell::new(0usize));
                    let copied = encoded_bytes.clone();
                    let hook =
                        test_support::install(move |observation| {
                            if observation.phase == Phase::BeforeCopy
                                && observation.requested == json_len
                            {
                                copied.set(copied.get() + observation.bytes_to_copy);
                            }
                            let target_len = fail_final_frame.map(|final_frame| {
                                if final_frame {
                                    frame_len
                                } else {
                                    json_len
                                }
                            });
                            if observation.phase == Phase::BeforeAllocation
                                && target_len == Some(observation.requested)
                            {
                                seen.set(true);
                                return Action::FailAllocation;
                            }
                            Action::Continue
                        });
                    let result = state.finalize_and_flush(true);
                    drop(hook);
                    if let Some(final_frame) = fail_final_frame {
                        assert!(injected.get(), "failure must reach the selected allocation");
                        if final_frame {
                            assert!(
                                encoded_bytes.get() == json_len,
                                "JSON must be encoded before final frame allocation fails"
                            );
                        } else {
                            assert!(
                                encoded_bytes.get() == 0,
                                "JSON allocation failure must precede encoding"
                            );
                        }
                        assert!(
                            result.is_err(),
                            "local failure must release no partial replay"
                        );
                        drop(result);
                        assert!(
                            state.finalize_and_flush(true).is_err(),
                            "local failure must remain closed"
                        );
                        state.publish_released_history();
                        assert!(
                            lease.history_projection(&binding, &replay).is_none(),
                            "local failure must grant no history proof"
                        );
                        assert!(
                            !cursor_exists(&lease, &binding),
                            "local failure must grant no cursor"
                        );
                        assert!(
                            store.history_metadata_bytes_for_test() == Some(history_before),
                            "local failure must retain no proof owners"
                        );
                    } else {
                        assert!(!injected.get(), "success control must inject no fault");
                        let frames = result.map_err(|error| error.code())?;
                        assert!(
                            frames.len() == if mixed_tool { 2 } else { 1 },
                            "success control must release the complete barrier"
                        );
                        assert!(
                            encoded_bytes.get() == json_len,
                            "success control must encode the complete terminal JSON"
                        );
                        assert!(
                            lease.history_projection(&binding, &replay).is_none(),
                            "prepared terminal alone must not publish history"
                        );
                        state.publish_released_history();
                        assert!(
                            lease.history_projection(&binding, &replay).is_some(),
                            "uninjected delivery must publish exact history"
                        );
                        assert!(
                            cursor_exists(&lease, &binding),
                            "uninjected mandatory completion must publish its cursor"
                        );
                        drop(frames);
                    }
                    drop(state);
                    assert!(budget.used() == 0, "all encoding owners must release");
                }
            }
        }
        Ok(())
    }

    #[test]
    fn reversible_restore_diagnostic_failure_never_publishes_cursor_or_tokens() {
        let budget = PayloadBudget::with_limit(4 * 1024 * 1024);
        let (store, lease, binding, token) = fixture(&budget);
        let observer = store
            .admit("codex", Some("fixture-barrier-session"), 1)
            .unwrap();
        let mut state = SseRestorationState::new(
            lease,
            binding.clone(),
            budget,
            ArgumentNormalizationMode::Exact,
        )
        .unwrap();
        assert!(state
            .process_sse_block(&frame(&added()))
            .unwrap()
            .is_empty());
        let item = tool_item(&format!("{{\"value\":\"{token}\",}}"));
        assert!(state
            .process_sse_block(&frame(&completed(item)))
            .unwrap()
            .is_empty());
        let error = state.finalize_and_flush(true).unwrap_err();
        assert_eq!(error.code(), "dlp_restore_failed");
        assert_eq!(
            error.reason_at(state.failure_stage()),
            Some(super::super::restore_diagnostics::RestoreReason::ArgumentsJson,)
        );
        assert!(!cursor_exists(&observer, &binding));
        assert!(observer
            .fork_attempt()
            .unwrap()
            .resume_completed_tokens(&binding, &[&token])
            .is_err());
        assert_eq!(state.finalize_and_flush(true).unwrap_err(), error);
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
        // A current authorized reference in unsupported metadata closes the barrier immediately.
        assert!(matches!(
            state.process_sse_block(&frame(&event)),
            Err(RestoreError::FailedBecause(
                super::super::restore_diagnostics::RestoreReason::TokenContext
            ))
        ));
        assert!(state.finalize_and_flush(false).is_err());
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
    fn private_barrier_ordinary_nonmatching_prefixes_flush_unchanged() {
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
            "rredact",
            "rredact-",
            "rredact-cn",
            "rredact-cn_phone-",
            "rredact-cn_phone-0",
            "rredact-cn_phone-Z",
            "vary_ph_",
            "vary_ph_0",
            "vary_ph_Z",
        ] {
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
                    assert!(
                        deltas == text,
                        "ordinary prefix must not be lost or moved into the done snapshot only"
                    );
                    assert_eq!(values.len(), 2, "preserve existing event positions");
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
        for suffix in [
            "v",
            "va",
            "var",
            "vary",
            "vary_",
            "vary_p",
            "vary_ph",
            "r",
            "rr",
            "rredact",
            "rredact-",
            "rredact-cn",
            "rredact-cn_phone",
        ] {
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

#[cfg(test)]
mod text_fallback_tests {
    use super::super::{
        response_restoration::{prepare_text_projection, TEXT_PROJECTION_NOTICE},
        tokenization::DlpRequestLease,
    };
    use super::*;

    fn fixture() -> (DlpTokenStore, DlpRequestLease, PayloadBudget, String) {
        let store = DlpTokenStore::new();
        let budget = PayloadBudget::with_limit(16 * 1024 * 1024);
        let mut lease = store
            .admit("codex", Some("projection-fixture"), 1)
            .expect("fixture admission");
        lease
            .prepare_with_budget(
                &[TokenFragment {
                    raw: "synthetic-projection-value",
                    whole_span_categories: &[TokenCategory::Custom],
                }],
                &[],
                budget.clone(),
                |_| Ok(()),
            )
            .expect("fixture preparation");
        let reference = lease
            .authorized_tokens()
            .expect("fixture authority")
            .as_slice()[0]
            .to_owned();
        (store, lease, budget, reference)
    }

    fn response(text: &str) -> Value {
        json!({"id":"resp_projection", "object":"response", "status":"completed", "output":[{
            "id":"msg_projection", "type":"message", "role":"assistant", "status":"completed",
            "content":[{"type":"output_text", "text":text, "annotations":[]}]
        }]})
    }

    #[test]
    fn text_fallback_json_whole_field_and_no_authority() {
        let (store, lease, budget, reference) = fixture();
        let raw = response(&format!("{reference} rredact-custom-damaged"));
        let prepared = prepare_text_projection(&raw, &lease, &budget).expect("eligible projection");
        assert!(prepared.as_value()["output"][0]["content"][0]["text"] == TEXT_PROJECTION_NOTICE);
        assert!(raw["output"][0]["content"][0]["text"] != TEXT_PROJECTION_NOTICE);
        let binding = lease
            .bind_actual_attempt(
                "fixture",
                "openai_responses",
                "http://127.0.0.1:1/responses",
                None,
            )
            .expect("binding");
        let mut observer = store
            .admit("codex", Some("projection-fixture"), 1)
            .expect("observer");
        assert!(observer
            .resume_completed_context(&binding, Some("resp_projection"), &[], &[])
            .is_err());
        assert!(observer
            .resume_completed_tokens(&binding, &[&reference])
            .is_err());
        drop(prepared);
        assert_eq!(budget.used(), 0);
    }

    #[test]
    fn text_fallback_json_hard_failures_override_format() {
        let (_store, lease, budget, reference) = fixture();
        for before in [true, false] {
            let unknown = reference.replacen("rredact-custom-", "rredact-cn_phone-", 1);
            let text = if before {
                format!("{unknown} rredact-custom-bad")
            } else {
                format!("rredact-custom-bad {unknown}")
            };
            let result = prepare_text_projection(&response(&text), &lease, &budget);
            assert!(
                result.is_err(),
                "zero_restore_projection_must_not_gain_authority"
            );
        }
        for field in ["id", "annotations", "unknown"] {
            let mut raw = response("rredact-custom-bad");
            raw["output"][0]["content"][0][field] = json!("rredact-custom-bad");
            let projected = prepare_text_projection(&raw, &lease, &budget)
                .expect("test-only damaged projection");
            assert!(
                projected.as_value()["output"][0]["content"][0][field]
                    == raw["output"][0]["content"][0][field],
                "literal_metadata_changed"
            );
        }
        for kind in [
            "reasoning",
            "refusal",
            "compaction",
            "function_call",
            "custom_tool_call",
            "unknown",
        ] {
            let mut raw = response("rredact-custom-bad");
            raw["output"]
                .as_array_mut()
                .expect("fixture output")
                .push(json!({"type":kind}));
            assert!(prepare_text_projection(&raw, &lease, &budget).is_err());
        }
        let mut incomplete = response("rredact-custom-bad");
        incomplete["status"] = json!("incomplete");
        assert!(prepare_text_projection(&incomplete, &lease, &budget).is_err());
        assert_eq!(budget.used(), 0);
    }

    #[test]
    fn text_fallback_json_valid_fields_restored_and_capacity_refuses() {
        let (_store, lease, budget, reference) = fixture();
        let mut raw = response("rredact-custom-bad");
        raw["output"][0]["content"]
            .as_array_mut()
            .expect("fixture content")
            .push(json!({"type":"output_text", "text":reference}));
        let prepared = prepare_text_projection(&raw, &lease, &budget).expect("eligible projection");
        assert!(
            prepared.as_value()["output"][0]["content"][1]["text"] == "synthetic-projection-value"
        );
        assert!(prepare_text_projection(&raw, &lease, &PayloadBudget::with_limit(1)).is_err());
        assert!(prepare_text_projection(&response("ordinary text"), &lease, &budget).is_err());
    }
    #[test]
    fn text_fallback_native_object_json_refuses_wrong_or_missing() {
        let (_store, lease, budget, _reference) = fixture();
        for object in [Some(json!("other")), Some(Value::Null), None] {
            let mut raw = response("rredact-custom-bad");
            if let Some(value) = object {
                raw["object"] = value;
            } else {
                raw.as_object_mut().expect("object").remove("object");
            }
            assert!(
                prepare_text_projection(&raw, &lease, &budget).is_err(),
                "non-native object cannot project"
            );
        }
        assert_eq!(budget.used(), 0);
    }
}

#[cfg(test)]
mod output_text_exact_tests {
    use super::super::response_restoration::stage_json_response;
    use super::*;

    fn text_response(text: &str) -> Value {
        json!({"id":"synthetic-exact-response","object":"response","status":"completed","output":[{
            "id":"synthetic-exact-message","type":"message","role":"assistant","status":"completed",
            "content":[{"type":"output_text","text":text,"annotations":[]}]
        }]})
    }

    #[test]
    fn output_text_exact_json_nonmatches_are_unchanged() {
        let store = DlpTokenStore::new();
        let lease = store
            .admit("codex", Some("synthetic-exact-json"), 1)
            .expect("synthetic lease");
        let budget = PayloadBudget::with_limit(4 * 1024 * 1024);
        let header = ["rredact", "-custom-"].concat();
        for text in [
            format!("Explain `{header}` safely."),
            format!("前🔒{header}**说明**"),
            header.clone(),
            format!("{header}{}", "0".repeat(20)),
            "ordinary unchanged text".to_owned(),
        ] {
            let response = text_response(&text);
            let staged = stage_json_response(&response, false, &lease, &budget);
            assert!(
                staged.is_ok(),
                "nonmatching output text must not fail restoration"
            );
            let staged = staged.expect("staged synthetic response");
            assert!(
                staged.as_value()["output"][0]["content"][0]["text"].as_str()
                    == Some(text.as_str()),
                "nonmatching output text must be preserved"
            );
        }
    }

    #[test]
    fn output_text_exact_json_nonmatches_do_not_hide_authorized_references() {
        let store = DlpTokenStore::new();
        let budget = PayloadBudget::with_limit(4 * 1024 * 1024);
        let mut lease = store
            .admit("codex", Some("synthetic-exact-json"), 1)
            .expect("synthetic lease");
        lease
            .prepare_with_budget(
                &[TokenFragment {
                    raw: "synthetic-restored-text",
                    whole_span_categories: &[TokenCategory::Custom],
                }],
                &[],
                budget.clone(),
                |_| Ok(()),
            )
            .expect("synthetic preparation");
        let refs = lease.authorized_tokens().expect("synthetic references");
        let reference = refs.as_slice()[0];
        let header = ["rredact", "-custom-"].concat();
        for text in [
            format!("前🔒{header}** {reference}"),
            format!("前🔒{header}{reference}"),
            format!("{reference} `{header}` {reference}"),
        ] {
            let expected = text.replace(reference, "synthetic-restored-text");
            let response = text_response(&text);
            let staged = stage_json_response(&response, false, &lease, &budget);
            assert!(
                staged.is_ok(),
                "nonmatch must not hide a later authorized reference"
            );
            assert!(
                staged.expect("synthetic staged response").as_value()["output"][0]["content"][0]
                    ["text"]
                    .as_str()
                    == Some(expected.as_str()),
                "only complete authorized references may change"
            );
        }
    }

    #[test]
    fn output_text_exact_json_complete_unauthorized_reference_stays_literal() {
        let store = DlpTokenStore::new();
        let budget = PayloadBudget::with_limit(4 * 1024 * 1024);
        let mut owner = store
            .admit("codex", Some("synthetic-exact-owner"), 1)
            .expect("synthetic owner");
        owner
            .prepare_with_budget(
                &[TokenFragment {
                    raw: "synthetic-private-text",
                    whole_span_categories: &[TokenCategory::Custom],
                }],
                &[],
                budget.clone(),
                |_| Ok(()),
            )
            .expect("synthetic preparation");
        let refs = owner.authorized_tokens().expect("synthetic references");
        let lease = store
            .admit("codex", Some("synthetic-exact-other"), 1)
            .expect("synthetic lease");
        let header = ["rredact", "-custom-"].concat();
        for text in [
            format!("前🔒{header}** {}", refs.as_slice()[0]),
            format!("前🔒{header}{}", refs.as_slice()[0]),
        ] {
            let response = text_response(&text);
            let staged =
                stage_json_response(&response, false, &lease, &budget).expect("literal response");
            assert!(staged.as_value() == &response, "foreign_literal_changed");
            assert!(lease.is_empty(), "foreign_literal_granted_authority");
        }
    }

    #[test]
    fn output_text_exact_json_budget_failure_refuses_without_retained_charge() {
        let store = DlpTokenStore::new();
        let lease = store
            .admit("codex", Some("synthetic-exact-capacity"), 1)
            .expect("synthetic lease");
        let budget = PayloadBudget::with_limit(1);
        let response = text_response("ordinary text remains subject to payload limits");
        assert!(
            stage_json_response(&response, false, &lease, &budget).is_err(),
            "exact recognition must not bypass the payload budget"
        );
        assert_eq!(budget.used(), 0);
    }

    #[test]
    fn output_text_exact_json_other_carriers_preserve_literals() {
        let store = DlpTokenStore::new();
        let budget = PayloadBudget::with_limit(4 * 1024 * 1024);
        let lease = store
            .admit("codex", Some("synthetic-exact-json"), 1)
            .expect("synthetic lease");
        let malformed = ["rredact", "-custom-**"].concat();
        for mut response in [text_response("safe"), text_response("safe")] {
            response["output"][0]["content"][0]["annotations"] = json!([{"text":malformed}]);
            assert!(
                stage_json_response(&response, false, &lease, &budget)
                    .map(|staged| staged.as_value() == &response)
                    .unwrap_or(false),
                "literal_carrier_changed"
            );
        }
        let mut response = text_response(&malformed);
        response["output"][0]["content"][0] = json!({"type":"refusal","refusal":malformed});
        assert!(
            stage_json_response(&response, false, &lease, &budget)
                .map(|staged| staged.as_value() == &response)
                .unwrap_or(false),
            "literal_carrier_changed"
        );
        response["output"][0] = json!({"type":"function_call","id":"synthetic-tool","call_id":"synthetic-call","name":"nonexecuting_fixture","arguments":serde_json::to_string(&json!({"text":malformed})).expect("synthetic JSON"),"status":"completed"});
        assert!(
            stage_json_response(&response, false, &lease, &budget)
                .map(|staged| staged.as_value() == &response)
                .unwrap_or(false),
            "literal_carrier_changed"
        );
    }
}

#[test]
fn reference_format_migration_sse_every_category_split_preserves_barrier_authorization() {
    use super::tokenization::TokenCategory::*;
    fn frame(event: &Value) -> String {
        format!(
            "event: {}\ndata: {event}\n\n",
            event["type"].as_str().expect("synthetic event type")
        )
    }
    for (category, name) in [
        (Phone, "cn_phone"),
        (IdCard, "cn_id_card"),
        (PrivateKey, "private_key"),
        (ApiKey, "api_token"),
        (Database, "db_conn"),
        (Password, "password"),
        (Infrastructure, "infra"),
        (Personal, "personal"),
        (Custom, "custom"),
        (Mixed, "mixed"),
    ] {
        for split in 1..(32 + name.len()) {
            for authorized in [false, true] {
                let store = DlpTokenStore::new();
                let mut lease = store.admit("codex", None, 1).expect("synthetic lease");
                let issued = lease
                    .prepare(
                        &[TokenFragment {
                            raw: "synthetic-category-restoration",
                            whole_span_categories: &[category],
                        }],
                        &[],
                        |tokens| Ok(tokens.to_vec()),
                    )
                    .expect("synthetic preparation");
                let reference = &issued[0];
                assert!(
                    reference.len() == 32 + name.len(),
                    "approved reference length required before split"
                );
                let mut candidate = reference.clone();
                if !authorized {
                    let digest_start = 8 + name.len() + 1;
                    let replacement = if &candidate[digest_start..digest_start + 1] == "0" {
                        "1"
                    } else {
                        "0"
                    };
                    candidate.replace_range(digest_start..digest_start + 1, replacement);
                }
                let binding = lease
                    .bind_actual_attempt(
                        "synthetic-provider",
                        "openai_responses",
                        "http://127.0.0.1:1/responses",
                        None,
                    )
                    .expect("synthetic binding");
                let mut state = SseRestorationState::new(
                    lease,
                    binding,
                    PayloadBudget::with_limit(8 * 1024 * 1024),
                    ArgumentNormalizationMode::Exact,
                )
                .expect("synthetic state");
                let first = state.process_sse_block(&frame(&json!({"type":"response.output_text.delta","output_index":0,"content_index":0,"delta":&candidate[..split]}))).expect("synthetic prefix");
                assert!(first.is_empty(), "possible reference must remain buffered");
                let tool = json!({"id":"synthetic-tool","type":"function_call","call_id":"synthetic-call","name":"non_executing_fixture","arguments":"{}","status":"completed"});
                assert!(state.process_sse_block(&frame(&json!({"type":"response.output_item.added","output_index":1,"item":tool}))).expect("synthetic barrier").is_empty(), "tool barrier must withhold executable items");
                assert!(state.process_sse_block(&frame(&json!({"type":"response.output_text.delta","output_index":0,"content_index":0,"delta":&candidate[split..]}))).expect("synthetic suffix").is_empty(), "reference suffix must remain behind barrier");
                let item = json!({"id":"synthetic-message","type":"message","role":"assistant","status":"completed","content":[{"type":"output_text","text":candidate}]});
                assert!(state.process_sse_block(&frame(&json!({"type":"response.completed","response":{"id":"synthetic-response","status":"completed","output":[item,tool]}}))).expect("synthetic snapshot").is_empty(), "completion snapshot must remain behind barrier");
                let result = state.finalize_and_flush(true);
                if authorized {
                    assert!(
                        result.is_ok(),
                        "authorized split reference and snapshot must restore"
                    );
                    let mut delivered = String::new();
                    for bytes in result.expect("synthetic released frames") {
                        let raw = std::str::from_utf8(&bytes).expect("synthetic UTF-8");
                        let data = raw
                            .lines()
                            .find_map(|line| line.strip_prefix("data: "))
                            .expect("synthetic data");
                        let event: Value = serde_json::from_str(data).expect("synthetic JSON");
                        if event["type"] == "response.output_text.delta" {
                            delivered.push_str(event["delta"].as_str().expect("synthetic delta"));
                        }
                        if event["type"] == "response.completed" {
                            assert!(
                                event["response"]["output"][0]["content"][0]["text"]
                                    == "synthetic-category-restoration",
                                "authorized snapshot must restore"
                            );
                        }
                    }
                    assert!(
                        delivered == "synthetic-category-restoration",
                        "split reference must restore exactly once"
                    );
                } else {
                    let frames = result.expect("unknown literal with valid completion");
                    let mut delivered = String::new();
                    for bytes in frames {
                        let raw = std::str::from_utf8(&bytes).expect("synthetic UTF-8");
                        for data in raw.lines().filter_map(|line| line.strip_prefix("data: ")) {
                            let event: Value = serde_json::from_str(data).expect("synthetic JSON");
                            if event["type"] == "response.output_text.delta" {
                                delivered.push_str(event["delta"].as_str().unwrap_or(""));
                            }
                            if event["type"] == "response.completed" {
                                assert!(
                                    event["response"]["output"][0]["content"][0]["text"]
                                        == candidate,
                                    "foreign_snapshot_changed"
                                );
                            }
                        }
                    }
                    assert!(delivered == candidate, "foreign_literal_changed");
                }
            }
        }
    }
}

#[test]
fn reference_format_legacy_output_preserves_literals_and_checks_nested_candidates() {
    use super::{response_restoration::stage_json_response, restore_diagnostics::RestoreReason};
    fn frame(event: &Value) -> String {
        format!(
            "event: {}\ndata: {event}\n\n",
            event["type"].as_str().expect("synthetic event type")
        )
    }
    fn response(text: &str) -> Value {
        json!({"id":"synthetic-legacy-response","object":"response","status":"completed","output":[{
            "id":"synthetic-legacy-message","type":"message","role":"assistant","status":"completed",
            "content":[{"type":"output_text","text":text}]
        }]})
    }
    let store = DlpTokenStore::new();
    let mut lease = store.admit("codex", None, 1).expect("synthetic lease");
    let issued = lease
        .prepare(
            &[TokenFragment {
                raw: "synthetic-legacy-restoration",
                whole_span_categories: &[TokenCategory::Custom],
            }],
            &[],
            |tokens| Ok(tokens.to_vec()),
        )
        .expect("synthetic preparation");
    let reference = &issued[0];
    let legacy = format!(
        "vary_cu_{}_ai",
        &reference[reference.len() - 23..reference.len() - 3]
    );
    let binding = lease
        .bind_actual_attempt(
            "synthetic-provider",
            "openai_responses",
            "http://127.0.0.1:1/responses",
            None,
        )
        .expect("synthetic binding");
    for (text, rejected) in [
        (legacy.clone(), false),
        (format!("{legacy}{reference}"), false),
        (format!("vary_cu_broken {legacy}"), false),
        ("vary_cu_broken".to_owned(), false),
        (format!("vary_cu_{reference}"), false),
        (format!("vary_cu_bad {reference} vary_ph_"), false),
    ] {
        let budget = PayloadBudget::with_limit(8 * 1024 * 1024);
        let raw_response = response(&text);
        let staged = stage_json_response(&raw_response, false, &lease, &budget);
        if rejected {
            assert!(
                matches!(
                    staged,
                    Err(RestoreError::FailedBecause(RestoreReason::TokenContext))
                ),
                "complete legacy output must retain context refusal"
            );
        } else {
            assert!(
                staged.is_ok(),
                "malformed legacy output remains ordinary text"
            );
            assert!(
                staged.expect("synthetic staged response").as_value()["output"][0]["content"][0]
                    ["text"]
                    == text.replace(reference, "synthetic-legacy-restoration"),
                "legacy nonmatches must not hide nested authorized references"
            );
        }
        for split in 0..=text.len() {
            let mut state = SseRestorationState::new(
                lease.fork_attempt().expect("synthetic attempt"),
                binding.clone(),
                budget.clone(),
                ArgumentNormalizationMode::Exact,
            )
            .expect("synthetic state");
            let result = (|| {
                let mut frames = Vec::new();
                for delta in [&text[..split], &text[split..]] {
                    frames.extend(state.process_sse_block(&frame(&json!({"type":"response.output_text.delta","output_index":0,"content_index":0,"delta":delta})))?);
                }
                frames.extend(state.process_sse_block(&frame(&json!({"type":"response.output_text.done","output_index":0,"content_index":0,"text":text})))?);
                frames.extend(state.process_sse_block(&frame(
                    &json!({"type":"response.completed","response":response(&text)}),
                ))?);
                frames.extend(state.finalize_and_flush(true)?);
                Ok::<_, RestoreError>(frames)
            })();
            if rejected {
                assert!(
                    matches!(
                        result,
                        Err(RestoreError::FailedBecause(RestoreReason::TokenContext))
                    ),
                    "complete legacy SSE reference must refuse at every split"
                );
            } else {
                assert!(
                    result.is_ok(),
                    "legacy nonmatches and nested references must survive every SSE split"
                );
                let mut delivered = String::new();
                for bytes in result.expect("synthetic frames") {
                    let raw = std::str::from_utf8(&bytes).expect("synthetic UTF-8");
                    let data = raw
                        .lines()
                        .find_map(|line| line.strip_prefix("data: "))
                        .expect("synthetic data");
                    let event: Value = serde_json::from_str(data).expect("synthetic JSON");
                    if event["type"] == "response.output_text.delta" {
                        delivered.push_str(event["delta"].as_str().expect("synthetic delta"));
                    }
                }
                assert!(
                    delivered == text.replace(reference, "synthetic-legacy-restoration"),
                    "legacy nonmatching SSE bytes must be preserved exactly once"
                );
            }
        }
    }
}

fn reference_format_strict_fixture() -> Result<
    (
        super::tokenization::DlpRequestLease,
        super::completed_context::ActualAttemptBinding,
        PayloadBudget,
    ),
    super::tokenization::TokenError,
> {
    let store = DlpTokenStore::new();
    let lease = store.admit("codex", None, 1)?;
    let binding = lease.bind_actual_attempt(
        "synthetic-provider",
        "openai_responses",
        "http://127.0.0.1:1/responses",
        None,
    )?;
    Ok((lease, binding, PayloadBudget::with_limit(4 * 1024 * 1024)))
}

#[test]
fn reference_format_strict_partial_headers_match_json_and_native_sse(
) -> Result<(), super::tokenization::TokenError> {
    use super::response_restoration::stage_json_response;
    for name in [
        "cn_phone",
        "cn_id_card",
        "private_key",
        "api_token",
        "db_conn",
        "password",
        "infra",
        "personal",
        "custom",
        "mixed",
    ] {
        let header = format!("rredact-{name}-");
        for length in 1..=header.len() {
            let text = &header[..length];
            for reasoning in [false, true] {
                let rejected = false;
                for with_done in [false, true] {
                    let (lease, binding, budget) = reference_format_strict_fixture()?;
                    let item = if reasoning {
                        json!({"id":"synthetic-strict-item","type":"reasoning","summary":[{"type":"summary_text","text":text}]})
                    } else {
                        json!({"id":"synthetic-strict-item","type":"message","role":"assistant","content":[{"type":"refusal","refusal":text}]})
                    };
                    let response = json!({"id":"synthetic-strict-response","status":"completed","output":[item]});
                    assert!(
                        stage_json_response(&response, false, &lease, &budget).is_err() == rejected,
                        "partial_literals_remain_unchanged"
                    );
                    let mut state = SseRestorationState::new(
                        lease,
                        binding,
                        budget,
                        ArgumentNormalizationMode::Exact,
                    )
                    .map_err(|_| super::tokenization::TokenError::Unavailable)?;
                    let prefix = if reasoning {
                        "response.reasoning_summary_text"
                    } else {
                        "response.refusal"
                    };
                    let result = (|| {
                        let mut frames = Vec::new();
                        for delta in [&text[..length / 2], &text[length / 2..]] {
                            let event = json!({"type":format!("{prefix}.delta"),"output_index":0,"content_index":0,"summary_index":0,"delta":delta});
                            frames.extend(state.process_sse_block(&format!(
                                "event: {}\ndata: {event}\n\n",
                                event["type"].as_str().unwrap_or("synthetic")
                            ))?);
                        }
                        if with_done {
                            let mut event = json!({"type":format!("{prefix}.done"),"output_index":0,"content_index":0,"summary_index":0});
                            event[if reasoning { "text" } else { "refusal" }] = json!(text);
                            frames.extend(state.process_sse_block(&format!(
                                "event: {}\ndata: {event}\n\n",
                                event["type"].as_str().unwrap_or("synthetic")
                            ))?);
                        }
                        let event = json!({"type":"response.completed","response":response});
                        frames.extend(state.process_sse_block(&format!(
                            "event: response.completed\ndata: {event}\n\n"
                        ))?);
                        frames.extend(state.finalize_and_flush(true)?);
                        Ok::<_, RestoreError>(frames)
                    })();
                    assert!(
                        result.is_err() == rejected,
                        "partial_literals_survive_legal_completion"
                    );
                    if let Ok(frames) = result {
                        let mut joined = String::new();
                        for frame in frames {
                            for line in std::str::from_utf8(&frame).unwrap_or("").lines() {
                                if let Some(data) = line.strip_prefix("data: ") {
                                    if let Ok(event) = serde_json::from_str::<Value>(data) {
                                        if event["type"] == format!("{prefix}.delta") {
                                            joined.push_str(event["delta"].as_str().unwrap_or(""));
                                        }
                                    }
                                }
                            }
                        }
                        assert!(
                            joined == text,
                            "strict nonmatching deltas must preserve every byte"
                        );
                    }
                }
            }
        }
    }
    Ok(())
}

#[tokio::test]
async fn reference_format_strict_partial_headers_survive_chat_reasoning_conversion(
) -> Result<(), super::tokenization::TokenError> {
    use crate::proxy::providers::streaming_codex_chat::create_responses_sse_stream_from_chat;
    use futures::StreamExt;
    for text in [
        "rredact-",
        "rredact-cn_ph",
        "rredact-private_key",
        "rredact-custom-",
        "vary_cu_",
    ] {
        let rejected = false;
        let first = json!({"id":"synthetic-chat-strict","object":"chat.completion.chunk","choices":[{"index":0,"delta":{"role":"assistant","reasoning_content":text},"finish_reason":null}]});
        let last = json!({"id":"synthetic-chat-strict","object":"chat.completion.chunk","choices":[{"index":0,"delta":{"content":"synthetic-safe-output"},"finish_reason":"stop"}]});
        let source = futures::stream::iter(vec![Ok::<_, std::io::Error>(bytes::Bytes::from(
            format!("data: {first}\n\ndata: {last}\n\ndata: [DONE]\n\n"),
        ))]);
        let converted = create_responses_sse_stream_from_chat(source);
        futures::pin_mut!(converted);
        let (lease, binding, budget) = reference_format_strict_fixture()?;
        let mut state =
            SseRestorationState::new(lease, binding, budget, ArgumentNormalizationMode::Exact)
                .map_err(|_| super::tokenization::TokenError::Unavailable)?;
        let mut failed = false;
        while let Some(chunk) = converted.next().await {
            assert!(chunk.is_ok(), "synthetic Chat conversion must remain valid");
            if let Ok(chunk) = chunk {
                let wire = std::str::from_utf8(&chunk).unwrap_or("");
                for block in wire.split("\n\n").filter(|block| !block.is_empty()) {
                    if !failed && state.process_sse_block(block).is_err() {
                        failed = true;
                    }
                }
            }
        }
        if !failed {
            failed = state.finalize_and_flush(true).is_err();
        }
        assert!(
            failed == rejected,
            "converted supported reasoning must remain unprocessed"
        );
    }
    Ok(())
}

#[test]
fn reasoning_passthrough_json_preserves_supported_leaves_and_strict_siblings(
) -> Result<(), super::tokenization::TokenError> {
    use super::response_restoration::stage_json_response;
    let (lease, _, budget) = reference_format_strict_fixture()?;
    for text in [
        "rredact-cn_phone-",
        "rredact-cn_phone-0123456789abcdef012Z-ai",
        "rredact-cn_phone-0123456789abcdef0123-ai",
        "rredact-custom-rredact-cn_phone-0123456789abcdef0123-ai-ai",
    ] {
        for item in [
            json!({"type":"reasoning","summary":[{"type":"summary_text","text":text}]}),
            json!({"type":"reasoning","content":[{"type":"reasoning_text","text":text}]}),
            json!({"type":"function_call","call_id":"synthetic-call","name":"synthetic_tool","arguments":"{}","reasoning_content":text}),
            json!({"type":"custom_tool_call","call_id":"synthetic-call","name":"synthetic_tool","input":"synthetic-safe","reasoning_content":text}),
        ] {
            let response =
                json!({"id":"synthetic-reasoning-response","status":"completed","output":[item]});
            let staged = stage_json_response(&response, false, &lease, &budget);
            assert!(
                staged.is_ok(),
                "supported reasoning leaf must not be scanned"
            );
            if let Ok(staged) = staged {
                assert!(
                    staged.as_value() == &response,
                    "reasoning bytes must remain exact"
                );
            }
        }
        for item in [
            json!({"type":"reasoning","summary":[],"metadata":{"text":text}}),
            json!({"type":"reasoning","summary":[{"type":"summary_text","text":"synthetic-safe","unknown":text}]}),
            json!({"type":"message","role":"assistant","content":[{"type":"refusal","refusal":text}]}),
            json!({"type":"function_call","call_id":"synthetic-call","name":"synthetic_tool","arguments":json!({"text":text}).to_string()}),
            json!({"type":"custom_tool_call","call_id":"synthetic-call","name":"synthetic_tool","input":text}),
        ] {
            let response =
                json!({"id":"synthetic-strict-response","status":"completed","output":[item]});
            assert!(
                stage_json_response(&response, false, &lease, &budget)
                    .map(|staged| staged.as_value() == &response)
                    .unwrap_or(false),
                "literal_siblings_changed"
            );
        }
    }
    Ok(())
}

#[test]
fn reasoning_passthrough_sse_preserves_deltas_and_rejects_snapshot_mismatch(
) -> Result<(), super::tokenization::TokenError> {
    let text = "rredact-cn_phone-0123456789abcdef012Z-ai";
    for mismatch in [false, true] {
        for split in 0..=text.len() {
            let (lease, binding, budget) = reference_format_strict_fixture()?;
            let mut state =
                SseRestorationState::new(lease, binding, budget, ArgumentNormalizationMode::Exact)
                    .map_err(|_| super::tokenization::TokenError::Unavailable)?;
            let mut emitted = String::new();
            for delta in [&text[..split], &text[split..]] {
                let event = json!({"type":"response.reasoning_summary_text.delta","output_index":0,"summary_index":0,"delta":delta});
                let result = state.process_sse_block(&format!(
                    "event: response.reasoning_summary_text.delta\ndata: {event}\n\n"
                ));
                assert!(
                    result.is_ok(),
                    "reasoning delta must not be scanned or buffered as a reference"
                );
                if let Ok(frames) = result {
                    for frame in frames {
                        for data in std::str::from_utf8(&frame)
                            .unwrap_or("")
                            .lines()
                            .filter_map(|line| line.strip_prefix("data: "))
                        {
                            if let Ok(event) = serde_json::from_str::<Value>(data) {
                                if event["type"] == "response.reasoning_summary_text.delta" {
                                    emitted.push_str(event["delta"].as_str().unwrap_or(""));
                                }
                            }
                        }
                    }
                }
            }
            assert!(
                emitted == text,
                "reasoning deltas must be delivered byte exact"
            );
            let final_text = if mismatch {
                "synthetic-different"
            } else {
                text
            };
            let response = json!({"id":"synthetic-reasoning-response","status":"completed","output":[{"type":"reasoning","summary":[{"type":"summary_text","text":final_text}]}]});
            let event = json!({"type":"response.completed","response":response});
            let result = state
                .process_sse_block(&format!("event: response.completed\ndata: {event}\n\n"))
                .and_then(|_| state.finalize_and_flush(true));
            assert!(
                result.is_err() == mismatch,
                "raw reasoning delta and snapshot equality must remain mandatory"
            );
        }
    }
    Ok(())
}

fn reasoning_pair_frame(event: Value) -> String {
    format!(
        "event: {}\ndata: {event}\n\n",
        event["type"].as_str().unwrap_or("")
    )
}

async fn reasoning_pair_delivered(events: Vec<Value>) -> String {
    use super::http_restoration::wrap_sse_stream_with_restoration;
    use bytes::Bytes;
    use futures::{stream, StreamExt};
    let (lease, binding, budget) = reference_format_strict_fixture().expect("synthetic fixture");
    let state = SseRestorationState::new(lease, binding, budget, ArgumentNormalizationMode::Exact)
        .expect("synthetic restorer");
    let upstream = stream::iter(
        events
            .into_iter()
            .map(|event| Ok::<_, std::io::Error>(Bytes::from(reasoning_pair_frame(event)))),
    );
    let mut stream = wrap_sse_stream_with_restoration(upstream, state);
    let mut delivered = String::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.expect("synthetic output chunk");
        delivered.push_str(std::str::from_utf8(&chunk).expect("synthetic output UTF-8"));
    }
    delivered
}

fn reasoning_pair_events(text: &str, split: usize) -> Vec<Value> {
    let part = json!({"type":"reasoning_text","text":text});
    let item = json!({"id":"synthetic-pair-reasoning","type":"reasoning","status":"completed","summary":[],"content":[part]});
    vec![
        json!({"type":"response.output_item.added","output_index":0,"item":{"id":"synthetic-pair-reasoning","type":"reasoning","status":"in_progress","summary":[],"content":[]}}),
        json!({"type":"response.content_part.added","output_index":0,"content_index":0,"item_id":"synthetic-pair-reasoning","part":{"type":"reasoning_text","text":""}}),
        json!({"type":"response.reasoning_text.delta","output_index":0,"content_index":0,"item_id":"synthetic-pair-reasoning","delta":&text[..split]}),
        json!({"type":"response.reasoning_text.delta","output_index":0,"content_index":0,"item_id":"synthetic-pair-reasoning","delta":&text[split..]}),
        json!({"type":"response.reasoning_text.done","output_index":0,"content_index":0,"item_id":"synthetic-pair-reasoning","text":text}),
        json!({"type":"response.content_part.done","output_index":0,"content_index":0,"item_id":"synthetic-pair-reasoning","part":part}),
        json!({"type":"response.output_item.done","output_index":0,"item":item}),
        json!({"type":"response.completed","response":{"id":"synthetic-pair-response","object":"response","status":"completed","output":[item]}}),
    ]
}

#[tokio::test]
async fn reasoning_pair_sse_releases_exact_content_through_all_snapshots() {
    for text in [
        "rredact-custom-",
        "rredact-custom-00000000000000000000-ai",
        "rredact-custom-rredact-custom-00000000000000000000-ai-ai",
    ] {
        for split in 0..=text.len() {
            let delivered = reasoning_pair_delivered(reasoning_pair_events(text, split)).await;
            assert!(
                delivered.contains("event: response.completed\n"),
                "supported reasoning content must reach final release"
            );
            assert!(
                !delivered.contains("event: response.failed\n"),
                "supported reasoning content must not be scanned"
            );
            let mut deltas = String::new();
            for data in delivered
                .lines()
                .filter_map(|line| line.strip_prefix("data: "))
            {
                let event: Value = serde_json::from_str(data).expect("synthetic delivered event");
                if event["type"] == "response.reasoning_text.delta" {
                    deltas.push_str(event["delta"].as_str().expect("synthetic reasoning delta"));
                }
            }
            assert!(
                deltas == text,
                "reasoning content deltas must be delivered exactly once"
            );
        }
    }
}

#[tokio::test]
async fn reasoning_pair_sse_rejects_raw_content_changes_at_each_boundary() {
    for changed in [2, 4, 5, 6, 7] {
        let mut events = reasoning_pair_events("synthetic-safe", 5);
        match changed {
            2 => events[2]["delta"] = json!("changed"),
            4 => events[4]["text"] = json!("changed"),
            5 => events[5]["part"]["text"] = json!("changed"),
            6 => events[6]["item"]["content"][0]["text"] = json!("changed"),
            _ => events[7]["response"]["output"][0]["content"][0]["text"] = json!("changed"),
        }
        let delivered = reasoning_pair_delivered(events).await;
        assert!(
            !delivered.contains("event: response.completed\n"),
            "raw reasoning content mismatch must withhold completion"
        );
        assert!(
            delivered.contains("event: response.failed\n"),
            "raw reasoning content mismatch must fail safely"
        );
    }
}

#[tokio::test]
async fn reasoning_pair_sse_rejects_unsupported_parts_and_strict_siblings() {
    let marker = "rredact-custom-00000000000000000000-ai";
    for case in 0..9 {
        let mut events = reasoning_pair_events("synthetic-safe", 5);
        match case {
            0 => events[1]["part"]["type"] = json!("summary_text"),
            1 => events[5]["part"]["type"] = json!("summary_text"),
            2 => events[5]["part"]["text"] = json!(7),
            3 => events[5]["part"]["future_text"] = json!(marker),
            4 => events[6]["item"]["metadata"] = json!({"text":marker}),
            5 => events[2]["metadata"] = json!({"text":marker}),
            6 => {
                events[2][marker] = json!(null);
            }
            7 => events[7]["response"]["output"][0]["content"][0]["type"] = json!("summary_text"),
            _ => {
                events[7]["response"]["output"][0]["summary"] =
                    json!([{"type":"reasoning_text","text":"synthetic-safe"}])
            }
        }
        let delivered = reasoning_pair_delivered(events).await;
        if (3..=6).contains(&case) {
            assert!(
                delivered.contains("event: response.completed\n")
                    && !delivered.contains("event: response.failed\n"),
                "literal_siblings_rejected"
            );
            continue;
        }
        assert!(
            !delivered.contains("event: response.completed\n"),
            "invalid_reasoning_shape_must_withhold_completion"
        );
        assert!(
            delivered.contains("event: response.failed\n"),
            "strict reasoning neighbors must fail safely"
        );
        assert!(
            !delivered.contains(marker),
            "rejected sibling data must not be reflected"
        );
    }
}

#[test]
fn reasoning_pair_added_events_reject_wrong_part_before_any_release() {
    let marker = "rredact-custom-00000000000000000000-ai";
    for event in [
        json!({"type":"response.reasoning_summary_part.added","output_index":0,"summary_index":0,"part":{"type":"refusal","refusal":marker}}),
        json!({"type":"response.output_item.added","output_index":0,"item":{"id":"synthetic-pair-reasoning","type":"reasoning","summary":[{"type":"refusal","refusal":marker}]}}),
        json!({"type":"response.reasoning_summary_part.added","output_index":0,"summary_index":0,"part":{"type":"reasoning_text","text":"synthetic-safe"}}),
        json!({"type":"response.content_part.added","output_index":0,"content_index":0,"part":{"type":"summary_text","text":"synthetic-safe"}}),
    ] {
        let (lease, binding, budget) =
            reference_format_strict_fixture().expect("synthetic fixture");
        let mut state =
            SseRestorationState::new(lease, binding, budget, ArgumentNormalizationMode::Exact)
                .expect("synthetic state");
        assert!(
            state
                .process_sse_block(&reasoning_pair_frame(event))
                .is_err(),
            "wrong added part must be rejected before any snapshot bytes can escape"
        );
    }
}

#[tokio::test]
async fn reasoning_pair_sse_item_only_content_snapshot_must_match_completion() {
    let raw_item = json!({"id":"synthetic-pair-reasoning","type":"reasoning","summary":[],"content":[{"type":"reasoning_text","text":"synthetic-first"}]});
    let mut final_item = raw_item.clone();
    final_item["content"][0]["text"] = json!("synthetic-changed");
    let delivered = reasoning_pair_delivered(vec![
        json!({"type":"response.output_item.done","output_index":0,"item":raw_item}),
        json!({"type":"response.completed","response":{"id":"synthetic-pair-response","status":"completed","output":[final_item]}}),
    ]).await;
    assert!(
        !delivered.contains("event: response.completed\n"),
        "raw item-only content must reconcile before completion release"
    );
    assert!(
        delivered.contains("event: response.failed\n"),
        "raw item-only content mismatch must fail safely"
    );
}

#[tokio::test]
async fn reasoning_pair_sse_double_arrays_preserve_tool_commit_barrier() {
    let summary = "synthetic summary rredact-custom-";
    let content = "synthetic 🧪 rredact-custom-00000000000000000000-ai";
    for tool_first in [false, true] {
        for case in 0..5 {
            let summary_part = json!({"type":"summary_text","text":summary});
            let content_part = json!({"type":"reasoning_text","text":content});
            let item = json!({"id":"synthetic-double-reasoning","type":"reasoning","status":"completed",
                "summary":[summary_part],"content":[content_part]});
            let mut final_item = item.clone();
            match case {
                1 => final_item["summary"][0]["text"] = json!("synthetic changed summary"),
                2 => final_item["content"][0]["text"] = json!("synthetic changed content"),
                _ => {}
            }
            let arguments = if case == 3 {
                json!({"value":"rredact-custom-"}).to_string()
            } else {
                "{}".to_owned()
            };
            let tool = json!({"id":"synthetic-double-tool","type":"function_call","status":"completed",
                "call_id":"synthetic-double-call","name":"synthetic_tool","arguments":arguments});
            let mut tool_added = json!({"type":"response.output_item.added","output_index":1,
                "item":{"id":"synthetic-double-tool","type":"function_call","status":"in_progress",
                "call_id":"synthetic-double-call","name":"synthetic_tool","arguments":""}});
            if case == 4 {
                tool_added["item"]["metadata"] =
                    json!({"text":"rredact-custom-00000000000000000000-ai"});
            }
            let mut events = vec![
                json!({"type":"response.output_item.added","output_index":0,"item":{
                    "id":"synthetic-double-reasoning","type":"reasoning","status":"in_progress","summary":[],"content":[]}}),
                json!({"type":"response.reasoning_summary_part.added","output_index":0,"summary_index":0,
                    "item_id":"synthetic-double-reasoning","part":{"type":"summary_text","text":""}}),
                json!({"type":"response.reasoning_summary_text.delta","output_index":0,"summary_index":0,
                    "item_id":"synthetic-double-reasoning","delta":summary}),
                json!({"type":"response.reasoning_summary_text.done","output_index":0,"summary_index":0,
                    "item_id":"synthetic-double-reasoning","text":summary}),
                json!({"type":"response.reasoning_summary_part.done","output_index":0,"summary_index":0,
                    "item_id":"synthetic-double-reasoning","part":summary_part}),
                json!({"type":"response.content_part.added","output_index":0,"content_index":0,
                    "item_id":"synthetic-double-reasoning","part":{"type":"reasoning_text","text":""}}),
                json!({"type":"response.reasoning_text.delta","output_index":0,"content_index":0,
                    "item_id":"synthetic-double-reasoning","delta":"synthetic 🧪 rredact-custom-"}),
                json!({"type":"response.reasoning_text.delta","output_index":0,"content_index":0,
                    "item_id":"synthetic-double-reasoning","delta":"00000000000000000000-ai"}),
                json!({"type":"response.reasoning_text.done","output_index":0,"content_index":0,
                    "item_id":"synthetic-double-reasoning","text":content}),
                json!({"type":"response.content_part.done","output_index":0,"content_index":0,
                    "item_id":"synthetic-double-reasoning","part":content_part}),
                json!({"type":"response.output_item.done","output_index":0,"item":item}),
            ];
            if tool_first {
                events.insert(0, tool_added);
            } else {
                events.push(tool_added);
            }
            events.extend([
                json!({"type":"response.function_call_arguments.delta","output_index":1,
                    "item_id":"synthetic-double-tool","delta":arguments}),
                json!({"type":"response.function_call_arguments.done","output_index":1,
                    "item_id":"synthetic-double-tool","arguments":arguments}),
                json!({"type":"response.output_item.done","output_index":1,"item":tool}),
                json!({"type":"response.completed","response":{"id":"synthetic-double-response","status":"completed",
                    "output":[final_item,tool]}}),
            ]);
            let delivered = reasoning_pair_delivered(events).await;
            assert!(
                delivered.contains("event: response.completed\n") == !matches!(case, 1 | 2),
                "only matching reasoning arrays and strict tools may release completion"
            );
            assert!(
                delivered.contains("event: response.failed\n") == matches!(case, 1 | 2),
                "invalid reasoning snapshots or tool neighbors must fail safely"
            );
            if matches!(case, 1 | 2) {
                assert!(
                    !delivered.contains("event: response.function_call_arguments.")
                        && !delivered.contains("\"type\":\"function_call\""),
                    "failed validation must withhold every executable tool frame"
                );
                continue;
            }
            let mut summary_deltas = String::new();
            let mut content_deltas = String::new();
            for data in delivered
                .lines()
                .filter_map(|line| line.strip_prefix("data: "))
            {
                let event: Value = serde_json::from_str(data).expect("synthetic delivered event");
                match event["type"].as_str() {
                    Some("response.reasoning_summary_text.delta") => summary_deltas
                        .push_str(event["delta"].as_str().expect("synthetic summary delta")),
                    Some("response.reasoning_text.delta") => content_deltas
                        .push_str(event["delta"].as_str().expect("synthetic content delta")),
                    Some("response.completed") => assert!(
                        event["response"]["output"][0] == item,
                        "final reasoning arrays must preserve exact independent leaf bytes"
                    ),
                    _ => {}
                }
            }
            assert!(
                summary_deltas == summary,
                "summary deltas must be delivered exactly once"
            );
            assert!(
                content_deltas == content,
                "content deltas must be delivered exactly once"
            );
        }
    }
}
