//! Non-Responses execution controls using observed native requests and sealed delivery.
use bytes::Bytes;
use futures::StreamExt;
use serde_json::Value;

use super::native_test_support::{record_delivery, NativeFixture, ScriptedSource, LIMIT, TEXT};

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

#[test]
fn c5_native_execution_observed_request_json_split_pending_and_delivery() -> TestResult {
    for mapped in [true, false] {
        for width in [1, 7, 4096] {
            let fixture = NativeFixture::new(mapped)?;
            let budget = fixture.budget.clone();
            let source = fixture.json_source();
            let wire = Bytes::from(serde_json::to_vec_pretty(&source)?);
            let offsets: Vec<_> = (width..wire.len()).step_by(width).collect();
            let (transport, stats) = ScriptedSource::split(wire.clone(), &offsets)?;
            let proof = futures::executor::block_on(fixture.json_proof(&source, transport));
            assert!(
                proof.is_ok(),
                "native_execution_configurable_json_transport_missing"
            );
            let proof = proof.map_err(|_| "native_execution_proof_rejected")?;
            assert!(
                stats.eof.get() == 1 && stats.pending.get() > 0 && stats.dropped.get() == 1,
                "native_execution_transport_not_consumed"
            );
            let finalized = fixture.finalize_json(&source, wire.clone(), proof)?;
            assert!(
                fixture.counts()? == (1, 0, 0),
                "native_execution_json_not_activated_before_poll"
            );
            let delivery =
                futures::executor::block_on(record_delivery(finalized.into_delivery_stream()))?;
            assert!(delivery.len() == 1, "native_execution_extra_delivery");
            let decoded: Value = serde_json::from_slice(&delivery[0])?;
            assert!(
                decoded["choices"][0]["message"]["content"].as_str()
                    == Some(if mapped { TEXT } else { &fixture.reference }),
                "native_execution_restoration_missing"
            );
            assert!(
                decoded["extension"] == source["extension"],
                "native_execution_extension_changed"
            );
            if !mapped {
                assert!(
                    delivery[0] == wire && delivery[0].as_ptr() == wire.as_ptr(),
                    "native_execution_original_owner_changed"
                );
            }
            drop(delivery);
            drop(fixture);
            assert!(budget.used() == 0, "native_execution_final_charge_retained");
        }
    }
    Ok(())
}

#[test]
fn c5_native_execution_sse_independent_splits_and_last_yield_continuation() -> TestResult {
    use super::{http_restoration::test_transport, native_test_support::NativeSseEntity};
    for mapped in [true, false] {
        for width in [1, 11, 4096] {
            let fixture = NativeFixture::new(mapped)?;
            let budget = fixture.budget.clone();
            let entity = NativeSseEntity::new(&fixture);
            let wire = Bytes::copy_from_slice(entity.wire.as_bytes());
            let offsets: Vec<_> = (width..wire.len()).step_by(width).collect();
            let (original, original_stats) = ScriptedSource::split(wire.clone(), &offsets)?;
            let (client, client_stats) = ScriptedSource::split(wire.clone(), &[0, wire.len() / 2])?;
            let proof = futures::executor::block_on(test_transport::native_sse_from_streams(
                &fixture.attempt,
                &fixture.binding,
                &budget,
                original,
                client,
            ));
            assert!(
                proof.is_ok(),
                "native_execution_configurable_sse_transport_missing"
            );
            let proof = proof.map_err(|_| "native_execution_sse_proof_rejected")?;
            assert!(
                original_stats.eof.get() == 1 && client_stats.eof.get() == 1,
                "native_execution_sse_eof_missing"
            );
            let finalized = fixture.finalize_sse(&entity, proof)?;
            assert!(
                fixture.counts()? == (0, 1, 1),
                "native_execution_sse_published_before_delivery"
            );
            let mut delivery = finalized.into_delivery_stream();
            let mut recorded = Vec::new();
            for _ in 0..3 {
                let chunk = futures::executor::block_on(delivery.next())
                    .ok_or("native_execution_sse_delivery_missing")??;
                recorded.push(chunk);
                assert!(
                    fixture.counts()? == (0, 1, 1),
                    "native_execution_sse_published_before_resume"
                );
            }
            assert!(
                futures::executor::block_on(delivery.next()).is_none(),
                "native_execution_sse_extra_delivery"
            );
            assert!(
                fixture.counts()? == (1, 0, 0),
                "native_execution_sse_resume_not_published"
            );
            let joined: Vec<u8> = recorded.iter().flat_map(|b| b.iter().copied()).collect();
            if !mapped {
                assert!(
                    joined == wire,
                    "native_execution_sse_unchanged_wire_changed"
                );
            }
            let text = std::str::from_utf8(&joined)?;
            let values: Vec<Value> = text
                .lines()
                .filter_map(|line| line.strip_prefix("data:"))
                .filter(|data| data.trim() != "[DONE]")
                .map(|data| serde_json::from_str(data.trim()))
                .collect::<Result<_, _>>()?;
            assert!(
                values[0]["choices"][0]["delta"]["content"].as_str()
                    == Some(if mapped { TEXT } else { &fixture.reference }),
                "native_execution_sse_restoration_missing"
            );
            assert!(
                values[0]["extension"] == entity.sources[0]["extension"],
                "native_execution_sse_extension_changed"
            );
            drop(recorded);
            drop(delivery);
            drop(fixture);
            assert!(budget.used() == 0, "native_execution_sse_charge_retained");
        }
    }
    Ok(())
}

#[test]
fn c5_native_execution_transport_error_eof_and_cancellation_never_publish() -> TestResult {
    use super::{
        http_restoration::test_transport,
        native_test_support::{NativeSseEntity, Step},
    };
    use std::{
        future::Future,
        io,
        task::{Context, Poll},
    };
    for mapped in [true, false] {
        for mode in 0..4 {
            let fixture = NativeFixture::new(mapped)?;
            let budget = fixture.budget.clone();
            let baseline = budget.used();
            let source = fixture.json_source();
            let wire = Bytes::from(serde_json::to_vec(&source)?);
            let (transport, stats) = ScriptedSource::new([
                Step::Bytes(wire.clone()),
                if mode == 0 {
                    Step::Error(io::ErrorKind::Interrupted)
                } else {
                    Step::Park
                },
                Step::Bytes(Bytes::from_static(b"must not poll")),
            ]);
            if mode == 0 {
                let result = futures::executor::block_on(fixture.json_proof(&source, transport));
                assert!(
                    result.is_err() && stats.polls.get() == 2 && stats.eof.get() == 0,
                    "native_execution_error_accepted_or_retried"
                );
            } else if mode == 1 {
                let mut future = Box::pin(fixture.json_proof(&source, transport));
                let mut cx = Context::from_waker(futures::task::noop_waker_ref());
                assert!(
                    matches!(future.as_mut().poll(&mut cx), Poll::Pending),
                    "native_execution_pending_treated_as_eof"
                );
                assert!(
                    budget.used() > baseline,
                    "native_execution_cancel_fixture_not_charged"
                );
                drop(future);
            } else {
                drop(transport);
                let entity = NativeSseEntity::new(&fixture);
                let wire = Bytes::copy_from_slice(entity.wire.as_bytes());
                let (original, original_stats) = ScriptedSource::new(if mode == 2 {
                    vec![
                        Step::Bytes(wire.clone()),
                        Step::Error(io::ErrorKind::ConnectionReset),
                    ]
                } else {
                    vec![Step::Bytes(wire.clone())]
                });
                let (client, client_stats) = ScriptedSource::new([Step::Bytes(wire), Step::Park]);
                let mut future = Box::pin(test_transport::native_sse_from_streams(
                    &fixture.attempt,
                    &fixture.binding,
                    &budget,
                    original,
                    client,
                ));
                let mut cx = Context::from_waker(futures::task::noop_waker_ref());
                if mode == 2 {
                    assert!(
                        matches!(future.as_mut().poll(&mut cx), Poll::Ready(Err(_))),
                        "native_execution_sse_error_accepted"
                    );
                    assert!(
                        original_stats.eof.get() == 0 && client_stats.polls.get() == 0,
                        "native_execution_client_polled_after_original_failure"
                    );
                } else {
                    assert!(
                        matches!(future.as_mut().poll(&mut cx), Poll::Pending),
                        "native_execution_sse_pending_treated_as_eof"
                    );
                    assert!(
                        original_stats.eof.get() == 1 && client_stats.eof.get() == 0,
                        "native_execution_client_pending_fixture_invalid"
                    );
                }
                drop(future);
                assert!(
                    original_stats.dropped.get() == 1 && client_stats.dropped.get() == 1,
                    "native_execution_sse_cancel_source_retained"
                );
            }
            assert!(
                stats.dropped.get() == 1,
                "native_execution_cancel_source_retained"
            );
            assert!(
                fixture.counts()? == (0, 0, 0),
                "native_execution_failure_published"
            );
            assert!(
                budget.used() == baseline,
                "native_execution_failure_charge_retained"
            );
            drop(fixture);
            assert!(
                budget.used() == 0,
                "native_execution_failure_owner_retained"
            );
        }
    }
    Ok(())
}

