//! Sealed native SSE finalization: real proof, restored wire, and delivery continuation.
use super::*;
use crate::proxy::{
    protocol_bridge::Protocol,
    security::{
        http_restoration::test_transport,
        restoration_contract::{
            ClientProtocol, ContinuityScope, EditConstraint, IngressNamespace, SourceClass,
            TextSites,
        },
        restoration_core::{
            stage_native_sse, NativeSseFrameObservation, NativeSseObservation,
            NativeSseSnapshotSelection,
        },
        tokenization::{publication_test_support, DlpTokenStore, TokenCategory, TokenFragment},
    },
};
use futures::StreamExt;

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;
const LIMIT: usize = 16 * 1024 * 1024;
const TEXT: &str = "Synthetic restored SSE text🔒";

struct Fixture {
    store: DlpTokenStore,
    lease: DlpRequestLease,
    binding: ActualAttemptBinding,
    budget: PayloadBudget,
    attempt: AttemptProvenance,
    sources: [Value; 2],
    entity: String,
    first_end: usize,
    second_end: usize,
}
impl Fixture {
    fn new(mapped: bool, replacement: &str, tool: bool) -> TestResult<Self> {
        let store = DlpTokenStore::new();
        let mut lease = store.admit_scoped_published_policy(
            ContinuityScope::Native {
                client: chat(),
                ingress: IngressNamespace::Direct,
            },
            "synthetic-sse-finalization-app",
            Some("synthetic-sse-finalization-session"),
            1,
        )?;
        let text = if mapped {
            lease.prepare(
                &[TokenFragment {
                    raw: replacement,
                    whole_span_categories: &[TokenCategory::Phone],
                }],
                &[],
                |tokens| Ok(tokens[0].clone()),
            )?
        } else {
            TEXT.to_owned()
        };
        let binding = lease.bind_actual_attempt(
            "synthetic-provider",
            "openai_chat",
            "https://synthetic.invalid/v1/chat/completions",
            Some("synthetic-principal"),
        )?;
        let budget = PayloadBudget::with_limit(LIMIT);
        let attempt = AttemptProvenance::new(&budget)?;
        let delta = if tool {
            json!({"role":"assistant","tool_calls":[{"index":0,"id":"synthetic-call","type":"function",
                "function":{"name":"synthetic-tool","arguments":format!("{{\"value\":\"{text}\"}}")}}]})
        } else {
            json!({"role":"assistant","content":text})
        };
        let sources = [
            json!({"id":"synthetic-response","object":"chat.completion.chunk","model":"synthetic-model",
                "choices":[{"index":0,"delta":delta,"finish_reason":null}],
                "synthetic_extension":{"types":[true,null,7,1.0]}}),
            json!({"id":"synthetic-response","object":"chat.completion.chunk","choices":[{"index":0,"delta":{},
                "finish_reason":if tool { "tool_calls" } else { "stop" }}]}),
        ];
        let first = format!(
            ": synthetic comment\r\nid: synthetic-frame\r\ndata: {}\r\n\r\n",
            sources[0]
        );
        let second = format!("data:{}\n\n", sources[1]);
        let first_end = first.len();
        let second_end = first_end + second.len();
        let entity = format!("{first}{second}data: [DONE]\n\n");
        Ok(Self {
            store,
            lease,
            binding,
            budget,
            attempt,
            sources,
            entity,
            first_end,
            second_end,
        })
    }
    fn frames(&self, tool: bool) -> TestResult<[NativeSseFrameObservation<'_>; 3]> {
        let mut sites = TextSites::new(self.budget.clone())?;
        sites.observe(
            if tool {
                &self.sources[0]["choices"][0]["delta"]["tool_calls"][0]["function"]["arguments"]
            } else {
                &self.sources[0]["choices"][0]["delta"]["content"]
            },
            if tool {
                SourceClass::ToolArguments
            } else {
                SourceClass::CurrentInput
            },
            EditConstraint::Editable,
        )?;
        Ok([
            require(NativeSseFrameObservation::new(
                &self.entity[..self.first_end],
                Some(&self.sources[0]),
                sites,
                self.budget.clone(),
            ))?,
            require(NativeSseFrameObservation::new(
                &self.entity[self.first_end..self.second_end],
                Some(&self.sources[1]),
                TextSites::new(self.budget.clone())?,
                self.budget.clone(),
            ))?,
            require(NativeSseFrameObservation::new(
                &self.entity[self.second_end..],
                None,
                TextSites::new(self.budget.clone())?,
                self.budget.clone(),
            ))?,
        ])
    }
    fn finalize(
        &self,
        tool: bool,
        proof_binding: &ActualAttemptBinding,
        proof_attempt: &AttemptProvenance,
        max: usize,
    ) -> TestResult<Result<FinalizedNativeSse, RestoreError>> {
        self.finalize_with(tool, proof_binding, proof_attempt, max, || ())
    }
    fn finalize_with<G>(
        &self,
        tool: bool,
        proof_binding: &ActualAttemptBinding,
        proof_attempt: &AttemptProvenance,
        max: usize,
        before_finalization: impl FnOnce() -> G,
    ) -> TestResult<Result<FinalizedNativeSse, RestoreError>> {
        let proof = require(futures::executor::block_on(
            test_transport::native_sse_proof(
                proof_attempt,
                proof_binding,
                &self.budget,
                Bytes::copy_from_slice(self.entity.as_bytes()),
                Bytes::copy_from_slice(self.entity.as_bytes()),
            ),
        ))?;
        let frames = self.frames(tool)?;
        let observed = require(NativeSseObservation::new(
            self.entity.as_bytes(),
            &[&frames[0], &frames[1], &frames[2]],
            chat(),
            &self.lease,
            &self.binding,
            self.budget.clone(),
        ))?;
        let stage = require(stage_native_sse(
            &observed,
            &[NativeSseSnapshotSelection::new(
                0,
                &self.sources[0]["choices"][0]["delta"],
            )],
            &self.lease,
            &self.binding,
            self.budget.clone(),
        ))?;
        let guard = before_finalization();
        let result = finalize_native_sse(stage, &self.lease, proof, &self.attempt, max);
        drop(guard);
        Ok(result)
    }
    fn counts(&self) -> TestResult<(usize, usize, usize)> {
        let (published, pending, pins, ..) = publication_test_support::counts(&self.store)?;
        Ok((published, pending, pins))
    }
}
fn chat() -> ClientProtocol {
    ClientProtocol::new(Protocol::Chat)
}
fn require<T>(r: Result<T, RestoreError>) -> TestResult<T> {
    r.map_err(|_| "synthetic_sse_finalization_rejected".into())
}

