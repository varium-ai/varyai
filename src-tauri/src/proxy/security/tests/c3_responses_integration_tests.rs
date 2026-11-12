//! Real Responses HTTP facade contracts; lifecycle rejection is never a completion proof.
//! The parent registers this module and runs target RED before facade ownership changes.
use axum::{body::Body, http::StatusCode, response::Response};
use http_body_util::BodyExt;
use serde_json::{json, Value};

use super::{
    completed_context::{ActualAttemptBinding, CompletedCallSpec},
    http_restoration::{CLIENT_PROCESSING_ERROR_CODE, CLIENT_PROCESSING_ERROR_MESSAGE},
    response_restoration::ArgumentNormalizationMode,
    restoration_core::{LifecycleOperation, ResponseLifecycle},
    tokenization::{DlpTokenStore, TokenCategory, TokenFragment},
    unified_test_support::{ResponsesFixture, TestResult, CONTACT, SESSION},
};

const RESPONSE_ID: &str = "synthetic-c3-owned-response";
const CALL_ID: &str = "synthetic-c3-owned-call";
const TOOL_ID: &str = "synthetic-c3-owned-tool-item";
const TOOL_NAME: &str = "synthetic_nonexecuting_tool";
const OPAQUE: &str = "synthetic-c3-owned-opaque";
const ORDINARY: &str = "Synthetic ordinary input";
const REASONING: &str = "Synthetic reasoning stays literal";

#[derive(Clone, Copy)]
enum Framing {
    Json,
    Sse,
}

struct NativeCase {
    response: Value,
    expected: Value,
    events: Vec<Value>,
    expected_arguments: String,
}

fn native_case(text: &str, mapped: bool) -> TestResult<NativeCase> {
    // The escaped reference is visible only after parsing the JSON argument string.
    // Restoring the entire encoded argument string cannot satisfy this control.
    let argument_value = |text: &str| {
        json!({
            "value": text,
            "nested": [format!("Synthetic quoted \"value\" \\ tail {text}"), 7, true, null],
            "untouched": {"enabled": false}
        })
    };
    let arguments =
        serde_json::to_string(&argument_value(text))?.replace("rredact", "\\u0072redact");
    let expected_arguments = if mapped {
        serde_json::to_string(&argument_value(CONTACT))?
    } else {
        arguments.clone()
    };
    let tool = json!({
        "id": TOOL_ID, "type": "function_call", "call_id": CALL_ID,
        "name": TOOL_NAME, "arguments": arguments, "status": "completed",
        "encrypted_reasoning": OPAQUE,
        "unknown_extension": {"enabled": true, "ordinal": 3}
    });
    let message = json!({
        "id": "synthetic-c3-owned-message", "type": "message", "role": "assistant",
        "status": "completed", "content": [
            {"type": "output_text", "text": text, "annotations": []},
            {"type": "refusal", "refusal": text}
        ]
    });
    let reasoning = json!({
        "id": "synthetic-c3-owned-reasoning", "type": "reasoning", "status": "completed",
        "summary": [{"type": "summary_text", "text": REASONING}],
        "content": [{"type": "reasoning_text", "text": REASONING}],
        "encrypted_content": "synthetic-c3-owned-reasoning-opaque"
    });
    let response = json!({
        "id": RESPONSE_ID, "object": "response", "status": "completed",
        "output": [tool, message, reasoning],
        "unknown_extension": {"enabled": true, "ordinal": 9}
    });
    let mut expected = response.clone();
    expected["output"][0]["arguments"] = json!(expected_arguments);
    let delivered_text = if mapped { CONTACT } else { text };
    expected["output"][1]["content"][0]["text"] = json!(delivered_text);
    expected["output"][1]["content"][1]["refusal"] = json!(delivered_text);

    // Start with the tool barrier so every later text/terminal is withheld on failure.
    let mut added = tool.clone();
    added["arguments"] = json!("");
    added["status"] = json!("in_progress");
    let events = vec![
        json!({"type":"response.output_item.added", "output_index":0, "item":added}),
        json!({"type":"response.function_call_arguments.delta", "output_index":0,
            "item_id":TOOL_ID, "delta":arguments}),
        json!({"type":"response.function_call_arguments.done", "output_index":0,
            "item_id":TOOL_ID, "arguments":arguments}),
        json!({"type":"response.output_item.done", "output_index":0, "item":tool}),
        json!({"type":"response.output_text.delta", "output_index":1, "content_index":0,
            "item_id":"synthetic-c3-owned-message", "delta":text}),
        json!({"type":"response.output_text.done", "output_index":1, "content_index":0,
            "item_id":"synthetic-c3-owned-message", "text":text}),
        json!({"type":"response.refusal.delta", "output_index":1, "content_index":1,
            "item_id":"synthetic-c3-owned-message", "delta":text}),
        json!({"type":"response.refusal.done", "output_index":1, "content_index":1,
            "item_id":"synthetic-c3-owned-message", "refusal":text}),
        json!({"type":"response.reasoning_summary_text.delta", "output_index":2, "summary_index":0,
            "item_id":"synthetic-c3-owned-reasoning", "delta":REASONING}),
        json!({"type":"response.reasoning_summary_text.done", "output_index":2, "summary_index":0,
            "item_id":"synthetic-c3-owned-reasoning", "text":REASONING}),
        json!({"type":"response.reasoning_text.delta", "output_index":2, "content_index":0,
            "item_id":"synthetic-c3-owned-reasoning", "delta":REASONING}),
        json!({"type":"response.reasoning_text.done", "output_index":2, "content_index":0,
            "item_id":"synthetic-c3-owned-reasoning", "text":REASONING}),
        json!({"type":"response.output_item.done", "output_index":1, "item":message}),
        json!({"type":"response.output_item.done", "output_index":2, "item":reasoning}),
        json!({"type":"response.completed", "response":response}),
    ];
    Ok(NativeCase {
        response,
        expected,
        events,
        expected_arguments,
    })
}