#[test]
fn c5_native_execution_sse_delivery_cancel_at_every_yield_rolls_back() -> TestResult {
    use super::{http_restoration::test_transport, native_test_support::NativeSseEntity};
    for mapped in [true, false] {
        for yields in 0..=3 {
            let fixture = NativeFixture::new(mapped)?;
            let budget = fixture.budget.clone();
            let baseline = budget.used();
            let entity = NativeSseEntity::new(&fixture);
            let wire = Bytes::copy_from_slice(entity.wire.as_bytes());
            let (original, _) = ScriptedSource::split(wire.clone(), &[1])?;
            let (client, _) = ScriptedSource::split(wire, &[2])?;
            let proof = futures::executor::block_on(test_transport::native_sse_from_streams(
                &fixture.attempt,
                &fixture.binding,
                &budget,
                original,
                client,
            ))
            .map_err(|_| "native_execution_sse_cancel_proof_rejected")?;
            let mut delivery = fixture.finalize_sse(&entity, proof)?.into_delivery_stream();
            let mut held = Vec::new();
            for _ in 0..yields {
                held.push(
                    futures::executor::block_on(delivery.next())
                        .ok_or("native_execution_cancel_yield_missing")??,
                );
                assert!(
                    fixture.counts()? == (0, 1, 1),
                    "native_execution_cancel_published_early"
                );
            }
            drop(delivery);
            assert!(
                fixture.counts()? == (0, 0, 0),
                "native_execution_cancel_pending_or_pin_retained"
            );
            drop(held);
            assert!(
                budget.used() == baseline,
                "native_execution_cancel_delivery_charge_retained"
            );
            drop(fixture);
            assert!(
                budget.used() == 0,
                "native_execution_cancel_final_charge_retained"
            );
        }
    }
    Ok(())
}

#[test]
fn c5_native_execution_real_budget_and_parser_faults_fail_closed() -> TestResult {
    use super::payload_json::test_support as json_events;
    for mapped in [true, false] {
        for mode in 0..3 {
            let fixture = NativeFixture::new(mapped)?;
            let budget = fixture.budget.clone();
            let baseline = budget.used();
            let source = fixture.json_source();
            let wire = Bytes::from(serde_json::to_vec(&source)?);
            let (transport, stats) = ScriptedSource::split(wire, &[])?;
            if mode == 0 {
                let occupied = budget.reserve(LIMIT - budget.used())?;
                let result = futures::executor::block_on(fixture.json_proof(&source, transport));
                assert!(
                    result.is_err() && stats.eof.get() == 0,
                    "native_execution_budget_failure_accepted"
                );
                drop(occupied);
            } else {
                let reached = std::rc::Rc::new(std::cell::Cell::new(false));
                let captured = reached.clone();
                let phase = if mode == 1 {
                    json_events::Phase::BeforeParse
                } else {
                    json_events::Phase::AfterParse
                };
                let hook = json_events::install(move |event| {
                    if event.phase == phase {
                        captured.set(true);
                        assert!(
                            event.charged > 0 && event.used >= event.charged,
                            "native_execution_parser_not_precharged"
                        );
                        return json_events::Action::Fail;
                    }
                    json_events::Action::Continue
                });
                let result = futures::executor::block_on(fixture.json_proof(&source, transport));
                drop(hook);
                assert!(
                    result.is_err() && reached.get(),
                    "native_execution_parser_fault_not_reached"
                );
            }
            assert!(
                fixture.counts()? == (0, 0, 0),
                "native_execution_fault_published"
            );
            assert!(
                budget.used() == baseline,
                "native_execution_fault_charge_retained"
            );
            drop(fixture);
            assert!(budget.used() == 0, "native_execution_fault_owner_retained");
        }
    }
    Ok(())
}

#[test]
fn c5_native_execution_shared_lifecycle_finish_abort_and_sticky_fault() -> TestResult {
    use super::{
        payload_buffer::PayloadBuffer,
        restoration_contract::{ProtocolPair, RestoreDecision, TextPolicy, UpstreamProtocol},
        restoration_core::{LifecycleLimits, LifecycleOperation, ResponseLifecycle},
    };
    use crate::proxy::protocol_bridge::Protocol;
    for mapped in [true, false] {
        for mode in 0..3 {
            let fixture = NativeFixture::new(mapped)?;
            let budget = fixture.budget.clone();
            let source = fixture.json_source();
            let wire = serde_json::to_vec(&source)?;
            let mut owner = ResponseLifecycle::begin(
                fixture.lease,
                fixture.binding,
                budget.clone(),
                ProtocolPair::new(
                    super::native_test_support::chat(),
                    UpstreamProtocol::new(Protocol::Chat),
                ),
                fixture.attempt.clone(),
                LifecycleLimits {
                    max_text_bytes: 65536,
                    max_text_streams: 16,
                    max_held_bytes: 65536,
                    max_held_units: 16,
                },
            )
            .map_err(|_| "native_execution_lifecycle_begin_rejected")?;
            let stream = owner
                .open_text(TextPolicy::Output)
                .map_err(|_| "native_execution_lifecycle_open_rejected")?;
            let boundary = fixture
                .reference
                .char_indices()
                .nth(2)
                .map(|(i, _)| i)
                .unwrap_or(0);
            let decision = owner
                .delta(&stream, &fixture.reference[..boundary])
                .map_err(|_| "native_execution_lifecycle_delta_rejected")?;
            if mapped {
                assert!(
                    matches!(decision, RestoreDecision::Hold),
                    "native_execution_partial_reference_released"
                );
            }
            drop(decision);
            owner
                .hold(PayloadBuffer::try_from_slice(&wire, budget.clone(), 65536)?)
                .map_err(|_| "native_execution_lifecycle_hold_rejected")?;
            let (published, pending, pins, ..) =
                super::tokenization::publication_test_support::counts(&fixture.store)?;
            assert!(
                (published, pending, pins) == (0, 0, 0),
                "native_execution_lifecycle_hold_published"
            );
            if mode == 1 {
                owner
                    .abort()
                    .map_err(|_| "native_execution_lifecycle_abort_rejected")?;
            } else if mode == 2 {
                let hook = ResponseLifecycle::reject_operation_for_test(LifecycleOperation::Delta);
                assert!(
                    owner
                        .delta(&stream, &fixture.reference[boundary..])
                        .is_err(),
                    "native_execution_lifecycle_fault_not_injected"
                );
                drop(hook);
                assert!(
                    owner.done(&stream, &fixture.reference).is_err(),
                    "native_execution_lifecycle_fault_not_sticky"
                );
                owner
                    .abort()
                    .map_err(|_| "native_execution_lifecycle_failed_abort_rejected")?;
            } else {
                drop(
                    owner
                        .delta(&stream, &fixture.reference[boundary..])
                        .map_err(|_| "native_execution_lifecycle_second_delta_rejected")?,
                );
                let done = owner
                    .done(&stream, &fixture.reference)
                    .map_err(|_| "native_execution_lifecycle_done_rejected")?;
                match &done {
                    RestoreDecision::Patch(text) => assert!(
                        mapped && text.as_str() == TEXT,
                        "native_execution_lifecycle_wrong_patch"
                    ),
                    RestoreDecision::Unchanged => {
                        assert!(!mapped, "native_execution_lifecycle_missing_patch")
                    }
                    _ => return Err("native_execution_lifecycle_unexpected_hold".into()),
                }
                drop(done);
                let inspection = owner
                    .finish()
                    .map_err(|_| "native_execution_lifecycle_finish_rejected")?;
                assert!(
                    inspection.dependency_tokens().len() == usize::from(mapped)
                        && inspection.restored_occurrences() == usize::from(mapped),
                    "native_execution_lifecycle_ledger_changed"
                );
                assert!(
                    inspection.held_units().len() == 1
                        && inspection.held_units()[0].as_slice() == wire,
                    "native_execution_lifecycle_held_owner_changed"
                );
                drop(inspection);
            }
            let (published, pending, pins, ..) =
                super::tokenization::publication_test_support::counts(&fixture.store)?;
            assert!(
                (published, pending, pins) == (0, 0, 0),
                "native_execution_inspection_minted_completion"
            );
            drop(stream);
            drop(fixture.attempt);
            assert!(
                budget.used() == 0,
                "native_execution_lifecycle_charge_retained"
            );
        }
    }
    Ok(())
}

