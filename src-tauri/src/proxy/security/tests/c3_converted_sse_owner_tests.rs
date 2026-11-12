//! Owner-level handoff controls, not completion or HTTP delivery acceptance.
use super::*;
use crate::proxy::{
    protocol_bridge::Protocol,
    security::{
        completion_json::JsonCompletionProtocol,
        restoration_contract::OriginalResponseObservation,
        restoration_core::{AttemptProvenance, LifecycleOperation, SseEvidenceRequirement},
        tokenization::publication_test_support,
        unified_test_support::{ResponsesFixture, TestResult, CONTACT, PRINCIPAL, SESSION},
    },
};
use serde_json::json;

#[derive(Clone, Copy)]
enum Case {
    Control,
    Unobserved,
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
    ForeignObservationAttempt,
    ForeignObservationUpstream,
    LaterRejection,
    Capacity,
    AlreadyFailed,
}

fn owner_case(case: Case, chat: bool, mapped: bool) -> TestResult {
    let fixture = ResponsesFixture::new();
    let mut body = json!({"input": if mapped { CONTACT } else { "Synthetic ordinary input" }});
    let request = fixture.admit(&mut body, false, Some(SESSION))?;
    let protocol_name = if chat { "openai_chat" } else { "anthropic" };
    let target = if chat {
        "https://synthetic.invalid/v1/chat/completions"
    } else {
        "https://synthetic.invalid/v1/messages"
    };
    let descriptor = crate::proxy::providers::CodexAdapter::resolve_dlp_operation_descriptor(
        "https://synthetic.invalid/v1",
        "/responses",
        protocol_name,
        target,
        false,
    )
    .ok_or("c3_sse_owner_descriptor_missing")?;
    let (prepared, input_owner) = request
        .prepare_owned_attempt_with_operation_for_proxy(
            Some(&descriptor),
            "synthetic-owner-provider",
            protocol_name,
            target,
            Some(PRINCIPAL),
            &mut body,
            Ok,
        )
        .map_err(|_| "c3_sse_owner_prepare_failed")?;
    drop(input_owner);
    assert!(
        prepared.binding.operation_lineage.is_some(),
        "c3_sse_owner_lineage_missing"
    );
    let expected_binding = prepared.binding.clone();
    let budget = PayloadBudget::with_limit(4 * 1024 * 1024);
    let provenance = AttemptProvenance::new(&budget)?;
    let pair = ProtocolPair::new(
        ClientProtocol::new(Protocol::Responses),
        UpstreamProtocol::new(if chat {
            Protocol::Chat
        } else {
            Protocol::Anthropic
        }),
    );
    let begin_before = ResponseLifecycle::begin_count_for_test();
    let mut state = SseRestorationState::new_bound(
        prepared.lease.fork_attempt_extracted()?,
        expected_binding.clone(),
        budget.clone(),
        ArgumentNormalizationMode::Exact,
        pair,
        provenance.clone(),
    )
    .map_err(|_| "c3_sse_owner_begin_failed")?;
    state.require_original_evidence(SseEvidenceRequirement::new(
        provenance.clone(),
        Some(if chat {
            JsonCompletionProtocol::ChatCompletions
        } else {
            JsonCompletionProtocol::AnthropicMessages
        }),
        ArgumentNormalizationMode::Exact,
    ));
    assert!(
        state.needs_converted_inspection(),
        "c3_sse_owner_wrong_path"
    );
    let publication_before = publication_test_support::counts(&fixture.store)?;
    let mut inspection = state
        .lend_original_inspection()
        .map_err(|_| "c3_sse_owner_lend_failed")?;
    // Frame contents are synthetic, borrowed and observed before any conversion.
    let raw = if chat {
        "data: {\"choices\":[]}"
    } else {
        "event: message_start\ndata: {\"type\":\"message_start\"}"
    };
    if !matches!(case, Case::Unobserved) {
        inspection
            .observe_frame(raw, &budget)
            .map_err(|_| "c3_sse_owner_control_observe_failed")?;
    }
    match case {
        Case::Control | Case::Unobserved => {}
        Case::Client => {
            state.original_pair =
                ProtocolPair::new(ClientProtocol::new(Protocol::Chat), pair.upstream())
        }
        Case::Upstream => {
            state.original_pair =
                ProtocolPair::new(pair.client(), UpstreamProtocol::new(Protocol::Gemini))
        }
        Case::Attempt => state.require_original_evidence(SseEvidenceRequirement::new(
            AttemptProvenance::new(&budget)?,
            Some(JsonCompletionProtocol::ChatCompletions),
            ArgumentNormalizationMode::Exact,
        )),
        Case::Store => state.binding.store_id ^= 1,
        Case::Namespace => state.binding.namespace ^= 1,
        Case::Generation => state.binding.generation[0] ^= 1,
        Case::Policy => state.binding.policy_revision ^= 1,
        Case::Provider => state.binding.provider_fingerprint[0] ^= 1,
        Case::Principal => state.binding.reusable_principal = !state.binding.reusable_principal,
        Case::Operation => state.binding.operation_lineage = None,
        Case::ForeignObservationAttempt | Case::ForeignObservationUpstream => {
            let foreign_attempt = if matches!(case, Case::ForeignObservationAttempt) {
                AttemptProvenance::new(&budget)?
            } else {
                provenance.clone()
            };
            let foreign_upstream = if matches!(case, Case::ForeignObservationUpstream) {
                UpstreamProtocol::new(Protocol::Gemini)
            } else {
                pair.upstream()
            };
            let observation = OriginalResponseObservation::frame(
                raw.as_bytes(),
                foreign_upstream,
                foreign_attempt,
                budget.clone(),
            )
            .map_err(|_| "c3_sse_owner_foreign_fixture_failed")?;
            assert!(
                inspection.observe(Ok(observation)).is_err(),
                "c3_sse_owner_foreign_observation_accepted"
            );
            assert!(
                inspection.observe_frame(raw, &budget).is_err(),
                "c3_sse_owner_observation_failure_not_sticky"
            );
        }
        Case::LaterRejection => {
            let reject =
                ResponseLifecycle::reject_operation_for_test(LifecycleOperation::ObserveOriginal);
            assert!(
                inspection.observe_frame(raw, &budget).is_err(),
                "c3_sse_owner_later_rejection_missing"
            );
            drop(reject);
            assert!(
                inspection.observe_frame(raw, &budget).is_err(),
                "c3_sse_owner_later_rejection_not_sticky"
            );
        }
        Case::Capacity => {
            let reservation = budget.reserve(budget.limit() - budget.used())?;
            assert!(
                inspection.observe_frame(raw, &budget).is_err(),
                "c3_sse_owner_capacity_not_rejected"
            );
            drop(reservation);
            assert!(
                inspection.observe_frame(raw, &budget).is_err(),
                "c3_sse_owner_capacity_not_sticky"
            );
        }
        Case::AlreadyFailed => {
            assert!(
                state
                    .process_sse_block("event: error\ndata: {\"type\":\"error\"}")
                    .is_err(),
                "c3_sse_owner_failed_state_fixture_invalid"
            );
        }
    }
    let result = state.reclaim_original_inspection(inspection);
    if matches!(case, Case::Control) {
        result.map_err(|_| "c3_sse_owner_control_reclaim_failed")?;
        for poll in 0..4 {
            let mut inspection = state
                .lend_original_inspection()
                .map_err(|_| "c3_sse_owner_repeat_lend_failed")?;
            // Converters may emit several client frames for one original frame.
            if poll % 2 == 0 {
                inspection
                    .observe_frame(raw, &budget)
                    .map_err(|_| "c3_sse_owner_repeat_observe_failed")?;
            }
            state
                .reclaim_original_inspection(inspection)
                .map_err(|_| "c3_sse_owner_repeat_reclaim_failed")?;
            assert!(
                state.process_sse_block(": synthetic keepalive").is_ok(),
                "c3_sse_owner_valid_poll_rejected"
            );
        }
    } else {
        assert!(result.is_err(), "c3_sse_owner_invalid_handoff_accepted");
        // Restore all expected identities, but a rejected handoff must stay rejected.
        state.binding = expected_binding;
        state.original_pair = pair;
        state.require_original_evidence(SseEvidenceRequirement::new(
            provenance.clone(),
            Some(if chat {
                JsonCompletionProtocol::ChatCompletions
            } else {
                JsonCompletionProtocol::AnthropicMessages
            }),
            ArgumentNormalizationMode::Exact,
        ));
        assert!(
            state.process_sse_block(": synthetic keepalive").is_err(),
            "c3_sse_owner_handoff_rejection_not_sticky"
        );
        assert!(
            state.lend_original_inspection().is_err(),
            "c3_sse_owner_rejected_state_lent_again"
        );
    }
    assert_eq!(
        ResponseLifecycle::begin_count_for_test() - begin_before,
        1,
        "c3_sse_owner_created_shadow"
    );
    drop(state);
    drop(provenance);
    assert_eq!(budget.used(), 0, "c3_sse_owner_budget_not_released");
    assert_eq!(
        publication_test_support::counts(&fixture.store)?,
        publication_before,
        "c3_sse_owner_observation_published_completion"
    );
    Ok(())
}