fn sse_wire(events: &[Value]) -> TestResult<String> {
    let mut wire = String::new();
    for event in events {
        let kind = event["type"]
            .as_str()
            .ok_or("c3_owned_event_kind_missing")?;
        wire.push_str(&format!("event: {kind}\ndata: {event}\n\n"));
    }
    Ok(wire)
}

fn decode_events(wire: &[u8]) -> TestResult<Vec<Value>> {
    let text = std::str::from_utf8(wire)?;
    text.lines()
        .filter_map(|line| line.strip_prefix("data: "))
        .map(|data| serde_json::from_str(data).map_err(Into::into))
        .collect()
}

// Each probe starts with a fresh admitted lease: one claim must not authorize another.
fn assert_continuity(
    store: &DlpTokenStore,
    binding: &ActualAttemptBinding,
    reference: &str,
    arguments: &str,
    visible: bool,
    mapped: bool,
) -> TestResult {
    for probe in 0..6 {
        let mut observer =
            store.admit_published_policy("codex", Some(SESSION), binding.policy_revision)?;
        assert!(
            observer.is_empty(),
            "c3_owned_observer_has_ambient_authority"
        );
        let call = CompletedCallSpec {
            call_id: CALL_ID,
            item_type: "function_call",
            name: TOOL_NAME,
            payload: arguments,
            reasoning: None,
            encrypted_reasoning: Some(OPAQUE),
        };
        let result = match probe {
            0 => observer.resume_completed_context(binding, Some(RESPONSE_ID), &[], &[]),
            1 => observer.resume_completed_context(binding, None, &[call], &[]),
            2 => observer.resume_completed_context(binding, None, &[], &[OPAQUE]),
            3 => observer.verify_output_call_ids(binding, Some(RESPONSE_ID), &[CALL_ID]),
            4 => observer.resume_completed_tokens(binding, &[reference]),
            _ => observer.resume_matching_completed_tokens(binding, &[reference]),
        };
        let should_succeed = probe == 5 || visible && (probe < 4 || mapped);
        assert!(
            result.is_ok() == should_succeed,
            "c3_owned_continuity_boundary_changed"
        );
        let resolves = visible && mapped && probe != 3;
        assert!(
            observer.resolve(reference).is_ok() == resolves,
            "c3_owned_mapping_authority_leaked"
        );
        assert!(
            observer.authorized_tokens()?.len() == usize::from(resolves),
            "c3_owned_dependency_lineage_changed"
        );
    }
    Ok(())
}