#[test]
fn c5_native_execution_json_lifecycle_gates_cannot_be_bypassed() -> TestResult {
    use super::restoration_core::{LifecycleOperation, ResponseLifecycle};

    for mapped in [true, false] {
        for operation in [
            LifecycleOperation::Begin,
            LifecycleOperation::OpenText,
            LifecycleOperation::Done,
            LifecycleOperation::Hold,
            LifecycleOperation::Finish,
        ] {
            if !mapped && operation == LifecycleOperation::Hold {
                continue;
            }
            let fixture = NativeFixture::new(mapped)?;
            let budget = fixture.budget.clone();
            let source = fixture.json_source();
            let wire = Bytes::from(serde_json::to_vec(&source)?);
            let (transport, _) = ScriptedSource::split(wire.clone(), &[])?;
            let proof = futures::executor::block_on(fixture.json_proof(&source, transport))
                .map_err(|_| "native_json_lifecycle_proof_rejected")?;
            let baseline = budget.used();
            let rejection = ResponseLifecycle::reject_operation_for_test(operation);
            let result = fixture.finalize_json(&source, wire, proof);
            assert!(result.is_err(), "native_json_lifecycle_gate_bypassed");
            drop(result);
            drop(rejection);
            assert!(
                fixture.counts()? == (0, 0, 0),
                "native_json_lifecycle_failure_published"
            );
            assert!(
                budget.used() == baseline,
                "native_json_lifecycle_failure_retained_owner"
            );
            drop(fixture);
            assert!(
                budget.used() == 0,
                "native_json_lifecycle_failure_retained_budget"
            );
        }
    }
    Ok(())
}

#[test]
fn c5_native_execution_json_lifecycle_attempt_handoff_is_sealed() -> TestResult {
    use super::{
        response_restoration::finalize_native_json,
        restoration_contract::{
            ClientProtocol, EditConstraint, ProtocolPair, SourceClass, TextSites, UpstreamProtocol,
        },
        restoration_core::{
            stage_native_json_with_lifecycle, AttemptProvenance, NativeJsonObservation,
        },
    };
    use crate::proxy::protocol_bridge::Protocol;
    for mapped in [true, false] {
        for mode in 0..3 {
            let fixture = NativeFixture::new(mapped)?;
            let budget = fixture.budget.clone();
            let source = fixture.json_source();
            let mut sites = TextSites::new(budget.clone())?;
            sites.observe(
                &source["choices"][0]["message"]["content"],
                SourceClass::CurrentInput,
                EditConstraint::Editable,
            )?;
            let observed = NativeJsonObservation::new(
                &source,
                ClientProtocol::new(Protocol::Chat),
                sites,
                &[&source["choices"][0]["message"]],
                budget.clone(),
            )
            .map_err(|_| "native_json_handoff_observation_rejected")?;
            let foreign = AttemptProvenance::new(&budget)?;
            let pair = ProtocolPair::new(
                ClientProtocol::new(if mode == 1 {
                    Protocol::Anthropic
                } else {
                    Protocol::Chat
                }),
                UpstreamProtocol::new(if mode == 2 {
                    Protocol::Responses
                } else {
                    Protocol::Chat
                }),
            );
            let staged = stage_native_json_with_lifecycle(
                &observed,
                &fixture.lease,
                &fixture.binding,
                pair,
                if mode == 0 {
                    &foreign
                } else {
                    &fixture.attempt
                },
            );
            let wire = Bytes::from(serde_json::to_vec(&source)?);
            let (transport, _) = ScriptedSource::split(wire.clone(), &[])?;
            let proof = futures::executor::block_on(fixture.json_proof(&source, transport))
                .map_err(|_| "native_json_handoff_proof_rejected")?;
            let result = staged.and_then(|stage| {
                finalize_native_json(
                    stage,
                    &fixture.lease,
                    proof,
                    &fixture.attempt,
                    wire,
                    super::native_test_support::MAX_ENTITY,
                )
            });
            assert!(
                result.is_err(),
                "native_json_lifecycle_foreign_attempt_or_pair_released"
            );
            drop(result);
            assert!(
                fixture.counts()? == (0, 0, 0),
                "native_json_handoff_foreign_published"
            );
            drop(observed);
            drop(foreign);
            drop(fixture);
            assert!(
                budget.used() == 0,
                "native_json_handoff_foreign_retained_budget"
            );
        }
    }
    Ok(())
}