#[test]
fn c5_native_sse_finalization_mapped_and_zero_require_final_delivery_resume() -> TestResult {
    for mapped in [true, false] {
        let fixture = Fixture::new(mapped, TEXT, false)?;
        let baseline = fixture.budget.used();
        let finalized =
            require(fixture.finalize(false, &fixture.binding, &fixture.attempt, 1024 * 1024)?)?;
        assert!(
            fixture.counts()? == (0, 1, 1),
            "native_sse_prepared_slot_missing_or_early_publication"
        );
        let mut stream = finalized.into_delivery_stream();
        let mut all = Vec::new();
        for _ in 0..3 {
            let frame = futures::executor::block_on(stream.next())
                .ok_or("synthetic_delivery_missing_frame")??;
            all.extend_from_slice(&frame);
            assert!(
                fixture.counts()? == (0, 1, 1),
                "native_sse_published_before_delivery_resume"
            );
        }
        assert!(
            futures::executor::block_on(stream.next()).is_none(),
            "native_sse_extra_delivery"
        );
        assert!(
            fixture.counts()? == (1, 0, 0),
            "native_sse_resume_did_not_publish"
        );
        drop(stream);
        let wire = std::str::from_utf8(&all)?;
        assert!(
            wire.contains(TEXT)
                && wire.starts_with(": synthetic comment\r\nid: synthetic-frame\r\n")
                && wire.ends_with("data: [DONE]\n\n"),
            "native_sse_wire_changed"
        );
        if !mapped {
            assert!(
                all == fixture.entity.as_bytes(),
                "native_sse_unchanged_bytes_lost"
            );
        }
        assert!(
            fixture.budget.used() == baseline,
            "native_sse_delivery_budget_leaked"
        );
    }
    Ok(())
}