async fn facade_case(
    framing: Framing,
    mapped: bool,
    rejection: Option<LifecycleOperation>,
) -> TestResult {
    let fixture = ResponsesFixture::new();
    let mut request = json!({"input":if mapped { CONTACT } else { ORDINARY }, "stream":matches!(framing, Framing::Sse)});
    let attempt = fixture.prepare(&mut request, false, Some(SESSION))?;
    assert!(
        attempt.has_active_tokens() == mapped,
        "c3_owned_request_mapping_precondition"
    );
    let binding = attempt.binding.clone();
    let text = request["input"]
        .as_str()
        .ok_or("c3_owned_request_text_missing")?;
    let mut decoy =
        fixture
            .store
            .admit_published_policy("codex", Some(SESSION), binding.policy_revision)?;
    let decoy_reference = decoy.prepare(
        &[TokenFragment {
            raw: "Synthetic uncompleted dependency",
            whole_span_categories: &[TokenCategory::Custom],
        }],
        &[],
        |references| Ok(references[0].clone()),
    )?;
    let reference = if mapped { text } else { &decoy_reference };
    let case = native_case(text, mapped)?;
    assert_continuity(
        &fixture.store,
        &binding,
        reference,
        &case.expected_arguments,
        false,
        mapped,
    )?;
    let (content_type, wire) = match framing {
        Framing::Json => ("application/json", serde_json::to_vec(&case.response)?),
        Framing::Sse => ("text/event-stream", sse_wire(&case.events)?.into_bytes()),
    };
    // Current-thread tests keep this !Send guard active through actual body polling.
    let guard = rejection.map(ResponseLifecycle::reject_operation_for_test);
    let delivered = attempt
        .restore_client_http_response(
            Response::builder()
                .header("content-type", content_type)
                .body(Body::from(wire))?,
            ArgumentNormalizationMode::Exact,
            None,
        )
        .await;
    let status = delivered.status();
    let bytes = delivered.into_body().collect().await?.to_bytes();
    drop(guard);
    let expected_visible = rejection.is_none();
    // Probe before client-output assertions so a missing owner cannot hide publication leaks.
    assert_continuity(
        &fixture.store,
        &binding,
        reference,
        &case.expected_arguments,
        expected_visible,
        mapped,
    )?;
    match framing {
        Framing::Json => {
            let value: Value = serde_json::from_slice(&bytes)?;
            if expected_visible {
                assert!(
                    status == StatusCode::OK && value == case.expected,
                    "c3_owned_json_native_semantics_changed"
                );
                let args: Value = serde_json::from_str(
                    value["output"][0]["arguments"]
                        .as_str()
                        .ok_or("c3_owned_json_args_missing")?,
                )?;
                let expected_args: Value = serde_json::from_str(&case.expected_arguments)?;
                assert!(
                    args == expected_args,
                    "c3_owned_json_arguments_not_string_leaves"
                );
            } else {
                assert!(
                    status == StatusCode::INTERNAL_SERVER_ERROR,
                    "c3_owned_json_lifecycle_failure_not_propagated"
                );
                assert!(
                    value["error"]["code"] == CLIENT_PROCESSING_ERROR_CODE
                        && value["error"]["message"] == CLIENT_PROCESSING_ERROR_MESSAGE,
                    "c3_owned_json_failure_not_sanitized"
                );
                assert!(
                    value.get("output").is_none() && value["status"] != "completed",
                    "c3_owned_json_failure_released_success_or_tool"
                );
                assert!(
                    !std::str::from_utf8(&bytes)?.contains(CONTACT),
                    "c3_owned_json_failure_released_private_text"
                );
            }
        }
        Framing::Sse => {
            let events = decode_events(&bytes)?;
            if expected_visible {
                assert!(
                    status == StatusCode::OK && events.len() == case.events.len(),
                    "c3_owned_sse_native_event_count_changed"
                );
                for (delivered, original) in events.iter().zip(&case.events) {
                    for key in [
                        "type",
                        "output_index",
                        "content_index",
                        "summary_index",
                        "item_id",
                    ] {
                        assert!(
                            delivered.get(key) == original.get(key),
                            "c3_owned_sse_kind_or_identity_changed"
                        );
                    }
                    if original["type"]
                        .as_str()
                        .is_some_and(|kind| kind.starts_with("response.reasoning"))
                    {
                        assert!(delivered == original, "c3_owned_sse_reasoning_changed");
                    }
                    if original["type"] == "response.refusal.done" {
                        assert!(
                            delivered["refusal"].as_str()
                                == Some(if mapped { CONTACT } else { text }),
                            "c3_owned_sse_refusal_policy_changed"
                        );
                    }
                }
                assert!(
                    events.last().and_then(|event| event.get("response")) == Some(&case.expected),
                    "c3_owned_sse_completed_snapshot_changed"
                );
            } else {
                assert!(
                    status == StatusCode::OK,
                    "c3_owned_sse_transport_contract_changed"
                );
                assert!(
                    events
                        .iter()
                        .any(|event| event["type"] == "response.failed"),
                    "c3_owned_sse_lifecycle_failure_not_propagated"
                );
                assert!(
                    !events.iter().any(|event| {
                        event["type"] == "response.completed"
                            || event["type"].as_str().is_some_and(|kind| {
                                kind.starts_with("response.function_call_arguments")
                            })
                            || event.pointer("/item/type").and_then(Value::as_str)
                                == Some("function_call")
                    }),
                    "c3_owned_sse_failure_released_tool_or_success_terminal"
                );
                assert!(
                    !std::str::from_utf8(&bytes)?.contains(CONTACT),
                    "c3_owned_sse_failure_released_private_text"
                );
            }
        }
    }
    Ok(())
}