#[test]
fn c5_native_execution_json_lifecycle_edits_dependencies_and_failure_atomicity() -> TestResult {
    use super::{
        payload_json::test_support as json_events,
        response_restoration::finalize_native_json,
        restoration_contract::{
            ClientProtocol, EditConstraint, ProtocolPair, SourceClass, TextSites, UpstreamProtocol,
        },
        restoration_core::{stage_native_json_with_lifecycle, NativeJsonObservation},
    };
    use crate::proxy::protocol_bridge::Protocol;

    // Characterize existing policies through the lifecycle composition, not a new RED.
    for mapped in [true, false] {
        for mode in 0..6 {
            let fixture = NativeFixture::new(mapped)?;
            let budget = fixture.budget.clone();
            let baseline = budget.used();
            let arguments = serde_json::to_string(
                &serde_json::json!({"value":fixture.reference,"types":[true,null,7]}),
            )?;
            let mut source = fixture.json_source();
            source["choices"][0]["finish_reason"] = "tool_calls".into();
            source["choices"][0]["message"]["tool_calls"] = serde_json::json!([
                {"id":"synthetic-call","type":"function","function":{"name":"synthetic_nonexecuting","arguments":arguments}}
            ]);
            source["extension"]["same_value"] = fixture.reference.clone().into();
            let wire = Bytes::from(serde_json::to_vec_pretty(&source)?);
            let original_ptr = wire.as_ptr();
            let (transport, _) = ScriptedSource::split(wire.clone(), &[1, 3, 7])?;
            let proof = futures::executor::block_on(fixture.json_proof(&source, transport))
                .map_err(|_| "native_json_lifecycle_tool_proof_rejected")?;
            let mut sites = TextSites::new(budget.clone())?;
            sites.observe(
                &source["choices"][0]["message"]["content"],
                SourceClass::CurrentInput,
                if mode == 1 {
                    EditConstraint::Immutable
                } else {
                    EditConstraint::Editable
                },
            )?;
            sites.observe(
                &source["choices"][0]["message"]["tool_calls"][0]["function"]["arguments"],
                SourceClass::ToolArguments,
                EditConstraint::Editable,
            )?;
            let observed = NativeJsonObservation::new(
                &source,
                ClientProtocol::new(Protocol::Chat),
                sites,
                &[&source["choices"][0]["message"]],
                budget.clone(),
            )
            .map_err(|_| "native_json_lifecycle_tool_observation_rejected")?;
            let occupied = if mode == 2 {
                Some(budget.reserve(LIMIT - budget.used())?)
            } else {
                None
            };
            let staged = stage_native_json_with_lifecycle(
                &observed,
                &fixture.lease,
                &fixture.binding,
                ProtocolPair::new(
                    ClientProtocol::new(Protocol::Chat),
                    UpstreamProtocol::new(Protocol::Chat),
                ),
                &fixture.attempt,
            );
            if mode == 2 || (mode == 1 && mapped) {
                assert!(
                    staged.is_err(),
                    "native_json_lifecycle_budget_or_immutable_escaped"
                );
                drop(staged);
                drop(proof);
            } else {
                let staged = staged.map_err(|_| "native_json_lifecycle_tool_stage_rejected")?;
                assert!(
                    staged.output_tokens().len() == usize::from(mapped)
                        && staged.restored_occurrences() == 2 * usize::from(mapped),
                    "native_json_lifecycle_ledger_duplicated_or_lost"
                );
                assert!(
                    staged.as_value()["extension"] == source["extension"],
                    "native_json_lifecycle_unselected_sibling_changed"
                );
                if mode == 3 {
                    drop(staged);
                    drop(proof);
                } else {
                    let reached = std::rc::Rc::new(std::cell::Cell::new(false));
                    let seen = reached.clone();
                    let hook = if mode >= 4 {
                        Some(json_events::install(move |event| {
                            let phase = if mode == 4 {
                                json_events::Phase::BeforeParse
                            } else {
                                json_events::Phase::AfterParse
                            };
                            if event.phase == phase {
                                seen.set(true);
                                json_events::Action::Fail
                            } else {
                                json_events::Action::Continue
                            }
                        }))
                    } else {
                        None
                    };
                    let finalized = finalize_native_json(
                        staged,
                        &fixture.lease,
                        proof,
                        &fixture.attempt,
                        wire.clone(),
                        super::native_test_support::MAX_ENTITY,
                    );
                    drop(hook);
                    if mode >= 4 {
                        assert!(
                            reached.get() && finalized.is_err(),
                            "native_json_lifecycle_finalizer_fault_escaped"
                        );
                        drop(finalized);
                    } else {
                        let finalized = finalized
                            .map_err(|_| "native_json_lifecycle_tool_finalize_rejected")?;
                        assert!(
                            fixture.counts()? == (1, 0, 0),
                            "native_json_lifecycle_json_not_activated_before_poll"
                        );
                        let chunks = futures::executor::block_on(record_delivery(
                            finalized.into_delivery_stream(),
                        ))?;
                        assert!(
                            chunks.len() == 1,
                            "native_json_lifecycle_partial_tool_release"
                        );
                        let delivered: Value = serde_json::from_slice(&chunks[0])?;
                        let text = if mapped {
                            TEXT
                        } else {
                            fixture.reference.as_str()
                        };
                        let inner: Value = serde_json::from_str(
                            delivered["choices"][0]["message"]["tool_calls"][0]["function"]
                                ["arguments"]
                                .as_str()
                                .ok_or("native_json_lifecycle_arguments_missing")?,
                        )?;
                        assert!(
                            delivered["choices"][0]["message"]["content"] == text
                                && inner["value"] == text
                                && inner["types"] == serde_json::json!([true, null, 7])
                                && delivered["extension"] == source["extension"],
                            "native_json_lifecycle_actual_wire_or_types_changed"
                        );
                        if !mapped {
                            assert!(
                                chunks[0].as_ptr() == original_ptr && chunks[0] == wire,
                                "native_json_lifecycle_original_owner_lost"
                            );
                        }
                        drop(chunks);
                    }
                }
            }
            drop(occupied);
            drop(observed);
            if mode >= 2 || (mode == 1 && mapped) {
                assert!(
                    fixture.counts()? == (0, 0, 0),
                    "native_json_lifecycle_failed_or_dropped_published"
                );
            }
            assert!(
                budget.used() == baseline,
                "native_json_lifecycle_stage_or_finalizer_charge_retained"
            );
            drop(fixture);
            assert!(
                budget.used() == 0,
                "native_json_lifecycle_final_charge_retained"
            );
        }
    }
    Ok(())
}

// Adapter fixture declares native stream grouping; the core never infers Chat indices.
fn execute_sse_handoff(
    fixture: &NativeFixture,
    fragments: &[&str],
    pair: super::restoration_contract::ProtocolPair,
    lifecycle_attempt: &super::restoration_core::AttemptProvenance,
) -> TestResult<super::response_restoration::FinalizedNativeSse> {
    execute_sse_handoff_selected(fixture, fragments, pair, lifecycle_attempt, 0, false)
}

fn execute_sse_handoff_selected(
    fixture: &NativeFixture,
    fragments: &[&str],
    pair: super::restoration_contract::ProtocolPair,
    lifecycle_attempt: &super::restoration_core::AttemptProvenance,
    coverage: usize,
    immutable: bool,
) -> TestResult<super::response_restoration::FinalizedNativeSse> {
    use super::{
        http_restoration::test_transport,
        native_test_support::chat,
        response_restoration::finalize_native_sse,
        restoration_contract::{EditConstraint, SourceClass, TextSites},
        restoration_core::{
            stage_native_sse_with_lifecycle, NativeSseFrameObservation, NativeSseObservation,
            NativeSseSnapshotSelection, NativeSseTextStream,
        },
    };
    let mut sources: Vec<Value> = fragments
        .iter()
        .map(|text| {
            serde_json::json!({
        "id":"synthetic-response","object":"chat.completion.chunk","choices":[{"index":0,
        "delta":{"role":"assistant","content":text},"finish_reason":null}],
        "extension":{"same_value":text,"types":[true,null,7]}})
        })
        .collect();
    sources.push(
        serde_json::json!({"id":"synthetic-response","object":"chat.completion.chunk",
        "choices":[{"index":0,"delta":{},"finish_reason":"stop"}]}),
    );
    let mut wire = String::new();
    let mut ranges = Vec::new();
    for source in &sources {
        let start = wire.len();
        wire.push_str(&format!(": synthetic\r\ndata: {source}\r\n\r\n"));
        ranges.push(start..wire.len());
    }
    let done_start = wire.len();
    wire.push_str("data: [DONE]\n\n");
    let bytes = Bytes::copy_from_slice(wire.as_bytes());
    let (original, _) = ScriptedSource::split(bytes.clone(), &[0, 1, bytes.len() / 2])?;
    let (client, _) = ScriptedSource::split(bytes, &[2, 3])?;
    let proof = futures::executor::block_on(test_transport::native_sse_from_streams(
        &fixture.attempt,
        &fixture.binding,
        &fixture.budget,
        original,
        client,
    ))
    .map_err(|_| "native_sse_handoff_http_proof_rejected")?;
    let mut frames = Vec::new();
    for (index, (source, range)) in sources.iter().zip(ranges).enumerate() {
        let mut sites = TextSites::new(fixture.budget.clone())?;
        if index < fragments.len() {
            sites.observe(
                &source["choices"][0]["delta"]["content"],
                SourceClass::CurrentInput,
                if immutable {
                    EditConstraint::Immutable
                } else {
                    EditConstraint::Editable
                },
            )?;
        }
        frames.push(
            NativeSseFrameObservation::new(
                &wire[range],
                Some(source),
                sites,
                fixture.budget.clone(),
            )
            .map_err(|_| "native_sse_handoff_frame_rejected")?,
        );
    }
    frames.push(
        NativeSseFrameObservation::new(
            &wire[done_start..],
            None,
            TextSites::new(fixture.budget.clone())?,
            fixture.budget.clone(),
        )
        .map_err(|_| "native_sse_handoff_done_rejected")?,
    );
    let refs: Vec<_> = frames.iter().collect();
    let observed = NativeSseObservation::new(
        wire.as_bytes(),
        &refs,
        chat(),
        &fixture.lease,
        &fixture.binding,
        fixture.budget.clone(),
    )
    .map_err(|_| "native_sse_handoff_observation_rejected")?;
    let selections: Vec<_> = sources[..fragments.len()]
        .iter()
        .enumerate()
        .map(|(i, v)| NativeSseSnapshotSelection::new(i, &v["choices"][0]["delta"]))
        .collect();
    let nodes: Vec<_> = sources[..fragments.len()]
        .iter()
        .map(|v| &v["choices"][0]["delta"]["content"])
        .collect();
    let foreign = Value::String(fragments[0].to_owned());
    let mut declared = nodes.clone();
    match coverage {
        1 => {
            declared.pop();
        }
        2 => declared.push(nodes[0]),
        3 => declared.reverse(),
        4 => declared[0] = &foreign,
        _ => {}
    }
    let streams = [NativeSseTextStream::new(&declared)];
    let stage = stage_native_sse_with_lifecycle(
        &observed,
        &selections,
        &streams,
        &fixture.lease,
        &fixture.binding,
        fixture.budget.clone(),
        pair,
        lifecycle_attempt,
    )
    .map_err(|_| "native_sse_handoff_stage_rejected")?;
    let full = fragments.concat();
    let expected = if fixture.lease.is_empty() {
        0
    } else {
        full.matches(&fixture.reference).count()
    };
    assert!(
        stage.restored_occurrences() == expected
            && stage.output_tokens().len() == usize::from(expected > 0),
        "native_sse_handoff_dependency_ledger_mismatch"
    );
    finalize_native_sse(stage, &fixture.lease, proof, &fixture.attempt, 1024 * 1024)
        .map_err(|_| "native_sse_handoff_finalization_rejected".into())
}