#[test]
fn c5_native_sse_finalization_drop_at_every_yield_never_publishes() -> TestResult {
    for mapped in [true, false] {
        for yields in 0..=3 {
            let fixture = Fixture::new(mapped, TEXT, false)?;
            let baseline = fixture.budget.used();
            let finalized = require(fixture.finalize(
                false,
                &fixture.binding,
                &fixture.attempt,
                1024 * 1024,
            )?)?;
            let mut stream = finalized.into_delivery_stream();
            for _ in 0..yields {
                let _ = futures::executor::block_on(stream.next())
                    .ok_or("synthetic_cancel_frame_missing")??;
                assert!(
                    fixture.counts()? == (0, 1, 1),
                    "native_sse_cancel_published_early"
                );
            }
            drop(stream);
            assert!(
                fixture.counts()? == (0, 0, 0),
                "native_sse_cancel_stranded_slot"
            );
            assert!(
                fixture.budget.used() == baseline,
                "native_sse_cancel_budget_leaked"
            );
        }
    }
    Ok(())
}

#[test]
fn c5_native_sse_finalization_restored_tool_arguments_revalidated_before_release() -> TestResult {
    for replacement in ["synthetic-safe", "synthetic-\"invalid-json"] {
        let fixture = Fixture::new(true, replacement, true)?;
        let baseline = fixture.budget.used();
        let result = fixture.finalize(true, &fixture.binding, &fixture.attempt, 1024 * 1024)?;
        if replacement == "synthetic-safe" {
            let finalized = require(result)?;
            assert!(
                fixture.counts()? == (0, 1, 1),
                "native_sse_valid_tool_not_pending"
            );
            drop(finalized);
        } else {
            assert!(result.is_err(), "native_sse_invalid_restored_tool_released");
        }
        assert!(
            fixture.counts()? == (0, 0, 0),
            "native_sse_tool_failure_published"
        );
        assert!(
            fixture.budget.used() == baseline,
            "native_sse_tool_failure_leaked"
        );
    }
    Ok(())
}

#[test]
fn c5_native_sse_finalization_foreign_proof_and_size_gate_fail_closed() -> TestResult {
    let fixture = Fixture::new(false, TEXT, false)?;
    let foreign = Fixture::new(false, TEXT, false)?;
    let other_attempt = AttemptProvenance::new(&fixture.budget)?;
    let baseline = fixture.budget.used();
    for (binding, attempt, max) in [
        (&foreign.binding, &fixture.attempt, 1024 * 1024),
        (&fixture.binding, &other_attempt, 1024 * 1024),
        (&fixture.binding, &fixture.attempt, 1),
    ] {
        assert!(
            fixture.finalize(false, binding, attempt, max)?.is_err(),
            "native_sse_foreign_or_oversize_released"
        );
        assert!(
            fixture.counts()? == (0, 0, 0),
            "native_sse_rejection_published"
        );
        assert!(
            fixture.budget.used() == baseline,
            "native_sse_rejection_leaked"
        );
    }
    Ok(())
}

#[test]
fn c5_native_sse_finalization_budget_and_verifier_faults_refund_before_publication() -> TestResult {
    use crate::proxy::security::payload_buffer::test_support as buffer_events;
    use std::{cell::Cell, rc::Rc};
    for mapped in [true, false] {
        let fixture = Fixture::new(mapped, TEXT, false)?;
        let baseline = fixture.budget.used();
        let denied = fixture.finalize_with(
            false,
            &fixture.binding,
            &fixture.attempt,
            usize::MAX,
            || {
                let reservation = fixture
                    .budget
                    .reserve(fixture.budget.limit() - fixture.budget.used());
                assert!(
                    reservation.is_ok(),
                    "native_sse_exhaustion_fixture_rejected"
                );
                reservation
            },
        )?;
        assert!(
            denied.is_err() && fixture.counts()? == (0, 0, 0),
            "native_sse_budget_denial_released"
        );
        assert!(
            fixture.budget.used() == baseline,
            "native_sse_budget_denial_leaked"
        );
        for phase in [
            buffer_events::Phase::BeforeAllocation,
            buffer_events::Phase::BeforeCopy,
        ] {
            let observed = Rc::new(Cell::new(0));
            let seen = Rc::clone(&observed);
            let store = fixture.store.clone();
            let denied = fixture.finalize_with(
                false,
                &fixture.binding,
                &fixture.attempt,
                usize::MAX,
                || {
                    buffer_events::install(move |event| {
                        if event.phase == phase {
                            seen.set(seen.get() + 1);
                            assert!(
                                event.used >= event.requested,
                                "native_sse_verifier_copy_not_precharged"
                            );
                            let (published, pending, pins, ..) =
                                publication_test_support::counts(&store)
                                    .unwrap_or_else(|_| panic!("synthetic_count_unavailable"));
                            assert!(
                                (published, pending, pins) == (0, 0, 0),
                                "native_sse_slot_before_verifier"
                            );
                            buffer_events::Action::FailAllocation
                        } else {
                            buffer_events::Action::Continue
                        }
                    })
                },
            )?;
            assert!(
                observed.get() == 1 && denied.is_err(),
                "native_sse_verifier_fault_not_exercised"
            );
            assert!(
                fixture.counts()? == (0, 0, 0) && fixture.budget.used() == baseline,
                "native_sse_verifier_failure_leaked"
            );
        }
    }
    Ok(())
}