macro_rules! facade_tests {
    ($framing:ident, $mapped:expr, $( $name:ident => $operation:expr ),+ $(,)?) => { $(
        #[tokio::test(flavor = "current_thread")]
        #[serial_test::serial]
        async fn $name() -> TestResult {
            facade_case(Framing::$framing, $mapped, $operation).await
        }
    )+ };
}

facade_tests!(Json, true,
    c3_responses_integration_control_json_mapped => None,
    c3_responses_integration_reject_json_leaf_mapped => Some(LifecycleOperation::Done),
    c3_responses_integration_reject_json_finish_mapped => Some(LifecycleOperation::Finish),
    c3_responses_integration_reject_json_original_mapped => Some(LifecycleOperation::ObserveOriginal),
);
facade_tests!(Json, false,
    c3_responses_integration_control_json_zero => None,
    c3_responses_integration_reject_json_leaf_zero => Some(LifecycleOperation::Done),
    c3_responses_integration_reject_json_finish_zero => Some(LifecycleOperation::Finish),
    c3_responses_integration_reject_json_original_zero => Some(LifecycleOperation::ObserveOriginal),
);
facade_tests!(Sse, true,
    c3_responses_integration_control_sse_mapped => None,
    c3_responses_integration_reject_sse_delta_mapped => Some(LifecycleOperation::Delta),
    c3_responses_integration_reject_sse_done_mapped => Some(LifecycleOperation::Done),
    c3_responses_integration_reject_sse_hold_mapped => Some(LifecycleOperation::Hold),
    c3_responses_integration_reject_sse_finish_mapped => Some(LifecycleOperation::Finish),
    c3_responses_integration_reject_sse_original_mapped => Some(LifecycleOperation::ObserveOriginal),
);
facade_tests!(Sse, false,
    c3_responses_integration_control_sse_zero => None,
    c3_responses_integration_reject_sse_delta_zero => Some(LifecycleOperation::Delta),
    c3_responses_integration_reject_sse_done_zero => Some(LifecycleOperation::Done),
    c3_responses_integration_reject_sse_hold_zero => Some(LifecycleOperation::Hold),
    c3_responses_integration_reject_sse_finish_zero => Some(LifecycleOperation::Finish),
    c3_responses_integration_reject_sse_original_zero => Some(LifecycleOperation::ObserveOriginal),
);

// This boundary runs before any converter can replace the original protocol tree.
// A rejection observed only by final Responses staging is too late for this contract.
async fn preconversion_json_case(chat: bool, mapped: bool) -> TestResult {
    use super::{completion_json::JsonCompletionProtocol, unified_test_support::PRINCIPAL};
    let fixture = ResponsesFixture::new();
    let mut body =
        json!({"model":"synthetic-model", "input":if mapped { CONTACT } else { ORDINARY }});
    let request = fixture.admit(&mut body, false, Some(SESSION))?;
    let attempt = request
        .prepare_attempt(
            "synthetic-original-provider",
            if chat { "openai_chat" } else { "anthropic" },
            "http://127.0.0.1:1/synthetic-original",
            Some(PRINCIPAL),
            &mut body,
            |candidate| Ok(candidate.clone()),
        )
        .map_err(|_| "c3_original_prepare_failed")?;
    let text = body["input"].as_str().ok_or("c3_original_input_missing")?;
    let original = if chat {
        json!({"id":"synthetic-original-chat", "object":"chat.completion", "model":"synthetic-model",
            "choices":[{"index":0, "message":{"role":"assistant", "content":text}, "finish_reason":"stop"}]})
    } else {
        json!({"id":"synthetic-original-anthropic", "type":"message", "role":"assistant", "model":"synthetic-model",
            "content":[{"type":"text", "text":text}], "stop_reason":"end_turn",
            "usage":{"input_tokens":1,"output_tokens":1}})
    };
    let raw = serde_json::to_string(&original)?;
    let guard = ResponseLifecycle::reject_operation_for_test(LifecycleOperation::ObserveOriginal);
    let result = attempt.verify_upstream_json_completion(
        if chat {
            JsonCompletionProtocol::ChatCompletions
        } else {
            JsonCompletionProtocol::AnthropicMessages
        },
        &raw,
    );
    drop(guard);
    assert!(
        result.is_err(),
        "c3_original_rejection_must_precede_conversion"
    );
    // Failure remains sticky after the injection is removed; no second observation repairs it.
    assert!(
        attempt
            .verify_upstream_json_completion(
                if chat {
                    JsonCompletionProtocol::ChatCompletions
                } else {
                    JsonCompletionProtocol::AnthropicMessages
                },
                &raw,
            )
            .is_err(),
        "c3_original_failure_must_remain_sticky"
    );
    Ok(())
}