#[test]
fn c5_native_execution_sse_lifecycle_gate_and_attempt_are_not_bypassed() -> TestResult {
    use super::{
        native_test_support::chat,
        restoration_contract::{ProtocolPair, UpstreamProtocol},
        restoration_core::{LifecycleOperation, ResponseLifecycle},
    };
    use crate::proxy::protocol_bridge::Protocol;
    let pair = ProtocolPair::new(chat(), UpstreamProtocol::new(Protocol::Chat));
    for operation in [
        LifecycleOperation::Begin,
        LifecycleOperation::OpenText,
        LifecycleOperation::Delta,
        LifecycleOperation::Done,
        LifecycleOperation::Hold,
        LifecycleOperation::Finish,
    ] {
        let fixture = NativeFixture::new(true)?;
        let budget = fixture.budget.clone();
        let baseline = budget.used();
        let guard = ResponseLifecycle::reject_operation_for_test(operation);
        let result = execute_sse_handoff(&fixture, &[&fixture.reference], pair, &fixture.attempt);
        assert!(result.is_err(), "native_sse_lifecycle_gate_bypassed");
        drop(result);
        drop(guard);
        assert!(
            fixture.counts()? == (0, 0, 0) && budget.used() == baseline,
            "native_sse_lifecycle_failure_not_atomic"
        );
        drop(fixture);
        assert!(budget.used() == 0, "native_sse_lifecycle_charge_retained");
    }
    Ok(())
}

#[test]
fn c5_native_execution_sse_lifecycle_identity_handoff_is_sealed() -> TestResult {
    use super::{
        native_test_support::chat,
        restoration_contract::{ClientProtocol, ProtocolPair, UpstreamProtocol},
        restoration_core::AttemptProvenance,
    };
    use crate::proxy::protocol_bridge::Protocol;
    let pair = ProtocolPair::new(chat(), UpstreamProtocol::new(Protocol::Chat));
    for kind in 0..3 {
        let fixture = NativeFixture::new(true)?;
        let budget = fixture.budget.clone();
        let foreign = AttemptProvenance::new(&budget)?;
        let selected_pair = match kind {
            1 => ProtocolPair::new(
                ClientProtocol::new(Protocol::Anthropic),
                UpstreamProtocol::new(Protocol::Chat),
            ),
            2 => ProtocolPair::new(chat(), UpstreamProtocol::new(Protocol::Anthropic)),
            _ => pair,
        };
        let result = execute_sse_handoff(
            &fixture,
            &[&fixture.reference],
            selected_pair,
            if kind == 0 {
                &foreign
            } else {
                &fixture.attempt
            },
        );
        assert!(
            result.is_err(),
            "native_sse_lifecycle_foreign_attempt_or_pair_released"
        );
        drop(result);
        drop(foreign);
        assert!(
            fixture.counts()? == (0, 0, 0),
            "native_sse_lifecycle_foreign_published"
        );
        drop(fixture);
        assert!(
            budget.used() == 0,
            "native_sse_lifecycle_foreign_charge_retained"
        );
    }
    Ok(())
}

#[test]
fn c5_native_execution_sse_lifecycle_cross_frame_reference_handoff() -> TestResult {
    use super::{
        native_test_support::chat,
        restoration_contract::{ProtocolPair, UpstreamProtocol},
    };
    use crate::proxy::protocol_bridge::Protocol;
    let pair = ProtocolPair::new(chat(), UpstreamProtocol::new(Protocol::Chat));
    let fixture = NativeFixture::new(true)?;
    let budget = fixture.budget.clone();
    let split = fixture.reference.len() / 2;
    let fragments = [&fixture.reference[..split], &fixture.reference[split..]];
    let finalized = execute_sse_handoff(&fixture, &fragments, pair, &fixture.attempt)?;
    assert!(
        fixture.counts()? == (0, 1, 1),
        "native_sse_handoff_early_publication"
    );
    let mut delivery = finalized.into_delivery_stream();
    let mut restored = String::new();
    for index in 0..4 {
        let chunk = futures::executor::block_on(delivery.next())
            .ok_or("native_sse_handoff_missing_frame")??;
        assert!(
            fixture.counts()? == (0, 1, 1),
            "native_sse_handoff_last_yield_activated"
        );
        if index < 2 {
            let data = std::str::from_utf8(&chunk)?
                .lines()
                .find_map(|line| line.strip_prefix("data: "))
                .ok_or("native_sse_handoff_data_missing")?;
            let parsed: Value = serde_json::from_str(data)?;
            restored.push_str(
                parsed["choices"][0]["delta"]["content"]
                    .as_str()
                    .ok_or("native_sse_handoff_text_missing")?,
            );
            assert!(
                parsed["extension"]["same_value"].as_str() == Some(fragments[index]),
                "native_sse_handoff_equal_sibling_changed"
            );
        }
    }
    assert!(
        restored == TEXT,
        "native_sse_lifecycle_cross_frame_reference_not_restored"
    );
    assert!(
        futures::executor::block_on(delivery.next()).is_none(),
        "native_sse_handoff_extra_frame"
    );
    assert!(
        fixture.counts()? == (1, 0, 0),
        "native_sse_handoff_continuation_missing"
    );
    drop(delivery);
    drop(fixture);
    assert!(
        budget.used() == 0,
        "native_sse_handoff_delivery_charge_retained"
    );
    Ok(())
}

#[test]
fn c5_native_execution_sse_lifecycle_coverage_immutable_and_all_reference_splits() -> TestResult {
    use super::{
        native_test_support::chat,
        restoration_contract::{ProtocolPair, UpstreamProtocol},
    };
    use crate::proxy::protocol_bridge::Protocol;
    let pair = ProtocolPair::new(chat(), UpstreamProtocol::new(Protocol::Chat));
    let fixture = NativeFixture::new(true)?;
    let budget = fixture.budget.clone();
    let baseline = budget.used();
    let split = fixture.reference.len() / 2;
    let fragments = [&fixture.reference[..split], &fixture.reference[split..]];
    for coverage in 1..=4 {
        let result = execute_sse_handoff_selected(
            &fixture,
            &fragments,
            pair,
            &fixture.attempt,
            coverage,
            false,
        );
        assert!(
            result.is_err(),
            "native_sse_handoff_invalid_coverage_accepted"
        );
        drop(result);
        assert!(
            fixture.counts()? == (0, 0, 0) && budget.used() == baseline,
            "native_sse_handoff_coverage_not_atomic"
        );
    }
    let result =
        execute_sse_handoff_selected(&fixture, &fragments, pair, &fixture.attempt, 0, true);
    assert!(result.is_err(), "native_sse_handoff_immutable_edited");
    drop(result);
    assert!(
        fixture.counts()? == (0, 0, 0) && budget.used() == baseline,
        "native_sse_handoff_immutable_not_atomic"
    );
    let token_len = fixture.reference.len();
    drop(fixture);
    assert!(
        budget.used() == 0,
        "native_sse_handoff_coverage_charge_retained"
    );
    for split in 1..token_len {
        let fixture = NativeFixture::new(true)?;
        let budget = fixture.budget.clone();
        let fragments = [&fixture.reference[..split], &fixture.reference[split..]];
        let finalized = execute_sse_handoff(&fixture, &fragments, pair, &fixture.attempt)?;
        let delivery =
            futures::executor::block_on(record_delivery(finalized.into_delivery_stream()))?;
        let mut restored = String::new();
        for chunk in delivery.iter().take(2) {
            let data = std::str::from_utf8(chunk)?
                .lines()
                .find_map(|line| line.strip_prefix("data: "))
                .ok_or("native_sse_handoff_split_data_missing")?;
            let value: Value = serde_json::from_str(data)?;
            restored.push_str(
                value["choices"][0]["delta"]["content"]
                    .as_str()
                    .ok_or("native_sse_handoff_split_text_missing")?,
            );
        }
        assert!(
            restored == TEXT && fixture.counts()? == (1, 0, 0),
            "native_sse_handoff_split_mismatch"
        );
        drop(delivery);
        drop(fixture);
        assert!(
            budget.used() == 0,
            "native_sse_handoff_split_charge_retained"
        );
    }
    Ok(())
}

