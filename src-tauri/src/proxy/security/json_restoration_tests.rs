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
    assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
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
        budget.used() >= 8 * plaintext.len(),
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
    for (item, field) in [
        (2, "reasoning_content"),
        (3, "input"),
        (3, "reasoning_content"),
    ] {
        assert!(staged.as_value()["output"][item][field] == plaintext);
    }
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
    assert_eq!(restored.restored_occurrences(), 8);
    drop(attempt);
    assert!(
        budget.used() >= 8 * plaintext.len(),
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