macro_rules! preconversion_json_tests {
    ($( $name:ident => ($chat:expr, $mapped:expr) ),+ $(,)?) => { $(
        #[tokio::test(flavor = "current_thread")]
        #[serial_test::serial]
        async fn $name() -> TestResult { preconversion_json_case($chat, $mapped).await }
    )+ };
}
preconversion_json_tests!(
    c3_json_preconversion_chat_mapped => (true, true),
    c3_json_preconversion_chat_zero => (true, false),
    c3_json_preconversion_anthropic_mapped => (false, true),
    c3_json_preconversion_anthropic_zero => (false, false),
);

fn preconversion_sse_case(chat: bool) -> TestResult {
    use super::{completion_json::JsonCompletionProtocol, unified_test_support::PRINCIPAL};
    let fixture = ResponsesFixture::new();
    let mut body = json!({"model":"synthetic-model", "input":CONTACT});
    let request = fixture.admit(&mut body, false, Some(SESSION))?;
    let attempt = request
        .prepare_attempt(
            "synthetic-original-provider",
            if chat { "openai_chat" } else { "anthropic" },
            "http://127.0.0.1:1/synthetic-original",
            Some(PRINCIPAL),
            &mut body,
            |candidate| Ok(candidate.clone()),
        )
        .map_err(|_| "c3_original_prepare_failed")?;
    let text = body["input"].as_str().ok_or("c3_original_input_missing")?;
    let wire = if chat {
        format!(
            "data: {}\n\ndata: [DONE]\n\n",
            json!({"id":"synthetic-original-chat", "object":"chat.completion.chunk",
            "choices":[{"index":0,"delta":{"role":"assistant","content":text},"finish_reason":"stop"}]})
        )
    } else {
        let events = [
            json!({"type":"message_start","message":{"id":"synthetic-original-anthropic","type":"message","role":"assistant","model":"synthetic-model","content":[],"usage":{"input_tokens":1,"output_tokens":0}}}),
            json!({"type":"content_block_start","index":0,"content_block":{"type":"text","text":text}}),
            json!({"type":"content_block_stop","index":0}),
            json!({"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":1}}),
            json!({"type":"message_stop"}),
        ];
        events
            .iter()
            .map(|event| {
                format!(
                    "event: {}\ndata: {event}\n\n",
                    event["type"].as_str().unwrap_or("")
                )
            })
            .collect()
    };
    let protocol = if chat {
        JsonCompletionProtocol::ChatCompletions
    } else {
        JsonCompletionProtocol::AnthropicMessages
    };
    let guard = ResponseLifecycle::reject_operation_for_test(LifecycleOperation::ObserveOriginal);
    let result = attempt.verify_upstream_sse_completion(protocol, &wire);
    drop(guard);
    assert!(
        result.is_err(),
        "c3_original_sse_rejection_must_precede_aggregation"
    );
    Ok(())
}
#[test]
#[serial_test::serial]
fn c3_json_preconversion_buffered_chat_sse() -> TestResult {
    preconversion_sse_case(true)
}
#[test]
#[serial_test::serial]
fn c3_json_preconversion_buffered_anthropic_sse() -> TestResult {
    preconversion_sse_case(false)
}

#[test]
#[serial_test::serial]
fn c3_json_preconversion_unobserved_owner_cannot_stage() -> TestResult {
    use super::{
        protocol_adapters::responses::{stage_json_response_bound, ResponsesJsonInspection},
        restoration_contract::{ClientProtocol, ProtocolPair, UpstreamProtocol},
        restoration_core::AttemptProvenance,
    };
    use crate::proxy::protocol_bridge::Protocol;
    let fixture = ResponsesFixture::new();
    let mut body = json!({"input":ORDINARY});
    let attempt = fixture.prepare(&mut body, false, Some(SESSION))?;
    let pair = ProtocolPair::new(
        ClientProtocol::new(Protocol::Responses),
        UpstreamProtocol::new(Protocol::Responses),
    );
    let provenance = AttemptProvenance::new(&attempt.budget)?;
    let inspection = ResponsesJsonInspection::begin(
        &attempt.lease,
        &attempt.binding,
        &attempt.budget,
        pair,
        provenance.clone(),
    )
    .map_err(|_| "c3_inspection_begin_failed")?;
    let response = native_case(ORDINARY, false)?.response;
    assert!(
        stage_json_response_bound(
            &response,
            false,
            &attempt.lease,
            &attempt.binding,
            &attempt.budget,
            pair,
            &provenance,
            inspection
        )
        .is_err(),
        "c3_unobserved_owner_must_not_stage"
    );
    Ok(())
}