macro_rules! owner_tests {
    ($( $name:ident => ($case:ident, $chat:expr, $mapped:expr) ),+ $(,)?) => { $(
        #[test]
        #[serial_test::serial]
        fn $name() -> TestResult { owner_case(Case::$case, $chat, $mapped) }
    )+ };
}
owner_tests! {
    c3_sse_owner_chat_mapped_control => (Control, true, true),
    c3_sse_owner_chat_zero_control => (Control, true, false),
    c3_sse_owner_anthropic_mapped_control => (Control, false, true),
    c3_sse_owner_anthropic_zero_control => (Control, false, false),
    c3_sse_owner_unobserved_mapped => (Unobserved, true, true),
    c3_sse_owner_unobserved_zero => (Unobserved, false, false),
    c3_sse_owner_client_mapped => (Client, true, true),
    c3_sse_owner_client_zero => (Client, false, false),
    c3_sse_owner_upstream_mapped => (Upstream, true, true),
    c3_sse_owner_upstream_zero => (Upstream, false, false),
    c3_sse_owner_attempt_mapped => (Attempt, true, true),
    c3_sse_owner_attempt_zero => (Attempt, false, false),
    c3_sse_owner_store_mapped => (Store, true, true),
    c3_sse_owner_store_zero => (Store, false, false),
    c3_sse_owner_namespace_mapped => (Namespace, true, true),
    c3_sse_owner_namespace_zero => (Namespace, false, false),
    c3_sse_owner_generation_mapped => (Generation, true, true),
    c3_sse_owner_generation_zero => (Generation, false, false),
    c3_sse_owner_policy_mapped => (Policy, true, true),
    c3_sse_owner_policy_zero => (Policy, false, false),
    c3_sse_owner_provider_mapped => (Provider, true, true),
    c3_sse_owner_provider_zero => (Provider, false, false),
    c3_sse_owner_principal_mapped => (Principal, true, true),
    c3_sse_owner_principal_zero => (Principal, false, false),
    c3_sse_owner_operation_mapped => (Operation, true, true),
    c3_sse_owner_operation_zero => (Operation, false, false),
    c3_sse_owner_foreign_attempt_mapped => (ForeignObservationAttempt, true, true),
    c3_sse_owner_foreign_attempt_zero => (ForeignObservationAttempt, false, false),
    c3_sse_owner_foreign_upstream_mapped => (ForeignObservationUpstream, true, true),
    c3_sse_owner_foreign_upstream_zero => (ForeignObservationUpstream, false, false),
    c3_sse_owner_later_rejection_mapped => (LaterRejection, true, true),
    c3_sse_owner_later_rejection_zero => (LaterRejection, false, false),
    c3_sse_owner_capacity_mapped => (Capacity, true, true),
    c3_sse_owner_capacity_zero => (Capacity, false, false),
    c3_sse_owner_failed_state_mapped => (AlreadyFailed, true, true),
    c3_sse_owner_failed_state_zero => (AlreadyFailed, false, false),
}