#[test]
fn c5_native_execution_sse_lifecycle_budget_parse_and_zero_map_characterization() -> TestResult {
    use super::{
        native_test_support::chat,
        payload_json::test_support,
        restoration_contract::{ProtocolPair, UpstreamProtocol},
        restoration_core::{LifecycleOperation, ResponseLifecycle},
    };
    use crate::proxy::protocol_bridge::Protocol;
    let pair = ProtocolPair::new(chat(), UpstreamProtocol::new(Protocol::Chat));
    for phase in [
        test_support::Phase::BeforeParse,
        test_support::Phase::AfterParse,
    ] {
        let fixture = NativeFixture::new(true)?;
        let budget = fixture.budget.clone();
        let baseline = budget.used();
        let guard = test_support::install(move |observation| {
            if observation.phase == phase {
                test_support::Action::Fail
            } else {
                test_support::Action::Continue
            }
        });
        let result = execute_sse_handoff(&fixture, &[&fixture.reference], pair, &fixture.attempt);
        assert!(result.is_err(), "native_sse_handoff_parse_fault_released");
        drop(result);
        drop(guard);
        assert!(
            fixture.counts()? == (0, 0, 0) && budget.used() == baseline,
            "native_sse_handoff_parse_fault_not_atomic"
        );
        drop(fixture);
        assert!(
            budget.used() == 0,
            "native_sse_handoff_parse_charge_retained"
        );
    }
    let fixture = NativeFixture::new(true)?;
    let budget = fixture.budget.clone();
    let baseline = budget.used();
    let reservation = budget.reserve(LIMIT - baseline)?;
    let result = execute_sse_handoff(&fixture, &[&fixture.reference], pair, &fixture.attempt);
    assert!(result.is_err(), "native_sse_handoff_budget_released");
    drop(result);
    drop(reservation);
    assert!(
        fixture.counts()? == (0, 0, 0) && budget.used() == baseline,
        "native_sse_handoff_budget_not_atomic"
    );
    drop(fixture);
    assert!(
        budget.used() == 0,
        "native_sse_handoff_budget_charge_retained"
    );
    let fixture = NativeFixture::new(false)?;
    let budget = fixture.budget.clone();
    let guard = ResponseLifecycle::reject_operation_for_test(LifecycleOperation::Hold);
    let finalized = execute_sse_handoff_selected(
        &fixture,
        &[&fixture.reference],
        pair,
        &fixture.attempt,
        0,
        true,
    )?;
    drop(guard);
    let delivery = futures::executor::block_on(record_delivery(finalized.into_delivery_stream()))?;
    assert!(
        fixture.counts()? == (1, 0, 0),
        "native_sse_handoff_zero_map_not_published"
    );
    drop(delivery);
    drop(fixture);
    assert!(
        budget.used() == 0,
        "native_sse_handoff_zero_map_charge_retained"
    );
    Ok(())
}

/// Two adapter-declared decoded streams, interleaved on the original native wire.
/// The synthetic tool is observed only; nothing in this fixture executes it.
fn execute_interleaved_sse_handoff<G>(
    fixture: &NativeFixture,
    before_finalization: impl FnOnce() -> G,
) -> TestResult<super::response_restoration::FinalizedNativeSse> {
    use super::{
        http_restoration::test_transport,
        native_test_support::chat,
        response_restoration::finalize_native_sse,
        restoration_contract::{
            EditConstraint, ProtocolPair, SourceClass, TextSites, UpstreamProtocol,
        },
        restoration_core::{
            stage_native_sse_with_lifecycle, NativeSseFrameObservation, NativeSseObservation,
            NativeSseSnapshotSelection, NativeSseTextStream,
        },
    };
    use crate::proxy::protocol_bridge::Protocol;
    let split = fixture.reference.len() / 2;
    let content = [
        fixture.reference[..split].to_owned(),
        format!("{}|{}", &fixture.reference[split..], fixture.reference),
    ];
    let arguments = [
        format!("{{\"value\":\"{}", &fixture.reference[..split]),
        format!("{}\"}}", &fixture.reference[split..]),
    ];
    let sources = [
        serde_json::json!({"id":"synthetic-response","object":"chat.completion.chunk",
            "choices":[{"index":0,"delta":{"role":"assistant","content":content[0]},"finish_reason":null}]}),
        serde_json::json!({"id":"synthetic-response","object":"chat.completion.chunk",
            "choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"id":"synthetic-call","type":"function",
                "function":{"name":"synthetic-observation-only","arguments":arguments[0]}}]},"finish_reason":null}]}),
        serde_json::json!({"id":"synthetic-response","object":"chat.completion.chunk",
            "choices":[{"index":0,"delta":{"content":content[1]},"finish_reason":null}]}),
        serde_json::json!({"id":"synthetic-response","object":"chat.completion.chunk",
            "choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"function":{"arguments":arguments[1]}}]},"finish_reason":null}]}),
        serde_json::json!({"id":"synthetic-response","object":"chat.completion.chunk",
            "choices":[{"index":0,"delta":{},"finish_reason":"tool_calls"}]}),
    ];
    let mut wire = String::new();
    let mut ranges = Vec::new();
    for source in &sources {
        let start = wire.len();
        wire.push_str(&format!("data: {source}\n\n"));
        ranges.push(start..wire.len());
    }
    let done_start = wire.len();
    wire.push_str("data: [DONE]\n\n");
    let bytes = Bytes::copy_from_slice(wire.as_bytes());
    let (original, _) = ScriptedSource::split(bytes.clone(), &[1, bytes.len() / 2])?;
    let (client, _) = ScriptedSource::split(bytes, &[0, 2])?;
    let proof = futures::executor::block_on(test_transport::native_sse_from_streams(
        &fixture.attempt,
        &fixture.binding,
        &fixture.budget,
        original,
        client,
    ))
    .map_err(|_| "native_interleaved_http_proof_rejected")?;
    let content_nodes = [
        &sources[0]["choices"][0]["delta"]["content"],
        &sources[2]["choices"][0]["delta"]["content"],
    ];
    let tool_nodes = [
        &sources[1]["choices"][0]["delta"]["tool_calls"][0]["function"]["arguments"],
        &sources[3]["choices"][0]["delta"]["tool_calls"][0]["function"]["arguments"],
    ];
    let nodes = [
        content_nodes[0],
        tool_nodes[0],
        content_nodes[1],
        tool_nodes[1],
    ];
    let mut frames = Vec::new();
    for (index, (source, range)) in sources.iter().zip(ranges).enumerate() {
        let mut sites = TextSites::new(fixture.budget.clone())?;
        if let Some(node) = nodes.get(index) {
            sites.observe(
                node,
                if index % 2 == 0 {
                    SourceClass::CurrentInput
                } else {
                    SourceClass::ToolArguments
                },
                EditConstraint::Editable,
            )?;
        }
        frames.push(
            NativeSseFrameObservation::new(
                &wire[range],
                Some(source),
                sites,
                fixture.budget.clone(),
            )
            .map_err(|_| "native_interleaved_frame_rejected")?,
        );
    }
    frames.push(
        NativeSseFrameObservation::new(
            &wire[done_start..],
            None,
            TextSites::new(fixture.budget.clone())?,
            fixture.budget.clone(),
        )
        .map_err(|_| "native_interleaved_done_rejected")?,
    );
    let refs: Vec<_> = frames.iter().collect();
    let observed = NativeSseObservation::new(
        wire.as_bytes(),
        &refs,
        chat(),
        &fixture.lease,
        &fixture.binding,
        fixture.budget.clone(),
    )
    .map_err(|_| "native_interleaved_observation_rejected")?;
    let selections: Vec<_> = sources[..4]
        .iter()
        .enumerate()
        .map(|(i, v)| NativeSseSnapshotSelection::new(i, &v["choices"][0]["delta"]))
        .collect();
    let streams = [
        NativeSseTextStream::new(&content_nodes),
        NativeSseTextStream::new(&tool_nodes),
    ];
    let stage = stage_native_sse_with_lifecycle(
        &observed,
        &selections,
        &streams,
        &fixture.lease,
        &fixture.binding,
        fixture.budget.clone(),
        ProtocolPair::new(chat(), UpstreamProtocol::new(Protocol::Chat)),
        &fixture.attempt,
    )
    .map_err(|_| "native_interleaved_stage_rejected")?;
    assert!(
        stage.restored_occurrences() == 3 && stage.output_tokens().len() == 1,
        "native_interleaved_duplicate_dependency_or_occurrence_mismatch"
    );
    assert!(
        fixture.counts()? == (0, 0, 0),
        "native_interleaved_staging_published"
    );
    let guard = before_finalization();
    let result = finalize_native_sse(stage, &fixture.lease, proof, &fixture.attempt, 1024 * 1024);
    drop(guard);
    result.map_err(|_| "native_interleaved_finalization_rejected".into())
}