#[derive(Clone, Copy)]
enum JsonHandoffCase {
    Control,
    Client,
    Upstream,
    Attempt,
    Store,
    Namespace,
    Generation,
    Policy,
    Provider,
    Principal,
    Operation,
}

// Synthetic owner-level checks complement, but do not replace, real prepared-attempt tests.
fn json_handoff_case(case: JsonHandoffCase, mapped: bool) -> TestResult {
    use super::{
        payload_budget::PayloadBudget,
        protocol_adapters::responses::{stage_json_response_bound, ResponsesJsonInspection},
        restoration_contract::{
            ClientProtocol, OriginalResponseObservation, ProtocolPair, UpstreamProtocol,
        },
        restoration_core::AttemptProvenance,
    };
    use crate::proxy::protocol_bridge::Protocol;
    let fixture = ResponsesFixture::new();
    let mut body = json!({"input":if mapped { CONTACT } else { ORDINARY }});
    let request = fixture.admit(&mut body, false, Some(SESSION))?;
    let descriptor = crate::proxy::providers::CodexAdapter::resolve_dlp_operation_descriptor(
        "https://synthetic.invalid/v1",
        "/responses",
        "openai_responses",
        "https://synthetic.invalid/v1/responses",
        false,
    )
    .ok_or("c3_handoff_descriptor_missing")?;
    let (attempt, input_owner) = request
        .prepare_owned_attempt_with_operation_for_proxy(
            Some(&descriptor),
            super::unified_test_support::PROVIDER,
            "openai_responses",
            super::unified_test_support::TARGET,
            Some(super::unified_test_support::PRINCIPAL),
            &mut body,
            Ok,
        )
        .map_err(|_| "c3_handoff_prepare_failed")?;
    drop(input_owner);
    let text = body["input"].as_str().ok_or("c3_handoff_input_missing")?;
    let native = native_case(text, mapped)?;
    let budget = PayloadBudget::with_limit(4 * 1024 * 1024);
    let pair = ProtocolPair::new(
        ClientProtocol::new(Protocol::Responses),
        UpstreamProtocol::new(Protocol::Responses),
    );
    let provenance = AttemptProvenance::new(&budget)?;
    let baseline = budget.used();
    let mut inspection = ResponsesJsonInspection::begin(
        &attempt.lease,
        &attempt.binding,
        &budget,
        pair,
        provenance.clone(),
    )
    .map_err(|_| "c3_handoff_begin_failed")?;
    let original = OriginalResponseObservation::json(
        &native.response,
        pair.upstream(),
        provenance.clone(),
        budget.clone(),
    )
    .map_err(|_| "c3_handoff_observation_failed")?;
    inspection
        .observe_original(&original)
        .map_err(|_| "c3_handoff_observe_failed")?;
    drop(original);
    let mut target_binding = attempt.binding.clone();
    let mut target_pair = pair;
    let mut target_attempt = provenance.clone();
    match case {
        JsonHandoffCase::Control => {}
        JsonHandoffCase::Client => {
            target_pair = ProtocolPair::new(ClientProtocol::new(Protocol::Chat), pair.upstream())
        }
        JsonHandoffCase::Upstream => {
            target_pair = ProtocolPair::new(pair.client(), UpstreamProtocol::new(Protocol::Chat))
        }
        // Binding equality is insufficient: another attempt still cannot consume this owner.
        JsonHandoffCase::Attempt => target_attempt = AttemptProvenance::new(&budget)?,
        JsonHandoffCase::Store => target_binding.store_id ^= 1,
        JsonHandoffCase::Namespace => target_binding.namespace ^= 1,
        JsonHandoffCase::Generation => target_binding.generation[0] ^= 1,
        JsonHandoffCase::Policy => target_binding.policy_revision ^= 1,
        JsonHandoffCase::Provider => target_binding.provider_fingerprint[0] ^= 1,
        JsonHandoffCase::Principal => {
            target_binding.reusable_principal = !target_binding.reusable_principal
        }
        JsonHandoffCase::Operation => {
            assert!(
                target_binding.operation_lineage.is_some(),
                "c3_handoff_operation_donor_missing"
            );
            target_binding.operation_lineage = None;
        }
    }
    let result = stage_json_response_bound(
        &native.response,
        false,
        &attempt.lease,
        &target_binding,
        &budget,
        target_pair,
        &target_attempt,
        inspection,
    );
    if matches!(case, JsonHandoffCase::Control) {
        let staged = result.map_err(|_| "c3_handoff_control_stage_failed")?;
        assert!(
            staged.as_value() == &native.expected,
            "c3_handoff_control_restoration_changed"
        );
        drop(staged);
    } else {
        assert!(
            result.is_err(),
            "c3_handoff_foreign_identity_must_not_stage"
        );
        drop(result);
    }
    drop(target_attempt);
    assert_eq!(
        budget.used(),
        baseline,
        "c3_handoff_retained_owner_after_consumption"
    );
    assert_continuity(
        &fixture.store,
        &attempt.binding,
        text,
        &native.expected_arguments,
        false,
        mapped,
    )?;
    drop(provenance);
    assert_eq!(budget.used(), 0, "c3_handoff_budget_not_released");
    Ok(())
}