// These controls call the production parser directly, not TextProjectionStream.
#[derive(Clone, Copy)]
enum KindFailure {
    None,
    Allocation,
    Copy,
}

fn kind_budget_case(mapped: bool, data_only: bool, failure: KindFailure) -> TestResult {
    use crate::proxy::security::{payload_json, payload_text};
    use std::{cell::Cell, rc::Rc};

    let fixture = ResponsesFixture::new();
    let mut body = json!({"input": if mapped { CONTACT } else { "Synthetic ordinary input" }});
    let prepared = fixture.prepare(&mut body, false, Some(SESSION))?;
    let budget = PayloadBudget::with_limit(4 * 1024 * 1024);
    let provenance = AttemptProvenance::new(&budget)?;
    let mut state = SseRestorationState::new_bound(
        prepared.lease.fork_attempt_extracted()?,
        prepared.binding.clone(),
        budget.clone(),
        ArgumentNormalizationMode::Exact,
        ProtocolPair::new(
            ClientProtocol::new(Protocol::Responses),
            UpstreamProtocol::new(Protocol::Responses),
        ),
        provenance.clone(),
    )
    .map_err(|_| "c3_kind_state_failed")?;
    let baseline = budget.used();
    let publication_before = publication_test_support::counts(&fixture.store)?;
    // Escaped input measures decoded UTF-8 bytes, not the wire representation.
    let kind = "synthetic.kind.\u{03bb}";
    let data = r#"{"type":"synthetic.kind.\u03bb","synthetic_extension":true}"#;
    let block = if data_only {
        format!("data: {data}\n\n")
    } else {
        format!("event: {kind}\ndata: {data}\n\n")
    };
    let json_charge = Rc::new(Cell::new(0));
    let parsed_charge = Rc::clone(&json_charge);
    let parse_hook = payload_json::test_support::install(move |event| {
        if event.phase == payload_json::test_support::Phase::AfterParse {
            parsed_charge.set(event.charged);
        }
        payload_json::test_support::Action::Continue
    });
    let allocations = Rc::new(Cell::new(0));
    let copies = Rc::new(Cell::new(0));
    let seen_allocations = Rc::clone(&allocations);
    let seen_copies = Rc::clone(&copies);
    let text_hook = payload_text::test_support::install(move |event| {
        use payload_text::test_support::{Action, Phase};
        assert_eq!(event.bytes, kind.len(), "c3_kind_unrelated_text_owner");
        assert!(json_charge.get() > 0, "c3_kind_not_after_production_parse");
        assert_eq!(
            event.used,
            baseline + data.len() + json_charge.get() + kind.len(),
            "c3_kind_not_independently_precharged"
        );
        match event.phase {
            Phase::BeforeAllocation => {
                seen_allocations.set(seen_allocations.get() + 1);
                if matches!(failure, KindFailure::Allocation) {
                    return Action::Fail;
                }
            }
            Phase::BeforeCopy => {
                seen_copies.set(seen_copies.get() + 1);
                if matches!(failure, KindFailure::Copy) {
                    return Action::Fail;
                }
            }
            Phase::AfterAllocation => {}
        }
        Action::Continue
    });
    let result = state.process_sse_block(&block);
    drop(text_hook);
    drop(parse_hook);
    if matches!(failure, KindFailure::None) {
        let frames = result.map_err(|_| "c3_kind_control_rejected")?;
        assert_eq!(frames.len(), 1, "c3_kind_control_frame_count");
        let frame = std::str::from_utf8(&frames[0])?;
        let (header, encoded) = frame.split_once("\ndata: ").ok_or("c3_kind_bad_framing")?;
        assert_eq!(header, format!("event: {kind}"), "c3_kind_header_changed");
        assert!(
            serde_json::from_str::<Value>(encoded)? == serde_json::from_str::<Value>(data)?,
            "c3_kind_native_payload_changed"
        );
        drop(frames);
        assert_eq!(budget.used(), baseline, "c3_kind_transient_charge_leaked");
    } else {
        assert!(result.is_err(), "c3_kind_unaccounted_copy_accepted");
        assert!(
            state.process_sse_block(": synthetic keepalive").is_err(),
            "c3_kind_failure_not_sticky"
        );
        assert_eq!(budget.used(), baseline, "c3_kind_failure_charge_leaked");
    }
    assert_eq!(allocations.get(), 1, "c3_kind_missing_allocation_control");
    assert_eq!(
        copies.get(),
        usize::from(!matches!(failure, KindFailure::Allocation)),
        "c3_kind_missing_copy_control"
    );
    drop(state);
    drop(provenance);
    assert_eq!(budget.used(), 0, "c3_kind_state_charge_leaked");
    assert_eq!(
        publication_test_support::counts(&fixture.store)?,
        publication_before,
        "c3_kind_parser_published"
    );
    Ok(())
}

macro_rules! kind_budget_tests {
    ($( $name:ident => ($mapped:expr, $data_only:expr, $failure:ident) ),+ $(,)?) => { $(
        #[test]
        #[serial_test::serial]
        fn $name() -> TestResult { kind_budget_case($mapped, $data_only, KindFailure::$failure) }
    )+ };
}
kind_budget_tests! {
    c3_sse_kind_mapped_header_control => (true, false, None),
    c3_sse_kind_zero_header_control => (false, false, None),
    c3_sse_kind_mapped_data_control => (true, true, None),
    c3_sse_kind_zero_data_control => (false, true, None),
    c3_sse_kind_mapped_header_allocation_refused => (true, false, Allocation),
    c3_sse_kind_zero_header_allocation_refused => (false, false, Allocation),
    c3_sse_kind_mapped_data_allocation_refused => (true, true, Allocation),
    c3_sse_kind_zero_data_allocation_refused => (false, true, Allocation),
    c3_sse_kind_mapped_header_copy_refused => (true, false, Copy),
    c3_sse_kind_zero_header_copy_refused => (false, false, Copy),
    c3_sse_kind_mapped_data_copy_refused => (true, true, Copy),
    c3_sse_kind_zero_data_copy_refused => (false, true, Copy),
}