#[test]
fn c5_native_execution_sse_interleaved_content_tool_and_duplicate_ledger_characterization(
) -> TestResult {
    let fixture = NativeFixture::new(true)?;
    let budget = fixture.budget.clone();
    let finalized = execute_interleaved_sse_handoff(&fixture, || ())?;
    let mut delivery = finalized.into_delivery_stream();
    let mut content = String::new();
    let mut arguments = String::new();
    for index in 0..6 {
        let chunk = futures::executor::block_on(delivery.next())
            .ok_or("native_interleaved_missing_frame")??;
        assert!(
            fixture.counts()? == (0, 1, 1),
            "native_interleaved_early_publication"
        );
        if index < 4 {
            let data = std::str::from_utf8(&chunk)?
                .lines()
                .find_map(|line| line.strip_prefix("data: "))
                .ok_or("native_interleaved_missing_data")?;
            let value: Value = serde_json::from_str(data)?;
            let delta = &value["choices"][0]["delta"];
            if index % 2 == 0 {
                content.push_str(
                    delta["content"]
                        .as_str()
                        .ok_or("native_interleaved_missing_content")?,
                );
            } else {
                arguments.push_str(
                    delta["tool_calls"][0]["function"]["arguments"]
                        .as_str()
                        .ok_or("native_interleaved_missing_arguments")?,
                );
            }
        }
    }
    let parsed: Value = serde_json::from_str(&arguments)?;
    assert!(
        content == format!("{TEXT}|{TEXT}") && parsed["value"].as_str() == Some(TEXT),
        "native_interleaved_restoration_mismatch"
    );
    assert!(
        futures::executor::block_on(delivery.next()).is_none() && fixture.counts()? == (1, 0, 0),
        "native_interleaved_final_continuation_missing"
    );
    drop(delivery);
    drop(fixture);
    assert!(budget.used() == 0, "native_interleaved_charge_retained");
    Ok(())
}

#[test]
fn c5_native_execution_sse_post_staging_finalizer_faults_and_budget_characterization() -> TestResult
{
    use super::payload_json::test_support;
    use std::{cell::Cell, rc::Rc};
    for phase in [
        test_support::Phase::BeforeParse,
        test_support::Phase::AfterParse,
    ] {
        let fixture = NativeFixture::new(true)?;
        let budget = fixture.budget.clone();
        let baseline = budget.used();
        let seen = Rc::new(Cell::new(0));
        let recorded = Rc::clone(&seen);
        let result = execute_interleaved_sse_handoff(&fixture, || {
            test_support::install(move |event| {
                if event.phase == phase {
                    recorded.set(recorded.get() + 1);
                    test_support::Action::Fail
                } else {
                    test_support::Action::Continue
                }
            })
        });
        assert!(
            result.is_err() && seen.get() > 0,
            "native_post_staging_finalizer_fault_not_reached"
        );
        drop(result);
        assert!(
            fixture.counts()? == (0, 0, 0) && budget.used() == baseline,
            "native_post_staging_finalizer_fault_not_atomic"
        );
        drop(fixture);
        assert!(
            budget.used() == 0,
            "native_post_staging_finalizer_fault_charge_retained"
        );
    }
    let fixture = NativeFixture::new(true)?;
    let budget = fixture.budget.clone();
    let baseline = budget.used();
    let result = execute_interleaved_sse_handoff(&fixture, || {
        let reservation = budget.reserve(budget.limit() - budget.used());
        assert!(
            reservation.is_ok(),
            "native_post_staging_budget_fixture_rejected"
        );
        reservation
    });
    assert!(
        result.is_err(),
        "native_post_staging_finalizer_budget_released"
    );
    drop(result);
    assert!(
        fixture.counts()? == (0, 0, 0) && budget.used() == baseline,
        "native_post_staging_finalizer_budget_not_atomic"
    );
    drop(fixture);
    assert!(
        budget.used() == 0,
        "native_post_staging_finalizer_budget_charge_retained"
    );
    Ok(())
}