macro_rules! json_handoff_tests {
    ($( $name:ident => ($case:ident, $mapped:expr) ),+ $(,)?) => { $(
        #[test]
        #[serial_test::serial]
        fn $name() -> TestResult { json_handoff_case(JsonHandoffCase::$case, $mapped) }
    )+ };
}
json_handoff_tests!(
    c3_json_preconversion_handoff_control_mapped => (Control, true),
    c3_json_preconversion_handoff_control_zero => (Control, false),
    c3_json_preconversion_handoff_wrong_client => (Client, true),
    c3_json_preconversion_handoff_wrong_upstream => (Upstream, true),
    c3_json_preconversion_handoff_same_binding_other_attempt => (Attempt, true),
    c3_json_preconversion_handoff_other_attempt_zero => (Attempt, false),
    c3_json_preconversion_handoff_wrong_store => (Store, true),
    c3_json_preconversion_handoff_wrong_namespace => (Namespace, true),
    c3_json_preconversion_handoff_wrong_generation => (Generation, true),
    c3_json_preconversion_handoff_wrong_policy => (Policy, true),
    c3_json_preconversion_handoff_wrong_provider => (Provider, true),
    c3_json_preconversion_handoff_wrong_principal => (Principal, true),
    c3_json_preconversion_handoff_wrong_operation => (Operation, true),
);

#[derive(Clone, Copy)]
enum JsonInspectionFailure {
    ForeignUpstream,
    ForeignAttempt,
    RejectedSecondObservation,
    RejectedLeaf,
    RejectedFinish,
    Capacity,
    Cancel,
}

fn json_inspection_failure_case(case: JsonInspectionFailure) -> TestResult {
    use super::{
        payload_budget::PayloadBudget,
        protocol_adapters::responses::{stage_json_response_bound, ResponsesJsonInspection},
        restoration_contract::{
            ClientProtocol, OriginalResponseObservation, ProtocolPair, UpstreamProtocol,
        },
        restoration_core::AttemptProvenance,
    };
    use crate::proxy::protocol_bridge::Protocol;
    let fixture = ResponsesFixture::new();
    let mut body = json!({"input":CONTACT});
    let attempt = fixture.prepare(&mut body, false, Some(SESSION))?;
    let text = body["input"]
        .as_str()
        .ok_or("c3_inspection_input_missing")?;
    let native = native_case(text, true)?;
    let budget = PayloadBudget::with_limit(4 * 1024 * 1024);
    let pair = ProtocolPair::new(
        ClientProtocol::new(Protocol::Responses),
        UpstreamProtocol::new(Protocol::Responses),
    );
    let provenance = AttemptProvenance::new(&budget)?;
    let baseline = budget.used();
    let mut inspection = ResponsesJsonInspection::begin(
        &attempt.lease,
        &attempt.binding,
        &budget,
        pair,
        provenance.clone(),
    )
    .map_err(|_| "c3_inspection_begin_failed")?;
    let original = OriginalResponseObservation::json(
        &native.response,
        pair.upstream(),
        provenance.clone(),
        budget.clone(),
    )
    .map_err(|_| "c3_inspection_observation_failed")?;
    inspection
        .observe_original(&original)
        .map_err(|_| "c3_inspection_initial_observe_failed")?;
    match case {
        JsonInspectionFailure::ForeignUpstream | JsonInspectionFailure::ForeignAttempt => {
            let upstream = if matches!(case, JsonInspectionFailure::ForeignUpstream) {
                UpstreamProtocol::new(Protocol::Chat)
            } else {
                pair.upstream()
            };
            let other = if matches!(case, JsonInspectionFailure::ForeignAttempt) {
                AttemptProvenance::new(&budget)?
            } else {
                provenance.clone()
            };
            let foreign = OriginalResponseObservation::json(
                &native.response,
                upstream,
                other,
                budget.clone(),
            )
            .map_err(|_| "c3_inspection_foreign_observation_failed")?;
            assert!(
                inspection.observe_original(&foreign).is_err(),
                "c3_inspection_foreign_original_accepted"
            );
            assert!(
                inspection.observe_original(&original).is_err(),
                "c3_inspection_failure_not_sticky"
            );
        }
        JsonInspectionFailure::RejectedSecondObservation => {
            let guard =
                ResponseLifecycle::reject_operation_for_test(LifecycleOperation::ObserveOriginal);
            assert!(
                inspection.observe_original(&original).is_err(),
                "c3_inspection_repeated_rejection_ignored"
            );
            drop(guard);
            assert!(
                inspection.observe_original(&original).is_err(),
                "c3_inspection_repeated_failure_repaired"
            );
        }
        _ => {}
    }
    drop(original);
    if matches!(case, JsonInspectionFailure::Cancel) {
        drop(inspection);
    } else {
        let exhausted = if matches!(case, JsonInspectionFailure::Capacity) {
            Some(budget.reserve(budget.limit() - budget.used())?)
        } else {
            None
        };
        let guard = match case {
            JsonInspectionFailure::RejectedLeaf => Some(
                ResponseLifecycle::reject_operation_for_test(LifecycleOperation::Done),
            ),
            JsonInspectionFailure::RejectedFinish => Some(
                ResponseLifecycle::reject_operation_for_test(LifecycleOperation::Finish),
            ),
            _ => None,
        };
        let result = stage_json_response_bound(
            &native.response,
            false,
            &attempt.lease,
            &attempt.binding,
            &budget,
            pair,
            &provenance,
            inspection,
        );
        drop(guard);
        assert!(result.is_err(), "c3_inspection_failed_owner_must_not_stage");
        drop(result);
        drop(exhausted);
    }
    assert_eq!(
        budget.used(),
        baseline,
        "c3_inspection_failed_owner_retained_charge"
    );
    assert_continuity(
        &fixture.store,
        &attempt.binding,
        text,
        &native.expected_arguments,
        false,
        true,
    )?;
    drop(provenance);
    assert_eq!(
        budget.used(),
        0,
        "c3_inspection_failed_owner_budget_not_released"
    );
    Ok(())
}