#[test]
fn c5_native_sse_finalization_late_metadata_failure_retires_prepared_slot() -> TestResult {
    use crate::proxy::security::payload_metadata::test_support as metadata_events;
    use std::{cell::Cell, rc::Rc};
    for mapped in [true, false] {
        let fixture = Fixture::new(mapped, TEXT, false)?;
        let baseline = fixture.budget.used();
        let observed = Rc::new(Cell::new(0));
        let seen = Rc::clone(&observed);
        let store = fixture.store.clone();
        let result = fixture.finalize_with(
            false,
            &fixture.binding,
            &fixture.attempt,
            usize::MAX,
            || {
                metadata_events::install(move |event| {
                    let (published, pending, pins, ..) = publication_test_support::counts(&store)
                        .unwrap_or_else(|_| panic!("synthetic_count_unavailable"));
                    if event.phase == metadata_events::Phase::BeforeAllocation && pending == 1 {
                        seen.set(seen.get() + 1);
                        assert!(
                            (published, pending, pins) == (0, 1, 1) && event.used >= event.bytes,
                            "native_sse_late_metadata_missing_precharge_or_slot"
                        );
                        metadata_events::Action::Fail
                    } else {
                        metadata_events::Action::Continue
                    }
                })
            },
        )?;
        assert!(
            result.is_err() && observed.get() == 1,
            "native_sse_late_metadata_not_denied"
        );
        assert!(
            fixture.counts()? == (0, 0, 0) && fixture.budget.used() == baseline,
            "native_sse_late_metadata_stranded_slot"
        );
    }
    Ok(())
}

#[test]
fn c5_native_sse_finalization_activation_failure_after_done_is_not_success() -> TestResult {
    for mapped in [true, false] {
        let fixture = Fixture::new(mapped, TEXT, false)?;
        let baseline = fixture.budget.used();
        let finalized =
            require(fixture.finalize(false, &fixture.binding, &fixture.attempt, usize::MAX)?)?;
        let mut stream = finalized.into_delivery_stream();
        for _ in 0..3 {
            let _ = futures::executor::block_on(stream.next())
                .ok_or("synthetic_activation_frame_missing")??;
        }
        assert!(
            fixture.counts()? == (0, 1, 1),
            "native_sse_done_published_without_resume"
        );
        publication_test_support::unavailable(&fixture.store)?;
        let error = futures::executor::block_on(stream.next())
            .ok_or("synthetic_activation_error_missing")?;
        assert!(
            error.is_err_and(|error| error.to_string() == "Response stream failed."),
            "native_sse_activation_failure_not_fixed_error"
        );
        assert!(
            futures::executor::block_on(stream.next()).is_none(),
            "native_sse_activation_extra_terminal"
        );
        drop(stream);
        assert!(
            fixture.counts()? == (0, 0, 0) && fixture.budget.used() == baseline,
            "native_sse_activation_failure_published_or_leaked"
        );
    }
    Ok(())
}

#[test]
fn c5_native_sse_finalization_yielded_bytes_retain_charge_until_last_owner_drop() -> TestResult {
    for mapped in [true, false] {
        let fixture = Fixture::new(mapped, TEXT, false)?;
        let baseline = fixture.budget.used();
        let finalized =
            require(fixture.finalize(false, &fixture.binding, &fixture.attempt, usize::MAX)?)?;
        let mut stream = finalized.into_delivery_stream();
        let frame = futures::executor::block_on(stream.next())
            .ok_or("synthetic_retained_frame_missing")??;
        let duplicate = frame.clone();
        drop(stream);
        assert!(
            fixture.counts()? == (0, 0, 0) && fixture.budget.used() >= baseline + frame.len(),
            "native_sse_yielded_allocation_charge_lost"
        );
        drop(frame);
        assert!(
            fixture.budget.used() >= baseline + duplicate.len(),
            "native_sse_cloned_frame_charge_lost"
        );
        drop(duplicate);
        assert!(
            fixture.budget.used() == baseline,
            "native_sse_retained_frame_leaked"
        );
    }
    Ok(())
}

