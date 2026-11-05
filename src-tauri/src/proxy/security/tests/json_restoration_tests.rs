//! Synthetic JSON restoration ownership and publication regressions.
use super::super::{
    payload_buffer::test_support::{self, Action, Phase},
    payload_metadata::PayloadVec,
    response_restoration::restore_text_with_lease,
    tokenization::{TokenCategory, TokenFragment},
};
use super::*;
use std::{cell::Cell, rc::Rc};

fn fixture(budget: &PayloadBudget, raw: &str) -> (DlpTokenStore, DlpPreparedAttempt, String) {
    let store = DlpTokenStore::new();
    let mut lease = store
        .admit("codex", Some("json-owner-session"), 1)
        .expect("lease");
    lease
        .prepare_with_budget(
            &[TokenFragment {
                raw,
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
            "json-owner-provider",
            "openai_responses",
            "http://127.0.0.1:1/responses",
            Some("json-owner-account"),
        )
        .expect("binding");
    let attempt = DlpPreparedAttempt {
        lease,
        binding,
        budget: budget.clone(),
        endpoint_is_compact: false,
        upstream_json_protocol: Some(JsonCompletionProtocol::NativeResponses),
        json_completion: std::sync::atomic::AtomicU8::new(0),
        audit_state: std::sync::Arc::new(
            RequestAuditState::new(&AppType::Codex, Some("json-owner-session"), budget.clone())
                .expect("audit state"),
        ),
        initial_audit: std::sync::Arc::new(
            PendingRequestAudit::empty(budget.clone()).expect("initial audit"),
        ),
        pending_audit: PendingRequestAudit::empty(budget.clone()).expect("pending audit"),
        upstream_payload_owner: None,
        history_mapping: None,
    };
    (store, attempt, token)
}

#[tokio::test]
async fn json_final_serialization_capacity_failure_does_not_publish_cursor() {
    let budget = PayloadBudget::with_limit(1024 * 1024);
    let plaintext = "x".repeat(4096);
    let (store, attempt, token) = fixture(&budget, &plaintext);
    let response = json!({"id":"resp_json_capacity", "object":"response", "status":"completed", "output":[{
        "id":"msg_json_capacity", "type":"message", "role":"assistant", "status":"completed",
        "content":[{"type":"output_text", "text":token}]
    }]});
    let mut expected = response.clone();
    expected["output"][0]["content"][0]["text"] = Value::String(plaintext);
    let output_len = serde_json::to_vec(&expected)
        .expect("synthetic output")
        .len();
    let reached = Rc::new(Cell::new(false));
    let observed = reached.clone();
    let hook = test_support::install(move |event| {
        if event.phase == Phase::CapacityObserved && event.requested == output_len {
            observed.set(true);
            Action::ReportCapacity(output_len + 1)
        } else {
            Action::Continue
        }
    });
    let response = attempt
        .restore_client_http_response(
            Response::builder()
                .header("content-type", "application/json")
                .body(Body::from(response.to_string()))
                .expect("response"),
            ArgumentNormalizationMode::Exact,
            None,
        )
        .await;
    drop(hook);
    assert!(
        reached.get(),
        "failure must occur at final serialization capacity validation"
    );
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    let mut next = store
        .admit("codex", Some("json-owner-session"), 1)
        .expect("next lease");
    let binding = next
        .bind_actual_attempt(
            "json-owner-provider",
            "openai_responses",
            "http://127.0.0.1:1/responses",
            Some("json-owner-account"),
        )
        .expect("next binding");
    assert!(
        next.resume_completed_context(&binding, Some("resp_json_capacity"), &[], &[])
            .is_err(),
        "failed client serialization must not leave a resumable cursor"
    );
    drop(response);
    assert_eq!(
        budget.used(),
        0,
        "failed restoration releases all transient owners"
    );
}

#[tokio::test]
async fn reasoning_history_json_final_encoding_failure_never_publishes(
) -> Result<(), Box<dyn std::error::Error>> {
    let Some(_completion) = crate::proxy::security::history_test_process::enter()? else {
        return Ok(());
    };
    for active in [false, true] {
        for reasoning in [false, true] {
            for fail_allocation in [false, true] {
                let budget = PayloadBudget::with_limit(1024 * 1024);
                let (store, mut attempt, _) = fixture(&budget, "synthetic-unused-authority");
                if !active {
                    let empty = store.admit("codex", Some("json-owner-session"), 1)?;
                    attempt.binding = empty.bind_actual_attempt(
                        "json-owner-provider",
                        "openai_responses",
                        "http://127.0.0.1:1/responses",
                        Some("json-owner-account"),
                    )?;
                    attempt.lease = empty;
                }
                assert!(
                    attempt.has_restoration_lease() == active,
                    "JSON encoding fixture must cover the requested token-authority branch"
                );
                let observer = attempt.lease.fork_attempt()?;
                let binding = attempt.binding.clone();
                let item = if reasoning {
                    json!({"id":"synthetic-json-reasoning", "type":"reasoning", "summary":[
                        {"type":"summary_text", "text":"synthetic reasoning rredact-custom-"}
                    ]})
                } else {
                    json!({"id":"synthetic-json-message", "type":"message", "role":"assistant",
                    "status":"completed", "content":[
                        {"type":"output_text", "text":"synthetic output rredact-custom-", "annotations":[]}
                    ]})
                };
                let raw = json!({"id":"synthetic-json-encoding", "object":"response",
                    "status":"completed", "output":[item]});
                let replay = json!({"input":raw["output"]});
                // Warm the controller without publishing any response authority.
                let pending = observer
                    .capture_delivered_history(&binding, &raw)
                    .ok_or("synthetic history capture unavailable")?;
                drop(pending);
                assert!(
                    observer.history_projection(&binding, &replay).is_none(),
                    "warming optional capture must not authorize undelivered JSON history"
                );
                let history_before = store
                    .history_metadata_bytes_for_test()
                    .ok_or("synthetic history accounting unavailable")?;
                let output = serde_json::to_vec(&raw)?;
                let output_len = output.len();
                // Trailing whitespace distinguishes input buffering from final encoding.
                let input = format!("{}\n ", std::str::from_utf8(&output)?);
                let reached = Rc::new(Cell::new(false));
                let observed = reached.clone();
                let pending_seen = Rc::new(Cell::new(false));
                let captured = pending_seen.clone();
                let accounting = store.clone();
                let hook = test_support::install(move |event| {
                    let phase = if fail_allocation {
                        Phase::BeforeAllocation
                    } else {
                        Phase::CapacityObserved
                    };
                    if event.phase == phase && event.requested == output_len {
                        observed.set(true);
                        captured.set(
                            accounting
                                .history_metadata_bytes_for_test()
                                .is_some_and(|bytes| bytes > history_before),
                        );
                        if fail_allocation {
                            Action::FailAllocation
                        } else {
                            Action::ReportCapacity(output_len + 1)
                        }
                    } else {
                        Action::Continue
                    }
                });
                let response = attempt
                    .restore_client_http_response(
                        Response::builder()
                            .header("content-type", "application/json")
                            .body(Body::from(input))?,
                        ArgumentNormalizationMode::Exact,
                        None,
                    )
                    .await;
                drop(hook);
                assert!(reached.get(), "failure must reach final JSON encoding");
                assert!(
                    pending_seen.get(),
                    "eligible pending proof must precede encoding"
                );
                assert!(
                    response.status() == StatusCode::INTERNAL_SERVER_ERROR,
                    "failed encoding must not release a successful response"
                );
                assert!(
                    observer.history_projection(&binding, &replay).is_none(),
                    "failed encoding must not publish history proof"
                );
                assert!(
                    observer
                        .fork_attempt()?
                        .resume_completed_context(
                            &binding,
                            Some("synthetic-json-encoding"),
                            &[],
                            &[]
                        )
                        .is_err(),
                    "failed encoding must not publish a cursor"
                );
                assert!(
                    store.history_metadata_bytes_for_test() == Some(history_before),
                    "failed encoding must release pending proof owners"
                );
                drop(response);
                assert!(
                    budget.used() == 0,
                    "failed encoding must release transient owners"
                );
            }
        }
    }
    Ok(())
}

#[test]
fn json_restored_leaf_charge_survives_until_owner_drop() {
    let budget = PayloadBudget::with_limit(64 * 1024);
    let plaintext = "x".repeat(4096);
    let (_store, attempt, token) = fixture(&budget, &plaintext);
    let mut tokens = PayloadVec::new(budget.clone()).expect("token list");
    let mut count = 0;
    let restored =
        restore_text_with_lease(&token, &attempt.lease, &budget, &mut tokens, &mut count)
            .expect("restore");
    drop(tokens);
    assert!(
        budget.used() >= plaintext.len(),
        "retained restored text must retain its allocation charge"
    );
    drop(restored);
    drop(attempt);
    assert_eq!(budget.used(), 0);
}

#[test]
fn json_restored_leaves_cannot_bypass_aggregate_budget() {
    let budget = PayloadBudget::with_limit(10 * 1024);
    let plaintext = "x".repeat(4096);
    let (_store, attempt, token) = fixture(&budget, &plaintext);
    let mut tokens = PayloadVec::new(budget.clone()).expect("token list");
    let mut count = 0;
    let first = restore_text_with_lease(&token, &attempt.lease, &budget, &mut tokens, &mut count)
        .expect("first leaf");
    let second = restore_text_with_lease(&token, &attempt.lease, &budget, &mut tokens, &mut count)
        .expect("second leaf");
    assert!(
        restore_text_with_lease(&token, &attempt.lease, &budget, &mut tokens, &mut count).is_err(),
        "all retained restored leaves must participate in the shared ceiling"
    );
    drop((first, second, tokens, attempt));
    assert_eq!(budget.used(), 0);
}

#[test]
fn json_staged_tree_retains_nested_arguments_reasoning_and_all_leaf_charges() {
    let budget = PayloadBudget::with_limit(1024 * 1024);
    let plaintext = "x".repeat(4096);
    let (store, attempt, token) = fixture(&budget, &plaintext);
    let response = json!({"id":"resp_owned_tree", "status":"completed", "output":[
        {"id":"msg_owned", "type":"message", "content":[
            {"type":"output_text", "text":token}, {"type":"refusal", "refusal":token}]},
        {"id":"rs_owned", "type":"reasoning", "summary":[{"type":"summary_text", "text":token}]},
        {"id":"fc_owned", "type":"function_call", "call_id":"call_owned", "name":"fixture_tool",
         "arguments":json!({"nested":[token, {"leaf":token}], "number":17}).to_string(), "reasoning_content":token},
        {"id":"ct_owned", "type":"custom_tool_call", "call_id":"custom_owned", "name":"fixture_custom",
         "input":token, "reasoning_content":token}
    ]});
    let staged =
        stage_json_response(&response, false, &attempt.lease, &budget).expect("staged tree");
    assert!(
        budget.used() >= 5 * plaintext.len(),
        "all expanded leaves remain charged in the tree"
    );
    let args: Value = serde_json::from_str(
        staged.as_value()["output"][2]["arguments"]
            .as_str()
            .expect("arguments"),
    )
    .expect("nested JSON");
    assert!(args["nested"][0] == plaintext && args["nested"][1]["leaf"] == plaintext);
    assert_eq!(args["number"], 17);
    assert!(
        staged.as_value()["output"][3]["input"] == plaintext,
        "custom tool input must still restore"
    );
    for item in [2, 3] {
        assert!(
            staged.as_value()["output"][item]["reasoning_content"] == token,
            "reasoning must remain delivered exactly"
        );
    }
    assert!(
        staged.as_value()["output"][1]["summary"][0]["text"] == token,
        "reasoning summary must remain delivered exactly"
    );
    let mut next = store
        .admit("codex", Some("json-owner-session"), 1)
        .expect("next");
    let next_binding = next
        .bind_actual_attempt(
            "json-owner-provider",
            "openai_responses",
            "http://127.0.0.1:1/responses",
            Some("json-owner-account"),
        )
        .expect("binding");
    assert!(
        next.resume_completed_context(&next_binding, Some("resp_owned_tree"), &[], &[])
            .is_err(),
        "staging publishes no context"
    );
    let witness = CompletionWitness::from_verified_completion(true).expect("completion");
    let restored = staged
        .publish(&attempt.lease, &attempt.binding, &witness, &budget)
        .expect("publication");
    assert!(
        restored.restored_occurrences() == 5,
        "only nonreasoning leaves restore"
    );
    drop(attempt);
    assert!(
        budget.used() >= 5 * plaintext.len(),
        "published restored tree still owns its allocation charge"
    );
    assert!(next
        .resume_completed_context(&next_binding, Some("resp_owned_tree"), &[], &[])
        .is_ok());
    drop(restored);
    assert_eq!(budget.used(), 0);
}

#[test]
fn json_staging_rejects_aggregate_growth_before_building_unowned_value_leaves() {
    let budget = PayloadBudget::with_limit(10 * 1024);
    let plaintext = "x".repeat(4096);
    let (_store, attempt, token) = fixture(&budget, &plaintext);
    let response = json!({"id":"resp_aggregate", "output":[{"type":"message", "content":[
        {"type":"output_text", "text":token}, {"type":"output_text", "text":token}, {"type":"output_text", "text":token}
    ]}]});
    assert!(stage_json_response(&response, false, &attempt.lease, &budget).is_err());
    assert!(
        response["output"][0]["content"][0]["text"] == token,
        "source remains immutable on budget failure"
    );
    drop(attempt);
    assert_eq!(budget.used(), 0);
}

#[tokio::test]
async fn json_http_success_publishes_only_prepared_output_and_retains_charge_through_last_clone() {
    let budget = PayloadBudget::with_limit(1024 * 1024);
    let plaintext = "x".repeat(4096);
    let (store, attempt, token) = fixture(&budget, &plaintext);
    let response = json!({"id":"resp_json_success", "object":"response", "status":"completed", "output":[{
        "id":"msg_success", "type":"message", "role":"assistant", "status":"completed", "content":[{"type":"output_text", "text":token}]
    }]});
    let response = attempt
        .restore_client_http_response(
            Response::builder()
                .header("content-type", "application/json")
                .body(Body::from(response.to_string()))
                .expect("response"),
            ArgumentNormalizationMode::Exact,
            None,
        )
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    let mut next = store
        .admit("codex", Some("json-owner-session"), 1)
        .expect("next");
    let binding = next
        .bind_actual_attempt(
            "json-owner-provider",
            "openai_responses",
            "http://127.0.0.1:1/responses",
            Some("json-owner-account"),
        )
        .expect("binding");
    assert!(next
        .resume_completed_context(&binding, Some("resp_json_success"), &[], &[])
        .is_ok());
    let bytes = axum::body::to_bytes(response.into_body(), 1024 * 1024)
        .await
        .expect("body");
    let decoded: Value = serde_json::from_slice(&bytes).expect("JSON");
    assert!(decoded["output"][0]["content"][0]["text"] == plaintext);
    let retained = budget.used();
    assert_eq!(
        retained,
        bytes.len(),
        "only the serialized output allocation remains charged"
    );
    let clone = bytes.clone();
    drop(bytes);
    assert_eq!(
        budget.used(),
        retained,
        "immutable sharing retains one charge"
    );
    drop(clone);
    assert_eq!(budget.used(), 0);
}

#[tokio::test]
async fn reasoning_pair_json_http_validates_content_and_publishes_only_success() {
    for case in 0..8 {
        let budget = PayloadBudget::with_limit(1024 * 1024);
        let (store, attempt, token) = fixture(&budget, "synthetic-unused-mapping");
        let mut item = json!({"id":"synthetic-pair-reasoning","type":"reasoning","status":"completed",
            "summary":[{"type":"summary_text","text":"rredact-custom-"}],
            "content":[{"type":"reasoning_text","text":token}]});
        match case {
            1 => item["content"][0]["type"] = json!("summary_text"),
            2 => item["summary"][0]["type"] = json!("reasoning_text"),
            3 => item["content"][0]["text"] = json!(7),
            4 => item["content"][0]["metadata"] = json!({"text":token}),
            5 => item["metadata"] = json!({"text":token}),
            6 => {
                item["content"][0][&token] = json!(null);
            }
            7 => item["content"][0] = json!({"type":"future_text","text":token}),
            _ => {}
        }
        let original = json!({"id":"synthetic-pair-json-response","object":"response","status":"completed","output":[item]});
        let response = attempt
            .restore_client_http_response(
                Response::builder()
                    .header("content-type", "application/json")
                    .body(Body::from(original.to_string()))
                    .expect("synthetic response"),
                ArgumentNormalizationMode::Exact,
                None,
            )
            .await;
        let expected = case == 0;
        assert!(
            response.status().is_success() == expected,
            "only exact reasoning content shapes may reach JSON delivery"
        );
        let bytes = axum::body::to_bytes(response.into_body(), 1024 * 1024)
            .await
            .expect("synthetic JSON body");
        let decoded: Value = serde_json::from_slice(&bytes).expect("synthetic JSON output");
        if expected {
            assert!(
                decoded == original,
                "delivered reasoning JSON must preserve exact text bytes"
            );
        } else {
            assert!(
                !std::str::from_utf8(&bytes)
                    .expect("synthetic UTF-8")
                    .contains(&token),
                "rejected nonleaf data must not appear in errors"
            );
        }
        let mut next = store
            .admit("codex", Some("json-owner-session"), 1)
            .expect("synthetic next lease");
        let binding = next
            .bind_actual_attempt(
                "json-owner-provider",
                "openai_responses",
                "http://127.0.0.1:1/responses",
                Some("json-owner-account"),
            )
            .expect("synthetic binding");
        assert!(
            next.resume_completed_context(&binding, Some("synthetic-pair-json-response"), &[], &[])
                .is_ok()
                == expected,
            "only successfully delivered JSON may publish a completed context"
        );
    }
}