#[test]
fn c5_native_execution_context_degradation_expired_history_keeps_current_json_and_sse() -> TestResult
{
    use super::{
        http_restoration::test_transport,
        native_test_support::{chat, ExpiredNativeHistory, NativeSseEntity},
    };
    for sse in [false, true] {
        let history = ExpiredNativeHistory::new()?;
        let (mut lease, binding) = history.current()?;
        let before = history.store.stats()?;
        let mut wrong = binding.clone();
        wrong.generation[0] ^= 1;
        assert!(
            test_transport::optional_native_delivered_history(
                &mut lease,
                &wrong,
                chat(),
                &history.delivered,
            )
            .is_err(),
            "native_bad_current_binding_was_degraded"
        );
        let available = test_transport::optional_native_delivered_history(
            &mut lease,
            &binding,
            chat(),
            &history.delivered,
        )
        .map_err(|_| "native_optional_history_decision_rejected")?;
        assert!(
            !available && lease.is_empty(),
            "native_expired_history_gained_authority"
        );
        assert!(
            lease.resolve_if_authorized(&history.reference)?.is_none()
                && history.store.stats()? == before,
            "native_history_miss_changed_authority"
        );
        let fixture = history.prepare_current(lease)?;
        let budget = fixture.budget.clone();
        assert!(
            fixture.lease.resolve(&fixture.reference)? == TEXT
                && fixture
                    .lease
                    .resolve_if_authorized(&history.reference)?
                    .is_none(),
            "native_current_mapping_or_history_boundary_lost"
        );
        if sse {
            let entity = NativeSseEntity::new(&fixture);
            let proof = futures::executor::block_on(test_transport::native_sse_proof(
                &fixture.attempt,
                &fixture.binding,
                &budget,
                Bytes::copy_from_slice(entity.wire.as_bytes()),
                Bytes::copy_from_slice(entity.wire.as_bytes()),
            ))
            .map_err(|_| "native_degraded_sse_proof_rejected")?;
            let finalized = fixture.finalize_sse(&entity, proof)?;
            let mut stream = finalized.into_delivery_stream();
            let mut chunks = Vec::new();
            for _ in 0..3 {
                chunks.push(
                    futures::executor::block_on(stream.next())
                        .ok_or("native_degraded_sse_chunk_missing")??,
                );
                assert!(
                    fixture.counts()? == (0, 1, 1),
                    "native_degraded_sse_published_early"
                );
            }
            assert!(
                futures::executor::block_on(stream.next()).is_none()
                    && fixture.counts()? == (1, 0, 0),
                "native_degraded_sse_not_published_on_resume"
            );
            let joined: Vec<u8> = chunks
                .iter()
                .flat_map(|chunk| chunk.iter().copied())
                .collect();
            let first = std::str::from_utf8(&joined)?
                .lines()
                .find_map(|line| line.strip_prefix("data:"))
                .ok_or("native_degraded_sse_data_missing")?;
            let body: Value = serde_json::from_str(first.trim())?;
            assert!(
                body["choices"][0]["delta"]["content"].as_str() == Some(TEXT),
                "native_degraded_sse_current_restoration_missing"
            );
        } else {
            let source = fixture.json_source();
            let wire = Bytes::from(serde_json::to_vec(&source)?);
            let (transport, _) = ScriptedSource::split(wire.clone(), &[1])?;
            let proof = futures::executor::block_on(fixture.json_proof(&source, transport))
                .map_err(|_| "native_degraded_json_proof_rejected")?;
            let finalized = fixture.finalize_json(&source, wire, proof)?;
            assert!(
                fixture.counts()? == (1, 0, 0),
                "native_degraded_json_activation_missing"
            );
            let chunks =
                futures::executor::block_on(record_delivery(finalized.into_delivery_stream()))?;
            let body: Value = serde_json::from_slice(&chunks[0])?;
            assert!(
                body["choices"][0]["message"]["content"].as_str() == Some(TEXT),
                "native_degraded_json_current_restoration_missing"
            );
        }
        drop(fixture);
        assert!(
            budget.used() == 0,
            "native_degraded_delivery_charge_retained"
        );
    }
    Ok(())
}

#[test]
fn c5_native_execution_context_degradation_request_failures_stay_closed() -> TestResult {
    use super::{
        http_restoration::test_transport,
        native_test_support::{chat, ExpiredNativeHistory},
        payload_budget::PayloadBudget,
        payload_buffer::test_support as buffer_events,
        payload_json::test_support as json_events,
        request_plan::PlanError,
        request_preparation::prepare_observed_request,
        restoration_contract::{EditConstraint, RequestObservation, SourceClass},
        runtime::{CompiledCustomRule, SecurityRuntimeConfig},
        tokenization::TokenError,
        types::{AuditAction, RiskLevel},
    };
    use std::{cell::Cell, rc::Rc};
    for mode in 0..4 {
        let history = ExpiredNativeHistory::new()?;
        let (mut lease, binding) = history.current()?;
        assert!(
            !test_transport::optional_native_delivered_history(
                &mut lease,
                &binding,
                chat(),
                &history.delivered,
            )
            .map_err(|_| "native_failure_history_decision_rejected")?,
            "native_failure_history_available"
        );
        let budget = PayloadBudget::with_limit(LIMIT);
        let body = serde_json::json!({"messages":[{"role":"user","content":TEXT}]});
        let mut observed = RequestObservation::new(&body, chat(), budget.clone())?;
        observed.observe_text(
            &body["messages"][0]["content"],
            SourceClass::CurrentInput,
            EditConstraint::Editable,
        )?;
        let before = history.store.stats()?;
        let baseline = budget.used();
        let runtime = if mode == 0 {
            SecurityRuntimeConfig {
                custom_rules: vec![CompiledCustomRule {
                    id: "synthetic-degraded-block".into(),
                    name: "Synthetic block".into(),
                    rule_type: "regex".into(),
                    pattern: "13900000000".into(),
                    regex: Some(regex::Regex::new("13900000000")?),
                    risk_level: RiskLevel::High,
                    action: AuditAction::Block,
                    enabled: true,
                }],
                ..Default::default()
            }
        } else {
            SecurityRuntimeConfig::default()
        };
        let reserved = if mode == 1 {
            Some(budget.reserve(budget.limit() - budget.used())?)
        } else {
            None
        };
        let ready = Rc::new(Cell::new(false));
        let seen = Rc::clone(&ready);
        let json_hook = json_events::install(move |event| {
            if event.phase == json_events::Phase::AfterBuild {
                seen.set(true);
            }
            json_events::Action::Continue
        });
        let denied = Rc::new(Cell::new(false));
        let seen = Rc::clone(&denied);
        let candidate_ready = Rc::clone(&ready);
        let buffer_hook = buffer_events::install(move |event| {
            if mode == 2
                && candidate_ready.get()
                && event.phase == buffer_events::Phase::BeforeAllocation
            {
                seen.set(true);
                buffer_events::Action::FailAllocation
            } else {
                buffer_events::Action::Continue
            }
        });
        let called = Cell::new(false);
        let result = prepare_observed_request(
            &mut lease,
            &observed,
            &runtime,
            &["chinese_phone_number"],
            &[],
            |_, _, _| {
                called.set(true);
                Err::<(), _>(TokenError::RedactionFailed)
            },
        );
        drop(buffer_hook);
        drop(json_hook);
        drop(reserved);
        let expected = match mode {
            0 => matches!(result, Err(PlanError::Blocked)),
            1 => matches!(result, Err(PlanError::Token(TokenError::CapacityExceeded))),
            2 => matches!(result, Err(PlanError::Token(TokenError::Unavailable))) && denied.get(),
            _ => {
                matches!(result, Err(PlanError::Token(TokenError::RedactionFailed))) && called.get()
            }
        };
        assert!(
            expected && called.get() == (mode == 3),
            "native_degraded_failure_not_preserved"
        );
        drop(result);
        assert!(
            lease.is_empty()
                && lease.resolve_if_authorized(&history.reference)?.is_none()
                && history.store.stats()? == before
                && budget.used() == baseline,
            "native_degraded_failure_not_atomic"
        );
        drop(observed);
        assert!(
            budget.used() == 0,
            "native_degraded_failure_charge_retained"
        );
    }
    Ok(())
}

#[test]
fn c5_native_execution_context_degradation_mandatory_response_failure_never_publishes() -> TestResult
{
    use super::{
        http_restoration::test_transport,
        native_test_support::{chat, ExpiredNativeHistory},
        payload_json::test_support,
    };
    use std::{cell::Cell, rc::Rc};
    let history = ExpiredNativeHistory::new()?;
    let (mut lease, binding) = history.current()?;
    assert!(
        !test_transport::optional_native_delivered_history(
            &mut lease,
            &binding,
            chat(),
            &history.delivered,
        )
        .map_err(|_| "native_restore_history_decision_rejected")?,
        "native_restore_history_available"
    );
    let fixture = history.prepare_current(lease)?;
    let budget = fixture.budget.clone();
    let baseline = budget.used();
    let source = fixture.json_source();
    let wire = Bytes::from(serde_json::to_vec(&source)?);
    let (transport, _) = ScriptedSource::split(wire.clone(), &[1])?;
    let proof = futures::executor::block_on(fixture.json_proof(&source, transport))
        .map_err(|_| "native_restore_original_proof_rejected")?;
    let reached = Rc::new(Cell::new(false));
    let seen = Rc::clone(&reached);
    let hook = test_support::install(move |event| {
        if event.phase == test_support::Phase::BeforeBuild {
            seen.set(true);
            test_support::Action::Fail
        } else {
            test_support::Action::Continue
        }
    });
    let result = fixture.finalize_json(&source, wire, proof);
    drop(hook);
    assert!(
        reached.get() && result.is_err(),
        "native_degraded_mandatory_restoration_escaped"
    );
    drop(result);
    assert!(
        fixture.counts()? == (0, 0, 0) && budget.used() == baseline,
        "native_degraded_failed_restoration_published_or_leaked"
    );
    drop(fixture);
    assert!(
        budget.used() == 0,
        "native_degraded_response_charge_retained"
    );
    Ok(())
}