#[test]
fn c5_native_sse_finalization_tool_wire_preserves_identity_types_and_unknown_fields() -> TestResult
{
    let fixture = Fixture::new(true, "synthetic-safe🔒", true)?;
    let baseline = fixture.budget.used();
    let finalized =
        require(fixture.finalize(true, &fixture.binding, &fixture.attempt, usize::MAX)?)?;
    let mut stream = finalized.into_delivery_stream();
    let first =
        futures::executor::block_on(stream.next()).ok_or("synthetic_tool_frame_missing")??;
    let raw = std::str::from_utf8(&first)?;
    let data = raw
        .lines()
        .find_map(|line| line.strip_prefix("data: "))
        .ok_or("synthetic_tool_data_missing")?;
    let parsed: Value = serde_json::from_str(data)?;
    let mut expected = fixture.sources[0].clone();
    expected["choices"][0]["delta"]["tool_calls"][0]["function"]["arguments"] =
        json!("{\"value\":\"synthetic-safe🔒\"}");
    assert!(
        parsed == expected
            && parsed["synthetic_extension"]["types"][2].is_u64()
            && parsed["synthetic_extension"]["types"][3].is_f64(),
        "native_sse_tool_identity_or_unknown_types_changed"
    );
    assert!(
        fixture.counts()? == (0, 1, 1),
        "native_sse_tool_yield_published"
    );
    drop(first);
    for _ in 0..2 {
        let _ = futures::executor::block_on(stream.next())
            .ok_or("synthetic_tool_terminal_missing")??;
        assert!(
            fixture.counts()? == (0, 1, 1),
            "native_sse_tool_terminal_published"
        );
    }
    assert!(
        futures::executor::block_on(stream.next()).is_none() && fixture.counts()? == (1, 0, 0),
        "native_sse_tool_resume_not_published"
    );
    drop(stream);
    assert!(
        fixture.budget.used() == baseline,
        "native_sse_tool_delivery_leaked"
    );
    Ok(())
}

#[test]
fn c5_native_sse_finalization_proof_digest_rejects_other_valid_wire_before_revalidation(
) -> TestResult {
    use crate::proxy::security::payload_buffer::test_support as buffer_events;
    use std::{cell::Cell, rc::Rc};
    let fixture = Fixture::new(false, TEXT, false)?;
    let other = Fixture::new(true, TEXT, false)?;
    let proof = require(futures::executor::block_on(
        test_transport::native_sse_proof(
            &fixture.attempt,
            &fixture.binding,
            &fixture.budget,
            Bytes::copy_from_slice(other.entity.as_bytes()),
            Bytes::copy_from_slice(other.entity.as_bytes()),
        ),
    ))?;
    let baseline = fixture.budget.used();
    let frames = fixture.frames(false)?;
    let observed = require(NativeSseObservation::new(
        fixture.entity.as_bytes(),
        &[&frames[0], &frames[1], &frames[2]],
        chat(),
        &fixture.lease,
        &fixture.binding,
        fixture.budget.clone(),
    ))?;
    let stage = require(stage_native_sse(
        &observed,
        &[NativeSseSnapshotSelection::new(
            0,
            &fixture.sources[0]["choices"][0]["delta"],
        )],
        &fixture.lease,
        &fixture.binding,
        fixture.budget.clone(),
    ))?;
    let allocations = Rc::new(Cell::new(0));
    let seen = Rc::clone(&allocations);
    let hook = buffer_events::install(move |event| {
        if event.phase == buffer_events::Phase::BeforeAllocation {
            seen.set(seen.get() + 1);
        }
        buffer_events::Action::Continue
    });
    let result = finalize_native_sse(stage, &fixture.lease, proof, &fixture.attempt, usize::MAX);
    drop(hook);
    assert!(
        result.is_err() && allocations.get() == 0 && fixture.counts()? == (0, 0, 0),
        "native_sse_wrong_source_proof_reached_revalidation_or_release"
    );
    drop(observed);
    drop(frames);
    assert!(
        fixture.budget.used() == baseline,
        "native_sse_wrong_source_proof_leaked"
    );
    Ok(())
}