macro_rules! json_inspection_failure_tests {
    ($( $name:ident => $case:ident ),+ $(,)?) => { $(
        #[test]
        #[serial_test::serial]
        fn $name() -> TestResult { json_inspection_failure_case(JsonInspectionFailure::$case) }
    )+ };
}
json_inspection_failure_tests!(
    c3_json_preconversion_original_wrong_upstream_sticky => ForeignUpstream,
    c3_json_preconversion_original_other_attempt_sticky => ForeignAttempt,
    c3_json_preconversion_original_second_rejection_sticky => RejectedSecondObservation,
    c3_json_preconversion_leaf_failure_cleanup => RejectedLeaf,
    c3_json_preconversion_finish_failure_cleanup => RejectedFinish,
    c3_json_preconversion_capacity_failure_cleanup => Capacity,
    c3_json_preconversion_cancel_cleanup => Cancel,
);

#[tokio::test(flavor = "current_thread")]
#[serial_test::serial]
async fn c3_json_preconversion_duplicate_verifier_cannot_publish() -> TestResult {
    use super::completion_json::JsonCompletionProtocol;
    let fixture = ResponsesFixture::new();
    let mut body = json!({"input":CONTACT});
    let attempt = fixture.prepare(&mut body, false, Some(SESSION))?;
    let text = body["input"].as_str().ok_or("c3_duplicate_input_missing")?;
    let native = native_case(text, true)?;
    let raw = serde_json::to_string(&native.response)?;
    let binding = attempt.binding.clone();
    attempt
        .verify_upstream_json_completion(JsonCompletionProtocol::NativeResponses, &raw)
        .map_err(|_| "c3_duplicate_first_observation_failed")?;
    assert!(
        attempt
            .verify_upstream_json_completion(JsonCompletionProtocol::NativeResponses, &raw)
            .is_err(),
        "c3_duplicate_observation_accepted"
    );
    let response = Response::builder()
        .header("content-type", "application/json")
        .body(Body::from(raw))?;
    let response = attempt
        .restore_client_http_response(response, ArgumentNormalizationMode::Exact, None)
        .await;
    assert_eq!(
        response.status(),
        StatusCode::INTERNAL_SERVER_ERROR,
        "c3_duplicate_observation_repaired_at_delivery"
    );
    let bytes = response.into_body().collect().await?.to_bytes();
    let error: Value = serde_json::from_slice(&bytes)?;
    assert!(
        error.get("output").is_none(),
        "c3_duplicate_observation_released_tool"
    );
    assert!(
        !std::str::from_utf8(&bytes)?.contains(CONTACT),
        "c3_duplicate_observation_released_private_text"
    );
    assert_continuity(
        &fixture.store,
        &binding,
        text,
        &native.expected_arguments,
        false,
        true,
    )?;
    Ok(())
}
