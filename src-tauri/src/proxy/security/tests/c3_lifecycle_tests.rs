//! Behavioral RED contracts for shared mechanics, not a protocol parser or completion issuer.
//! The parent registers this module and verifies RED before authorizing implementation.
use super::{
    completed_context::ActualAttemptBinding,
    payload_budget::PayloadBudget,
    payload_buffer::PayloadBuffer,
    payload_text::PayloadText,
    response_restoration::RestoreError,
    restoration_contract::{
        ClientProtocol, ContinuityScope, IngressNamespace, OriginalResponseObservation,
        OriginalResponseSource, ProtocolPair, RestoreDecision, TextPolicy, UpstreamProtocol,
    },
    restoration_core::{AttemptProvenance, LifecycleLimits, ResponseLifecycle},
    tokenization::{DlpRequestLease, DlpTokenStore, TokenCategory, TokenError, TokenFragment},
};
use crate::proxy::protocol_bridge::Protocol;
use serde_json::json;

type TestResult<T = ()> = Result<T, &'static str>;
const BUDGET_BYTES: usize = 2 * 1024 * 1024;
const PLAIN: &str = "Synthetic lifecycle private text";
const ORDINARY: &str = "Synthetic ordinary text";

fn accepted<T>(result: Result<T, RestoreError>, marker: &'static str) -> TestResult<T> {
    result.map_err(|_| marker)
}

fn pair() -> ProtocolPair {
    ProtocolPair::new(
        ClientProtocol::new(Protocol::Chat),
        UpstreamProtocol::new(Protocol::Chat),
    )
}

fn limits() -> LifecycleLimits {
    LifecycleLimits {
        max_text_bytes: 64 * 1024,
        max_text_streams: 16,
        max_held_bytes: 64 * 1024,
        max_held_units: 16,
    }
}

struct Fixture {
    _store: DlpTokenStore,
    lease: Option<DlpRequestLease>,
    binding: Option<ActualAttemptBinding>,
    budget: PayloadBudget,
    attempt: AttemptProvenance,
    reference: Option<PayloadText>,
}

impl Fixture {
    fn new(mapped: bool) -> TestResult<Self> {
        let budget = PayloadBudget::with_limit(BUDGET_BYTES);
        let store = DlpTokenStore::new();
        let mut lease = store
            .admit_scoped_published_policy(
                ContinuityScope::Native {
                    client: ClientProtocol::new(Protocol::Chat),
                    ingress: IngressNamespace::Direct,
                },
                "synthetic-c3-lifecycle-app",
                Some("synthetic-c3-lifecycle-session"),
                1,
            )
            .map_err(|_| "c3_lifecycle_admission_fixture")?;
        // Even test reference metadata is budget-owned; no uncharged token String/Vec.
        let reference = if mapped {
            Some(
                lease
                    .prepare_with_budget(
                        &[TokenFragment {
                            raw: PLAIN,
                            whole_span_categories: &[TokenCategory::Phone],
                        }],
                        &[],
                        budget.clone(),
                        |tokens| {
                            let token = tokens.first().ok_or(TokenError::Unavailable)?;
                            PayloadText::try_from_str(token.as_str(), budget.clone(), 128)
                        },
                    )
                    .map_err(|_| "c3_lifecycle_mapping_fixture")?,
            )
        } else {
            None
        };
        let binding = lease
            .bind_actual_attempt(
                "synthetic-c3-lifecycle-provider",
                "openai_chat",
                "https://synthetic.invalid/v1/chat/completions",
                Some("synthetic-c3-lifecycle-principal"),
            )
            .map_err(|_| "c3_lifecycle_binding_fixture")?;
        let attempt =
            AttemptProvenance::new(&budget).map_err(|_| "c3_lifecycle_provenance_fixture")?;
        Ok(Self {
            _store: store,
            lease: Some(lease),
            binding: Some(binding),
            budget,
            attempt,
            reference,
        })
    }

    fn begin(&mut self, limits: LifecycleLimits) -> TestResult<ResponseLifecycle> {
        accepted(
            ResponseLifecycle::begin(
                self.lease.take().ok_or("c3_lifecycle_lease_missing")?,
                self.binding.take().ok_or("c3_lifecycle_binding_missing")?,
                self.budget.clone(),
                pair(),
                self.attempt.clone(),
                limits,
            ),
            "c3_lifecycle_begin_positive",
        )
    }

    fn reference(&self) -> TestResult<&str> {
        self.reference
            .as_ref()
            .map(PayloadText::as_str)
            .ok_or("c3_lifecycle_reference_missing")
    }

    fn unit(&self, bytes: &[u8]) -> TestResult<PayloadBuffer> {
        PayloadBuffer::try_from_slice(bytes, self.budget.clone(), BUDGET_BYTES)
            .map_err(|_| "c3_lifecycle_native_unit_fixture")
    }
}

fn restored(decision: &RestoreDecision, original: &str) -> TestResult<bool> {
    match decision {
        RestoreDecision::Unchanged => Ok(original == PLAIN),
        RestoreDecision::Patch(text) => Ok(text.as_str() == PLAIN),
        RestoreDecision::Hold => Err("c3_lifecycle_snapshot_unexpected_hold"),
    }
}

fn append_decision(
    out: &mut PayloadBuffer,
    decision: RestoreDecision,
    original: &str,
) -> TestResult {
    match decision {
        RestoreDecision::Unchanged => out.append(original.as_bytes()),
        RestoreDecision::Patch(text) => out.append(text.as_str().as_bytes()),
        RestoreDecision::Hold => Ok(()),
    }
    .map_err(|_| "c3_lifecycle_output_collection_fixture")
}

#[test]
fn c3_lifecycle_begin_finish_consumes_inspection_owner_without_completion() -> TestResult {
    let mut fixture = Fixture::new(false)?;
    let baseline = fixture.budget.used();
    let lifecycle = fixture.begin(limits())?;
    assert!(
        fixture.budget.used() > baseline,
        "c3_lifecycle_owner_metadata_uncharged"
    );
    assert!(
        lifecycle.dependency_tokens().is_empty() && lifecycle.restored_occurrences() == 0,
        "c3_lifecycle_empty_ledger"
    );
    let finished = accepted(lifecycle.finish(), "c3_lifecycle_finish_positive")?;
    assert!(
        finished.attempt().matches(&fixture.attempt)
            && finished.pair().client().protocol() == Protocol::Chat
            && finished.held_units().is_empty(),
        "c3_lifecycle_finish_inspection_identity"
    );
    // FinishedInspection has no proof, spec, publication, resume or completion operation.
    assert!(
        fixture.budget.used() > baseline,
        "c3_lifecycle_finish_released_live_inspection"
    );
    drop(finished);
    assert!(
        fixture.budget.used() == baseline,
        "c3_lifecycle_finish_cleanup"
    );
    Ok(())
}

#[test]
fn c3_lifecycle_original_json_is_borrowed_with_charged_claim_metadata() -> TestResult {
    let fixture = Fixture::new(false)?;
    let native = json!({"content": ORDINARY, "unknown": [7, null, true]});
    let baseline = fixture.budget.used();
    let mut observation = accepted(
        OriginalResponseObservation::json(
            &native,
            pair().upstream(),
            fixture.attempt.clone(),
            fixture.budget.clone(),
        ),
        "c3_lifecycle_original_json_positive",
    )?;
    accepted(
        observation.claim_node(&native["content"]),
        "c3_lifecycle_original_claim_positive",
    )?;
    assert!(
        matches!(observation.source(), OriginalResponseSource::Json(value) if std::ptr::eq(value, &native)),
        "c3_lifecycle_original_json_was_copied"
    );
    assert!(
        observation.claims().len() == 1 && fixture.budget.used() > baseline,
        "c3_lifecycle_original_metadata_uncharged"
    );
    assert!(
        native["content"].as_str() == Some(ORDINARY) && native["unknown"][0] == 7,
        "c3_lifecycle_original_json_mutated"
    );
    drop(observation);
    assert!(
        fixture.budget.used() == baseline,
        "c3_lifecycle_original_cleanup"
    );
    Ok(())
}

#[test]
fn c3_lifecycle_original_frame_keeps_every_byte_split_including_partial_utf8() -> TestResult {
    let fixture = Fixture::new(false)?;
    let raw = "data: 前🔒tail\n\n".as_bytes();
    for split in 0..=raw.len() {
        for bytes in [&raw[..split], &raw[split..]] {
            let baseline = fixture.budget.used();
            let observation = accepted(
                OriginalResponseObservation::frame(
                    bytes,
                    pair().upstream(),
                    fixture.attempt.clone(),
                    fixture.budget.clone(),
                ),
                "c3_lifecycle_original_frame_positive",
            )?;
            assert!(
                matches!(observation.source(), OriginalResponseSource::Frame(value)
                    if value.as_ptr() == bytes.as_ptr() && value.len() == bytes.len()),
                "c3_lifecycle_original_frame_not_exact_borrow"
            );
            assert!(
                fixture.budget.used() > baseline,
                "c3_lifecycle_frame_metadata_uncharged"
            );
            drop(observation);
            assert!(
                fixture.budget.used() == baseline,
                "c3_lifecycle_frame_cleanup"
            );
        }
    }
    Ok(())
}

#[test]
fn c3_lifecycle_same_attempt_zero_map_original_is_inspection_not_eof_proof() -> TestResult {
    let mut fixture = Fixture::new(false)?;
    let mut lifecycle = fixture.begin(limits())?;
    // Deliberately incomplete native bytes. Accepting the borrow cannot attest EOF.
    let raw = b"data: {\"unfinished\":";
    let observation = accepted(
        OriginalResponseObservation::frame(
            raw,
            pair().upstream(),
            fixture.attempt.clone(),
            fixture.budget.clone(),
        ),
        "c3_lifecycle_same_attempt_observation_positive",
    )?;
    accepted(
        lifecycle.observe_original(&observation),
        "c3_lifecycle_same_attempt_zero_map_positive",
    )?;
    let finished = accepted(
        lifecycle.finish(),
        "c3_lifecycle_inspection_finish_positive",
    )?;
    assert!(
        finished.dependency_tokens().is_empty() && finished.restored_occurrences() == 0,
        "c3_lifecycle_observation_granted_mapping_authority"
    );
    drop(finished);
    Ok(())
}

#[test]
fn c3_lifecycle_foreign_attempt_zero_map_rejected_and_failure_sticks() -> TestResult {
    let mut fixture = Fixture::new(false)?;
    let mut lifecycle = fixture.begin(limits())?;
    let other = AttemptProvenance::new(&fixture.budget)
        .map_err(|_| "c3_lifecycle_foreign_provenance_fixture")?;
    let native = json!({"content": ORDINARY});
    let observation = accepted(
        OriginalResponseObservation::json(
            &native,
            pair().upstream(),
            other,
            fixture.budget.clone(),
        ),
        "c3_lifecycle_foreign_observation_fixture",
    )?;
    assert!(
        matches!(
            lifecycle.observe_original(&observation),
            Err(RestoreError::RestoreFailed)
        ),
        "c3_lifecycle_zero_map_foreign_attempt_accepted"
    );
    assert!(
        lifecycle.failure() == Some(RestoreError::RestoreFailed)
            && lifecycle.open_text(TextPolicy::Output).is_err(),
        "c3_lifecycle_foreign_attempt_failure_not_sticky"
    );
    assert!(
        lifecycle.finish().is_err(),
        "c3_lifecycle_foreign_attempt_finished"
    );
    Ok(())
}

#[test]
fn c3_lifecycle_wrong_original_upstream_rejected_for_same_attempt_zero_map() -> TestResult {
    let mut fixture = Fixture::new(false)?;
    let mut lifecycle = fixture.begin(limits())?;
    let native = json!({"content": ORDINARY});
    let observation = accepted(
        OriginalResponseObservation::json(
            &native,
            UpstreamProtocol::new(Protocol::Anthropic),
            fixture.attempt.clone(),
            fixture.budget.clone(),
        ),
        "c3_lifecycle_wrong_upstream_observation_fixture",
    )?;
    assert!(
        lifecycle.observe_original(&observation).is_err() && lifecycle.finish().is_err(),
        "c3_lifecycle_wrong_upstream_accepted"
    );
    Ok(())
}

#[test]
fn c3_lifecycle_done_restores_snapshot_and_records_exact_dependency_once() -> TestResult {
    let mut fixture = Fixture::new(true)?;
    let mut lifecycle = fixture.begin(limits())?;
    let id = accepted(
        lifecycle.open_text(TextPolicy::Output),
        "c3_lifecycle_open_positive",
    )?;
    let decision = accepted(
        lifecycle.done(&id, fixture.reference()?),
        "c3_lifecycle_done_positive",
    )?;
    assert!(
        restored(&decision, fixture.reference()?)?,
        "c3_lifecycle_done_not_restored"
    );
    assert!(
        lifecycle.dependency_tokens().len() == 1
            && lifecycle.dependency_tokens()[0].as_str() == fixture.reference()?
            && lifecycle.restored_occurrences() == 1,
        "c3_lifecycle_done_dependency_ledger"
    );
    let finished = accepted(lifecycle.finish(), "c3_lifecycle_snapshot_finish_positive")?;
    let text = accepted(
        finished.text_state(&id),
        "c3_lifecycle_finished_text_inspection",
    )?;
    assert!(
        text.raw_done.as_ref().map(PayloadText::as_str) == Some(fixture.reference()?)
            && text.restored_done.as_ref().map(PayloadText::as_str) == Some(PLAIN),
        "c3_lifecycle_done_snapshot_not_retained"
    );
    Ok(())
}

#[test]
fn c3_lifecycle_text_policy_preserves_reasoning_and_restores_other_supported_text() -> TestResult {
    for policy in [
        TextPolicy::Output,
        TextPolicy::Refusal,
        TextPolicy::ToolArguments,
        TextPolicy::Reasoning,
    ] {
        let mut fixture = Fixture::new(true)?;
        let mut lifecycle = fixture.begin(limits())?;
        let id = accepted(
            lifecycle.open_text(policy),
            "c3_lifecycle_policy_open_positive",
        )?;
        let decision = accepted(
            lifecycle.done(&id, fixture.reference()?),
            "c3_lifecycle_policy_done_positive",
        )?;
        if policy == TextPolicy::Reasoning {
            assert!(
                matches!(&decision, RestoreDecision::Unchanged)
                    && lifecycle.dependency_tokens().is_empty()
                    && lifecycle.restored_occurrences() == 0,
                "c3_lifecycle_reasoning_policy_erased"
            );
        } else {
            assert!(
                restored(&decision, fixture.reference()?)?,
                "c3_lifecycle_restore_policy_erased"
            );
        }
        drop(accepted(
            lifecycle.finish(),
            "c3_lifecycle_policy_finish_positive",
        )?);
    }
    Ok(())
}

#[test]
fn c3_lifecycle_reference_and_utf8_deltas_reconcile_at_every_decoded_boundary() -> TestResult {
    let mut fixture = Fixture::new(true)?;
    let mut native = PayloadText::with_capacity(
        "前🔒".len() + fixture.reference()?.len() + "尾".len(),
        fixture.budget.clone(),
        BUDGET_BYTES,
    )
    .map_err(|_| "c3_lifecycle_split_native_fixture")?;
    native
        .append("前🔒")
        .and_then(|_| native.append(fixture.reference().map_err(|_| TokenError::Unavailable)?))
        .and_then(|_| native.append("尾"))
        .map_err(|_| "c3_lifecycle_split_native_fixture")?;
    let mut expected = PayloadText::with_capacity(
        "前🔒".len() + PLAIN.len() + "尾".len(),
        fixture.budget.clone(),
        BUDGET_BYTES,
    )
    .map_err(|_| "c3_lifecycle_split_expected_fixture")?;
    expected
        .append("前🔒")
        .and_then(|_| expected.append(PLAIN))
        .and_then(|_| expected.append("尾"))
        .map_err(|_| "c3_lifecycle_split_expected_fixture")?;
    let mut configured = limits();
    configured.max_text_streams = native.as_str().len() + 1;
    let mut lifecycle = fixture.begin(configured)?;
    let mut occurrences = 0;
    for split in 0..=native.as_str().len() {
        if !native.as_str().is_char_boundary(split) {
            continue;
        }
        let id = accepted(
            lifecycle.open_text(TextPolicy::Output),
            "c3_lifecycle_delta_open_positive",
        )?;
        let mut emitted = PayloadBuffer::new(fixture.budget.clone(), BUDGET_BYTES)
            .map_err(|_| "c3_lifecycle_delta_output_fixture")?;
        for chunk in [&native.as_str()[..split], &native.as_str()[split..]] {
            let decision = accepted(lifecycle.delta(&id, chunk), "c3_lifecycle_delta_positive")?;
            append_decision(&mut emitted, decision, chunk)?;
        }
        let snapshot = accepted(
            lifecycle.done(&id, native.as_str()),
            "c3_lifecycle_delta_done_positive",
        )?;
        assert!(
            emitted.as_slice() == expected.as_str().as_bytes()
                && matches!(snapshot, RestoreDecision::Patch(ref text) if text.as_str() == expected.as_str()),
            "c3_lifecycle_split_restoration_mismatch"
        );
        occurrences += 1;
        assert!(
            lifecycle.dependency_tokens().len() == 1
                && lifecycle.restored_occurrences() == occurrences,
            "c3_lifecycle_delta_done_double_counted"
        );
    }
    drop(accepted(
        lifecycle.finish(),
        "c3_lifecycle_split_finish_positive",
    )?);
    Ok(())
}

#[test]
fn c3_lifecycle_cross_owner_text_id_cannot_be_reused() -> TestResult {
    let mut first = Fixture::new(false)?;
    let mut second = Fixture::new(false)?;
    let mut donor = first.begin(limits())?;
    let mut recipient = second.begin(limits())?;
    let foreign = accepted(
        donor.open_text(TextPolicy::Output),
        "c3_lifecycle_donor_open_positive",
    )?;
    let own = accepted(
        recipient.open_text(TextPolicy::Output),
        "c3_lifecycle_recipient_open_positive",
    )?;
    assert!(
        recipient.delta(&foreign, ORDINARY).is_err()
            && recipient.delta(&own, ORDINARY).is_err()
            && recipient.finish().is_err(),
        "c3_lifecycle_foreign_text_id_accepted"
    );
    accepted(donor.abort(), "c3_lifecycle_donor_abort_positive")
}

#[test]
fn c3_lifecycle_mismatched_done_poisoning_blocks_delta_hold_and_finish() -> TestResult {
    let mut fixture = Fixture::new(false)?;
    let mut lifecycle = fixture.begin(limits())?;
    let id = accepted(
        lifecycle.open_text(TextPolicy::Output),
        "c3_lifecycle_mismatch_open_positive",
    )?;
    drop(accepted(
        lifecycle.delta(&id, ORDINARY),
        "c3_lifecycle_mismatch_delta_positive",
    )?);
    assert!(
        lifecycle.done(&id, "Synthetic different snapshot").is_err(),
        "c3_lifecycle_done_mismatch_accepted"
    );
    assert!(
        lifecycle.failure() == Some(RestoreError::RestoreFailed)
            && lifecycle.delta(&id, ORDINARY).is_err()
            && lifecycle.hold(fixture.unit(b"synthetic unit")?).is_err()
            && lifecycle.finish().is_err(),
        "c3_lifecycle_mismatch_failure_not_sticky"
    );
    Ok(())
}

#[test]
fn c3_lifecycle_duplicate_done_and_unfinished_finish_fail_closed() -> TestResult {
    for duplicate in [false, true] {
        let mut fixture = Fixture::new(false)?;
        let mut lifecycle = fixture.begin(limits())?;
        let id = accepted(
            lifecycle.open_text(TextPolicy::Output),
            "c3_lifecycle_terminal_open_positive",
        )?;
        if duplicate {
            drop(accepted(
                lifecycle.done(&id, ORDINARY),
                "c3_lifecycle_first_done_positive",
            )?);
            assert!(
                lifecycle.done(&id, ORDINARY).is_err(),
                "c3_lifecycle_duplicate_done_accepted"
            );
        }
        assert!(
            lifecycle.finish().is_err(),
            "c3_lifecycle_invalid_text_finished"
        );
    }
    Ok(())
}

#[test]
fn c3_lifecycle_hold_counts_native_owner_bytes_and_keeps_units_until_inspection_drop() -> TestResult
{
    let mut fixture = Fixture::new(false)?;
    let baseline = fixture.budget.used();
    let mut lifecycle = fixture.begin(limits())?;
    accepted(
        lifecycle.hold(fixture.unit(b"unit-a")?),
        "c3_lifecycle_hold_first_positive",
    )?;
    accepted(
        lifecycle.hold(fixture.unit(b"unit-b")?),
        "c3_lifecycle_hold_second_positive",
    )?;
    assert!(
        lifecycle.held_units().len() == 2 && lifecycle.held_bytes() == 12,
        "c3_lifecycle_hold_accounting"
    );
    let finished = accepted(lifecycle.finish(), "c3_lifecycle_held_inspection_positive")?;
    assert!(
        finished.held_units()[0].as_slice() == b"unit-a"
            && finished.held_units()[1].as_slice() == b"unit-b"
            && fixture.budget.used() > baseline,
        "c3_lifecycle_hold_released_or_reordered"
    );
    drop(finished);
    assert!(
        fixture.budget.used() == baseline,
        "c3_lifecycle_held_cleanup"
    );
    Ok(())
}

#[test]
fn c3_lifecycle_hold_limits_cannot_be_bypassed_by_deferred_or_empty_units() -> TestResult {
    for byte_limit in [false, true] {
        let mut fixture = Fixture::new(false)?;
        let mut configured = limits();
        configured.max_held_bytes = if byte_limit { 1 } else { 64 };
        configured.max_held_units = if byte_limit { 2 } else { 1 };
        let mut lifecycle = fixture.begin(configured)?;
        accepted(
            lifecycle.hold(fixture.unit(b"a")?),
            "c3_lifecycle_hold_limit_positive",
        )?;
        // No caller bytes/count parameter exists, including for deferred/empty units.
        let next = if byte_limit {
            b"b".as_slice()
        } else {
            b"".as_slice()
        };
        let baseline = fixture.budget.used();
        assert!(
            lifecycle.hold(fixture.unit(next)?).is_err(),
            "c3_lifecycle_hold_limit_bypassed"
        );
        assert!(
            lifecycle.held_units().len() == 1
                && lifecycle.held_bytes() == 1
                && fixture.budget.used() == baseline
                && lifecycle.finish().is_err(),
            "c3_lifecycle_hold_limit_partial_commit"
        );
    }
    Ok(())
}

#[test]
fn c3_lifecycle_atomic_budget_failure_preserves_dependency_ledger_and_sticks() -> TestResult {
    let mut fixture = Fixture::new(true)?;
    let mut lifecycle = fixture.begin(limits())?;
    let id = accepted(
        lifecycle.open_text(TextPolicy::Output),
        "c3_lifecycle_atomic_open_positive",
    )?;
    let baseline = fixture.budget.used();
    let pressure = fixture
        .budget
        .reserve(fixture.budget.limit() - baseline)
        .map_err(|_| "c3_lifecycle_budget_pressure_fixture")?;
    assert!(
        lifecycle.delta(&id, fixture.reference()?).is_err(),
        "c3_lifecycle_budget_failure_missing"
    );
    assert!(
        lifecycle.dependency_tokens().is_empty()
            && lifecycle.restored_occurrences() == 0
            && fixture.budget.used() == fixture.budget.limit(),
        "c3_lifecycle_budget_failure_partial_ledger"
    );
    drop(pressure);
    assert!(
        fixture.budget.used() == baseline && lifecycle.delta(&id, ORDINARY).is_err(),
        "c3_lifecycle_budget_failure_not_sticky"
    );
    accepted(lifecycle.abort(), "c3_lifecycle_failed_abort_positive")
}

#[test]
fn c3_lifecycle_hold_metadata_budget_failure_is_atomic() -> TestResult {
    let mut fixture = Fixture::new(false)?;
    let mut lifecycle = fixture.begin(limits())?;
    let unit = fixture.unit(b"synthetic held unit")?;
    let unit_bytes = unit.len();
    let baseline = fixture.budget.used();
    let pressure = fixture
        .budget
        .reserve(fixture.budget.limit() - baseline)
        .map_err(|_| "c3_lifecycle_hold_pressure_fixture")?;
    assert!(
        lifecycle.hold(unit).is_err(),
        "c3_lifecycle_hold_metadata_failure_missing"
    );
    assert!(
        lifecycle.held_units().is_empty()
            && lifecycle.held_bytes() == 0
            && fixture.budget.used() == fixture.budget.limit() - unit_bytes,
        "c3_lifecycle_hold_metadata_partial_commit"
    );
    drop(pressure);
    accepted(
        lifecycle.abort(),
        "c3_lifecycle_hold_failure_abort_positive",
    )
}

#[test]
fn c3_lifecycle_abort_consumes_active_owner_and_refunds_budget() -> TestResult {
    let mut fixture = Fixture::new(true)?;
    let baseline = fixture.budget.used();
    let mut lifecycle = fixture.begin(limits())?;
    let id = accepted(
        lifecycle.open_text(TextPolicy::Output),
        "c3_lifecycle_abort_open_positive",
    )?;
    drop(accepted(
        lifecycle.delta(&id, fixture.reference()?),
        "c3_lifecycle_abort_delta_positive",
    )?);
    accepted(
        lifecycle.hold(fixture.unit(b"synthetic withheld unit")?),
        "c3_lifecycle_abort_hold_positive",
    )?;
    // Opaque stream IDs retain budget-owned identity metadata until they too are dropped.
    drop(id);
    accepted(lifecycle.abort(), "c3_lifecycle_abort_positive")?;
    assert!(
        fixture.budget.used() == baseline,
        "c3_lifecycle_abort_retained_payload"
    );
    Ok(())
}

#[test]
fn c3_lifecycle_budget_failure_preserves_previously_committed_dependency_ledger() -> TestResult {
    let mut fixture = Fixture::new(true)?;
    let mut lifecycle = fixture.begin(limits())?;
    let first = accepted(
        lifecycle.open_text(TextPolicy::Output),
        "c3_lifecycle_prior_ledger_open_positive",
    )?;
    drop(accepted(
        lifecycle.done(&first, fixture.reference()?),
        "c3_lifecycle_prior_ledger_done_positive",
    )?);
    let second = accepted(
        lifecycle.open_text(TextPolicy::Output),
        "c3_lifecycle_next_ledger_open_positive",
    )?;
    let baseline = fixture.budget.used();
    let pressure = fixture
        .budget
        .reserve(fixture.budget.limit() - baseline)
        .map_err(|_| "c3_lifecycle_prior_ledger_pressure_fixture")?;
    assert!(
        lifecycle.done(&second, fixture.reference()?).is_err(),
        "c3_lifecycle_prior_ledger_failure_missing"
    );
    assert!(
        lifecycle.dependency_tokens().len() == 1
            && lifecycle.dependency_tokens()[0].as_str() == fixture.reference()?
            && lifecycle.restored_occurrences() == 1
            && fixture.budget.used() == fixture.budget.limit(),
        "c3_lifecycle_prior_ledger_partial_commit"
    );
    drop(pressure);
    assert!(
        fixture.budget.used() == baseline,
        "c3_lifecycle_prior_ledger_leak"
    );
    accepted(
        lifecycle.abort(),
        "c3_lifecycle_prior_ledger_abort_positive",
    )
}

#[test]
fn c3_lifecycle_original_claim_rejects_equal_foreign_node_without_metadata_growth() -> TestResult {
    let fixture = Fixture::new(false)?;
    let native = json!({"content": ORDINARY});
    let foreign = json!(ORDINARY);
    let mut observation = accepted(
        OriginalResponseObservation::json(
            &native,
            pair().upstream(),
            fixture.attempt.clone(),
            fixture.budget.clone(),
        ),
        "c3_lifecycle_foreign_node_observation_positive",
    )?;
    accepted(
        observation.claim_node(&native["content"]),
        "c3_lifecycle_owned_node_claim_positive",
    )?;
    let baseline = fixture.budget.used();
    assert!(
        observation.claim_node(&foreign).is_err()
            && observation.claims().len() == 1
            && std::ptr::eq(observation.claims()[0], &native["content"])
            && fixture.budget.used() == baseline,
        "c3_lifecycle_equal_foreign_node_accepted"
    );
    Ok(())
}

#[test]
fn c3_lifecycle_open_stream_limit_is_atomic_and_sticky() -> TestResult {
    for max_text_streams in [0, 1] {
        let mut fixture = Fixture::new(false)?;
        let baseline = fixture.budget.used();
        let mut configured = limits();
        configured.max_text_streams = max_text_streams;
        let mut lifecycle = fixture.begin(configured)?;
        let id = if max_text_streams == 1 {
            Some(accepted(
                lifecycle.open_text(TextPolicy::Output),
                "c3_lifecycle_stream_limit_open_positive",
            )?)
        } else {
            None
        };
        let before_failure = fixture.budget.used();
        assert!(
            lifecycle.open_text(TextPolicy::Refusal).is_err()
                && lifecycle.failure().is_some()
                && fixture.budget.used() == before_failure,
            "c3_lifecycle_stream_limit_partial_commit"
        );
        assert!(
            lifecycle.open_text(TextPolicy::Reasoning).is_err()
                && lifecycle
                    .hold(fixture.unit(b"synthetic blocked unit")?)
                    .is_err()
                && lifecycle.held_units().is_empty()
                && lifecycle.held_bytes() == 0
                && lifecycle.dependency_tokens().is_empty()
                && lifecycle.restored_occurrences() == 0
                && fixture.budget.used() == before_failure,
            "c3_lifecycle_stream_limit_failure_not_sticky"
        );
        drop(id);
        accepted(
            lifecycle.abort(),
            "c3_lifecycle_stream_limit_abort_positive",
        )?;
        assert!(
            fixture.budget.used() == baseline,
            "c3_lifecycle_stream_limit_abort_cleanup"
        );
    }
    Ok(())
}

#[test]
fn c3_lifecycle_abort_without_delta_retains_only_live_text_id_identity() -> TestResult {
    let mut fixture = Fixture::new(false)?;
    let baseline = fixture.budget.used();
    let mut lifecycle = fixture.begin(limits())?;
    let id = accepted(
        lifecycle.open_text(TextPolicy::Output),
        "c3_lifecycle_abort_without_delta_open_positive",
    )?;
    accepted(
        lifecycle.hold(fixture.unit(b"synthetic withheld unit")?),
        "c3_lifecycle_abort_without_delta_hold_positive",
    )?;
    let before_abort = fixture.budget.used();
    // Exercise active cleanup without passing through an unimplemented delta operation.
    accepted(
        lifecycle.abort(),
        "c3_lifecycle_abort_without_delta_positive",
    )?;
    assert!(
        fixture.budget.used() > baseline && fixture.budget.used() < before_abort,
        "c3_lifecycle_live_text_id_identity_charge_lost"
    );
    drop(id);
    assert!(
        fixture.budget.used() == baseline,
        "c3_lifecycle_abort_without_delta_cleanup"
    );
    Ok(())
}

#[test]
fn c3_lifecycle_delta_late_failure_preserves_committed_ledger_and_native_owners() -> TestResult {
    use super::payload_buffer::test_support::{install, Action, Phase};
    use std::{cell::Cell, rc::Rc};

    let mut fixture = Fixture::new(true)?;
    let mut lifecycle = fixture.begin(limits())?;
    let first = accepted(
        lifecycle.open_text(TextPolicy::Output),
        "c3_lifecycle_late_delta_first_open_positive",
    )?;
    drop(accepted(
        lifecycle.done(&first, fixture.reference()?),
        "c3_lifecycle_late_delta_prior_done_positive",
    )?);
    let second = accepted(
        lifecycle.open_text(TextPolicy::Output),
        "c3_lifecycle_late_delta_second_open_positive",
    )?;
    drop(accepted(
        lifecycle.delta(&second, ORDINARY),
        "c3_lifecycle_late_delta_prior_delta_positive",
    )?);
    accepted(
        lifecycle.hold(fixture.unit(b"synthetic late delta owner")?),
        "c3_lifecycle_late_delta_hold_positive",
    )?;
    let ledger_owner = lifecycle.dependency_tokens()[0].as_str().as_ptr();
    let native_owner = lifecycle.held_units()[0].as_slice().as_ptr();
    let held_bytes = lifecycle.held_bytes();
    let baseline = fixture.budget.used();
    let reached = Rc::new(Cell::new(false));
    let reached_hook = Rc::clone(&reached);
    let hook = install(move |observation| {
        // Fail after restoration, while appending the restored delta to staged history.
        if observation.phase == Phase::BeforeCopy
            && observation.bytes_to_copy == ORDINARY.len() + PLAIN.len()
        {
            reached_hook.set(true);
            Action::FailAllocation
        } else {
            Action::Continue
        }
    });
    let result = lifecycle.delta(&second, fixture.reference()?);
    drop(hook);
    assert!(
        reached.get() && result.is_err(),
        "c3_lifecycle_late_delta_failure_not_reached"
    );
    let error = result
        .err()
        .ok_or("c3_lifecycle_late_delta_error_missing")?;
    assert!(
        lifecycle.failure() == Some(error)
            && lifecycle.dependency_tokens().len() == 1
            && lifecycle.dependency_tokens()[0].as_str().as_ptr() == ledger_owner
            && lifecycle.dependency_tokens()[0].as_str() == fixture.reference()?
            && lifecycle.restored_occurrences() == 1
            && lifecycle.held_units().len() == 1
            && lifecycle.held_units()[0].as_slice().as_ptr() == native_owner
            && lifecycle.held_bytes() == held_bytes
            && fixture.budget.used() == baseline,
        "c3_lifecycle_late_delta_partial_commit"
    );
    assert!(
        lifecycle.delta(&second, ORDINARY).err() == Some(error)
            && lifecycle.done(&second, ORDINARY).err() == Some(error)
            && lifecycle.open_text(TextPolicy::Reasoning).err() == Some(error)
            && fixture.budget.used() == baseline,
        "c3_lifecycle_late_delta_failure_not_sticky"
    );
    accepted(lifecycle.abort(), "c3_lifecycle_late_delta_abort_positive")
}

#[test]
fn c3_lifecycle_done_late_failure_preserves_committed_ledger_and_native_owners() -> TestResult {
    use super::payload_text::test_support::{install, Action, Phase};
    use std::{cell::Cell, rc::Rc};

    let mut fixture = Fixture::new(true)?;
    let mut lifecycle = fixture.begin(limits())?;
    let first = accepted(
        lifecycle.open_text(TextPolicy::Output),
        "c3_lifecycle_late_done_first_open_positive",
    )?;
    drop(accepted(
        lifecycle.done(&first, fixture.reference()?),
        "c3_lifecycle_late_done_prior_done_positive",
    )?);
    let second = accepted(
        lifecycle.open_text(TextPolicy::Refusal),
        "c3_lifecycle_late_done_second_open_positive",
    )?;
    accepted(
        lifecycle.hold(fixture.unit(b"synthetic late done owner")?),
        "c3_lifecycle_late_done_hold_positive",
    )?;
    let ledger_owner = lifecycle.dependency_tokens()[0].as_str().as_ptr();
    let native_owner = lifecycle.held_units()[0].as_slice().as_ptr();
    let held_bytes = lifecycle.held_bytes();
    let baseline = fixture.budget.used();
    let copies = Rc::new(Cell::new(0usize));
    let copies_hook = Rc::clone(&copies);
    let hook = install(move |observation| {
        if observation.phase == Phase::BeforeCopy && observation.bytes == PLAIN.len() {
            let next = copies_hook.get() + 1;
            copies_hook.set(next);
            // The first copy is restoration; the second retains the done snapshot.
            if next == 2 {
                return Action::Fail;
            }
        }
        Action::Continue
    });
    let result = lifecycle.done(&second, fixture.reference()?);
    drop(hook);
    assert!(
        copies.get() == 2 && result.is_err(),
        "c3_lifecycle_late_done_failure_not_reached"
    );
    let error = result.err().ok_or("c3_lifecycle_late_done_error_missing")?;
    assert!(
        lifecycle.failure() == Some(error)
            && lifecycle.dependency_tokens().len() == 1
            && lifecycle.dependency_tokens()[0].as_str().as_ptr() == ledger_owner
            && lifecycle.dependency_tokens()[0].as_str() == fixture.reference()?
            && lifecycle.restored_occurrences() == 1
            && lifecycle.held_units().len() == 1
            && lifecycle.held_units()[0].as_slice().as_ptr() == native_owner
            && lifecycle.held_bytes() == held_bytes
            && fixture.budget.used() == baseline,
        "c3_lifecycle_late_done_partial_commit"
    );
    assert!(
        lifecycle.done(&second, fixture.reference()?).err() == Some(error)
            && lifecycle.delta(&first, ORDINARY).err() == Some(error)
            && lifecycle
                .hold(fixture.unit(b"synthetic blocked owner")?)
                .err()
                == Some(error)
            && fixture.budget.used() == baseline,
        "c3_lifecycle_late_done_failure_not_sticky"
    );
    assert!(
        lifecycle.finish().err() == Some(error),
        "c3_lifecycle_late_done_finished"
    );
    Ok(())
}

#[test]
fn c3_lifecycle_interleaved_tails_are_stream_local_and_reasoning_stays_literal() -> TestResult {
    let mut fixture = Fixture::new(true)?;
    let mut lifecycle = fixture.begin(limits())?;
    let output = accepted(
        lifecycle.open_text(TextPolicy::Output),
        "c3_lifecycle_interleaved_output_open_positive",
    )?;
    let refusal = accepted(
        lifecycle.open_text(TextPolicy::Refusal),
        "c3_lifecycle_interleaved_refusal_open_positive",
    )?;
    let reasoning = accepted(
        lifecycle.open_text(TextPolicy::Reasoning),
        "c3_lifecycle_interleaved_reasoning_open_positive",
    )?;
    let reference = fixture.reference()?;
    let (prefix, suffix) = reference.split_at(reference.len() / 2);
    assert!(
        matches!(
            accepted(
                lifecycle.delta(&output, prefix),
                "c3_lifecycle_interleaved_hold_positive"
            )?,
            RestoreDecision::Hold
        ),
        "c3_lifecycle_interleaved_prefix_not_held"
    );
    for chunk in [prefix, suffix] {
        assert!(
            matches!(
                accepted(
                    lifecycle.delta(&reasoning, chunk),
                    "c3_lifecycle_reasoning_delta_positive"
                )?,
                RestoreDecision::Unchanged
            ),
            "c3_lifecycle_reasoning_delta_not_literal"
        );
    }
    assert!(
        matches!(
            accepted(
                lifecycle.delta(&refusal, suffix),
                "c3_lifecycle_interleaved_other_delta_positive"
            )?,
            RestoreDecision::Unchanged
        ) && lifecycle.dependency_tokens().is_empty()
            && lifecycle.restored_occurrences() == 0,
        "c3_lifecycle_interleaved_tail_crossed_streams"
    );
    let decision = accepted(
        lifecycle.delta(&output, suffix),
        "c3_lifecycle_interleaved_restore_positive",
    )?;
    assert!(
        restored(&decision, suffix)?,
        "c3_lifecycle_interleaved_not_restored"
    );
    drop(accepted(
        lifecycle.done(&output, reference),
        "c3_lifecycle_interleaved_output_done_positive",
    )?);
    drop(accepted(
        lifecycle.done(&refusal, suffix),
        "c3_lifecycle_interleaved_refusal_done_positive",
    )?);
    assert!(
        matches!(
            accepted(
                lifecycle.done(&reasoning, reference),
                "c3_lifecycle_reasoning_done_positive"
            )?,
            RestoreDecision::Unchanged
        ) && lifecycle.dependency_tokens().len() == 1
            && lifecycle.restored_occurrences() == 1,
        "c3_lifecycle_interleaved_done_recounted"
    );
    let incomplete = accepted(
        lifecycle.open_text(TextPolicy::ToolArguments),
        "c3_lifecycle_incomplete_open_positive",
    )?;
    assert!(
        matches!(
            accepted(
                lifecycle.delta(&incomplete, prefix),
                "c3_lifecycle_incomplete_delta_positive"
            )?,
            RestoreDecision::Hold
        ) && matches!(
            accepted(
                lifecycle.done(&incomplete, prefix),
                "c3_lifecycle_incomplete_done_positive"
            )?,
            RestoreDecision::Unchanged
        ),
        "c3_lifecycle_incomplete_tail_not_reconciled"
    );
    drop(accepted(
        lifecycle.finish(),
        "c3_lifecycle_interleaved_finish_positive",
    )?);
    Ok(())
}

#[test]
fn c3_lifecycle_finished_inspection_rejects_foreign_and_surviving_ids_without_mutation(
) -> TestResult {
    let mut fixture = Fixture::new(false)?;
    let baseline = fixture.budget.used();
    let mut lifecycle = fixture.begin(limits())?;
    let own = accepted(
        lifecycle.open_text(TextPolicy::Output),
        "c3_lifecycle_inspection_own_open_positive",
    )?;
    drop(accepted(
        lifecycle.done(&own, ORDINARY),
        "c3_lifecycle_inspection_own_done_positive",
    )?);
    let finished = accepted(
        lifecycle.finish(),
        "c3_lifecycle_inspection_own_finish_positive",
    )?;

    let mut foreign_fixture = Fixture::new(false)?;
    let mut foreign_owner = foreign_fixture.begin(limits())?;
    let foreign = accepted(
        foreign_owner.open_text(TextPolicy::Output),
        "c3_lifecycle_inspection_foreign_open_positive",
    )?;
    let before_reject = fixture.budget.used();
    let foreign_before_reject = foreign_fixture.budget.used();
    assert!(
        finished.text_state(&foreign).err() == Some(RestoreError::RestoreFailed)
            && fixture.budget.used() == before_reject
            && foreign_fixture.budget.used() == foreign_before_reject,
        "c3_lifecycle_inspection_foreign_id_accepted_or_allocated"
    );
    accepted(
        foreign_owner.abort(),
        "c3_lifecycle_inspection_foreign_abort_positive",
    )?;
    let foreign_after_abort = foreign_fixture.budget.used();
    assert!(
        finished.text_state(&foreign).err() == Some(RestoreError::RestoreFailed)
            && fixture.budget.used() == before_reject
            && foreign_fixture.budget.used() == foreign_after_abort,
        "c3_lifecycle_inspection_surviving_foreign_id_accepted"
    );
    let first = accepted(
        finished.text_state(&own),
        "c3_lifecycle_inspection_own_borrow_positive",
    )?;
    let second = accepted(
        finished.text_state(&own),
        "c3_lifecycle_inspection_own_reborrow_positive",
    )?;
    assert!(
        std::ptr::eq(first, second)
            && first.raw_done.as_ref().map(PayloadText::as_str) == Some(ORDINARY)
            && first.restored_done.as_ref().map(PayloadText::as_str) == Some(ORDINARY)
            && finished.dependency_tokens().is_empty()
            && finished.restored_occurrences() == 0
            && fixture.budget.used() == before_reject,
        "c3_lifecycle_inspection_borrow_mutated_owner"
    );
    drop(finished);
    assert!(
        fixture.budget.used() > baseline,
        "c3_lifecycle_inspection_live_id_uncharged"
    );
    drop(own);
    drop(foreign);
    assert!(
        fixture.budget.used() == baseline,
        "c3_lifecycle_inspection_id_cleanup"
    );
    Ok(())
}

#[test]
fn c3_lifecycle_responses_begin_accepts_current_binding_without_optional_history() -> TestResult {
    let budget = PayloadBudget::with_limit(BUDGET_BYTES);
    let store = DlpTokenStore::new();
    let lease = store
        .admit_published_policy("synthetic-c3-responses-current", None, 1)
        .map_err(|_| "c3_lifecycle_responses_admission_fixture")?;
    let binding = lease
        .bind_actual_attempt(
            "synthetic-c3-responses-provider",
            "responses",
            "https://synthetic.invalid/v1/responses",
            None,
        )
        .map_err(|_| "c3_lifecycle_responses_binding_fixture")?;
    let client = ClientProtocol::new(Protocol::Responses);
    assert!(
        !lease.history_binding_live(&binding)
            && lease
                .require_current_attempt_binding(client, &binding)
                .is_ok(),
        "c3_lifecycle_responses_current_binding_precondition"
    );
    let attempt =
        AttemptProvenance::new(&budget).map_err(|_| "c3_lifecycle_responses_provenance_fixture")?;
    let baseline = budget.used();
    let lifecycle = accepted(
        ResponseLifecycle::begin(
            lease,
            binding,
            budget.clone(),
            ProtocolPair::new(client, UpstreamProtocol::new(Protocol::Responses)),
            attempt.clone(),
            limits(),
        ),
        "c3_lifecycle_responses_begin_current_positive",
    )?;
    assert!(
        lifecycle.dependency_tokens().is_empty()
            && lifecycle.restored_occurrences() == 0
            && lifecycle.failure().is_none()
            && budget.used() > baseline,
        "c3_lifecycle_responses_begin_granted_unrelated_authority"
    );
    accepted(
        lifecycle.abort(),
        "c3_lifecycle_responses_begin_abort_positive",
    )?;
    assert!(
        budget.used() == baseline,
        "c3_lifecycle_responses_begin_cleanup"
    );
    Ok(())
}

// Real Responses admission, deliberately without optional delivered-history eligibility.
struct ResponsesObservationFixture {
    _store: DlpTokenStore,
    budget: PayloadBudget,
    attempt: AttemptProvenance,
    lifecycle: ResponseLifecycle,
}

impl ResponsesObservationFixture {
    fn new() -> TestResult<Self> {
        let budget = PayloadBudget::with_limit(BUDGET_BYTES);
        let store = DlpTokenStore::new();
        let lease = store
            .admit_published_policy("synthetic-c3-responses-observation", None, 1)
            .map_err(|_| "c3_responses_observation_admission_fixture")?;
        let binding = lease
            .bind_actual_attempt(
                "synthetic-c3-responses-provider",
                "responses",
                "https://synthetic.invalid/v1/responses",
                None,
            )
            .map_err(|_| "c3_responses_observation_binding_fixture")?;
        let client = ClientProtocol::new(Protocol::Responses);
        assert!(
            !lease.history_binding_live(&binding)
                && lease
                    .require_current_attempt_binding(client, &binding)
                    .is_ok()
                && lease
                    .require_native_attempt_binding(client, &binding)
                    .is_err(),
            "c3_responses_observation_current_no_history_scope_precondition"
        );
        let attempt = AttemptProvenance::new(&budget)
            .map_err(|_| "c3_responses_observation_provenance_fixture")?;
        let lifecycle = accepted(
            ResponseLifecycle::begin(
                lease,
                binding,
                budget.clone(),
                ProtocolPair::new(client, UpstreamProtocol::new(Protocol::Responses)),
                attempt.clone(),
                limits(),
            ),
            "c3_responses_observation_begin_current_positive",
        )?;
        Ok(Self {
            _store: store,
            budget,
            attempt,
            lifecycle,
        })
    }
}

#[test]
fn c3_lifecycle_responses_original_json_accepts_current_no_history_without_completion() -> TestResult
{
    let mut fixture = ResponsesObservationFixture::new()?;
    let unfinished = accepted(
        fixture.lifecycle.open_text(TextPolicy::Output),
        "c3_responses_original_json_open_positive",
    )?;
    let native = json!({
        "id": "synthetic-c3-responses-observation",
        "object": "response",
        "status": "in_progress",
        "output": [
            {
                "id": "synthetic-c3-message",
                "type": "message",
                "role": "assistant",
                "content": [
                    {"type": "output_text", "text": ORDINARY},
                    {"type": "refusal", "refusal": "Synthetic refusal"}
                ]
            },
            {
                "id": "synthetic-c3-reasoning",
                "type": "reasoning",
                "summary": [{"type": "summary_text", "text": "Synthetic reasoning"}]
            }
        ]
    });
    let mut observation = accepted(
        OriginalResponseObservation::json(
            &native,
            UpstreamProtocol::new(Protocol::Responses),
            fixture.attempt.clone(),
            fixture.budget.clone(),
        ),
        "c3_responses_original_json_fixture",
    )?;
    for node in [
        &native["output"][0]["content"][0],
        &native["output"][0]["content"][1],
        &native["output"][1]["summary"][0],
    ] {
        accepted(
            observation.claim_node(node),
            "c3_responses_original_json_claim_positive",
        )?;
    }
    let before_observe = fixture.budget.used();
    accepted(
        fixture.lifecycle.observe_original(&observation),
        "c3_responses_original_json_current_no_history_positive",
    )?;
    assert!(
        matches!(observation.source(), OriginalResponseSource::Json(value) if std::ptr::eq(value, &native))
            && observation.claims().len() == 3
            && std::ptr::eq(observation.claims()[0], &native["output"][0]["content"][0])
            && std::ptr::eq(observation.claims()[1], &native["output"][0]["content"][1])
            && std::ptr::eq(observation.claims()[2], &native["output"][1]["summary"][0])
            && native["status"].as_str() == Some("in_progress")
            && native["output"][0]["content"][0]["text"].as_str() == Some(ORDINARY),
        "c3_responses_original_json_native_identity_changed"
    );
    assert!(
        fixture.lifecycle.failure().is_none()
            && fixture.lifecycle.dependency_tokens().is_empty()
            && fixture.lifecycle.restored_occurrences() == 0
            && fixture.lifecycle.held_units().is_empty()
            && fixture.lifecycle.held_bytes() == 0
            && fixture.budget.used() == before_observe,
        "c3_responses_original_json_granted_unrelated_authority"
    );
    // A valid original borrow must not finish an unfinished text stream or attest completion.
    assert!(
        fixture.lifecycle.finish().err() == Some(RestoreError::RestoreFailed),
        "c3_responses_original_json_completed_unfinished_stream"
    );
    drop(unfinished);
    Ok(())
}

#[test]
fn c3_lifecycle_responses_original_frame_accepts_current_no_history_without_eof() -> TestResult {
    let mut fixture = ResponsesObservationFixture::new()?;
    let unfinished = accepted(
        fixture.lifecycle.open_text(TextPolicy::Output),
        "c3_responses_original_frame_open_positive",
    )?;
    // An original byte chunk can end inside UTF-8, before JSON or SSE completion.
    let raw = b"event: response.output_text.delta\ndata: {\"type\":\"response.output_text.delta\",\"output_index\":7,\"content_index\":2,\"delta\":\"\xf0\x9f";
    let observation = accepted(
        OriginalResponseObservation::frame(
            raw,
            UpstreamProtocol::new(Protocol::Responses),
            fixture.attempt.clone(),
            fixture.budget.clone(),
        ),
        "c3_responses_original_frame_fixture",
    )?;
    let before_observe = fixture.budget.used();
    accepted(
        fixture.lifecycle.observe_original(&observation),
        "c3_responses_original_frame_current_no_history_positive",
    )?;
    assert!(
        matches!(observation.source(), OriginalResponseSource::Frame(bytes)
            if bytes.as_ptr() == raw.as_ptr() && bytes.len() == raw.len())
            && observation.claims().is_empty()
            && fixture.lifecycle.failure().is_none()
            && fixture.lifecycle.dependency_tokens().is_empty()
            && fixture.lifecycle.restored_occurrences() == 0
            && fixture.lifecycle.held_units().is_empty()
            && fixture.lifecycle.held_bytes() == 0
            && fixture.budget.used() == before_observe,
        "c3_responses_original_frame_changed_bytes_or_granted_authority"
    );
    assert!(
        fixture.lifecycle.finish().err() == Some(RestoreError::RestoreFailed),
        "c3_responses_original_frame_attested_eof"
    );
    drop(unfinished);
    Ok(())
}

#[test]
fn c3_lifecycle_responses_original_rejects_wrong_provenance_and_failure_sticks() -> TestResult {
    let mut fixture = ResponsesObservationFixture::new()?;
    let native = json!({"id": "synthetic-c3-original", "output": [], "status": "in_progress"});
    let valid = accepted(
        OriginalResponseObservation::json(
            &native,
            UpstreamProtocol::new(Protocol::Responses),
            fixture.attempt.clone(),
            fixture.budget.clone(),
        ),
        "c3_responses_original_provenance_valid_fixture",
    )?;
    // A blanket Responses rejection cannot satisfy the negative provenance contract.
    accepted(
        fixture.lifecycle.observe_original(&valid),
        "c3_responses_original_provenance_control_positive",
    )?;
    let other = AttemptProvenance::new(&fixture.budget)
        .map_err(|_| "c3_responses_original_foreign_provenance_fixture")?;
    let foreign = accepted(
        OriginalResponseObservation::json(
            &native,
            UpstreamProtocol::new(Protocol::Responses),
            other,
            fixture.budget.clone(),
        ),
        "c3_responses_original_foreign_observation_fixture",
    )?;
    let before_reject = fixture.budget.used();
    assert!(
        fixture.lifecycle.observe_original(&foreign).err() == Some(RestoreError::RestoreFailed)
            && fixture.lifecycle.failure() == Some(RestoreError::RestoreFailed)
            && fixture.lifecycle.observe_original(&valid).err()
                == Some(RestoreError::RestoreFailed)
            && fixture.lifecycle.open_text(TextPolicy::Output).err()
                == Some(RestoreError::RestoreFailed)
            && fixture.lifecycle.dependency_tokens().is_empty()
            && fixture.lifecycle.restored_occurrences() == 0
            && fixture.lifecycle.held_units().is_empty()
            && fixture.budget.used() == before_reject,
        "c3_responses_original_foreign_attempt_accepted_or_failure_not_sticky"
    );
    assert!(
        fixture.lifecycle.finish().err() == Some(RestoreError::RestoreFailed),
        "c3_responses_original_foreign_attempt_finished"
    );
    Ok(())
}

#[test]
fn c3_lifecycle_responses_original_rejects_wrong_upstream_and_failure_sticks() -> TestResult {
    let mut fixture = ResponsesObservationFixture::new()?;
    let raw = b"event: response.created\ndata: {\"type\":\"response.created\"}\n\n";
    let valid = accepted(
        OriginalResponseObservation::frame(
            raw,
            UpstreamProtocol::new(Protocol::Responses),
            fixture.attempt.clone(),
            fixture.budget.clone(),
        ),
        "c3_responses_original_upstream_valid_fixture",
    )?;
    accepted(
        fixture.lifecycle.observe_original(&valid),
        "c3_responses_original_upstream_control_positive",
    )?;
    let wrong_upstream = accepted(
        OriginalResponseObservation::frame(
            raw,
            UpstreamProtocol::new(Protocol::Chat),
            fixture.attempt.clone(),
            fixture.budget.clone(),
        ),
        "c3_responses_original_wrong_upstream_fixture",
    )?;
    let before_reject = fixture.budget.used();
    assert!(
        fixture.lifecycle.observe_original(&wrong_upstream).err()
            == Some(RestoreError::RestoreFailed)
            && fixture.lifecycle.failure() == Some(RestoreError::RestoreFailed)
            && fixture.lifecycle.observe_original(&valid).err()
                == Some(RestoreError::RestoreFailed)
            && fixture.lifecycle.open_text(TextPolicy::Output).err()
                == Some(RestoreError::RestoreFailed)
            && fixture.lifecycle.dependency_tokens().is_empty()
            && fixture.lifecycle.restored_occurrences() == 0
            && fixture.lifecycle.held_units().is_empty()
            && fixture.budget.used() == before_reject,
        "c3_responses_original_wrong_upstream_accepted_or_failure_not_sticky"
    );
    assert!(
        fixture.lifecycle.finish().err() == Some(RestoreError::RestoreFailed),
        "c3_responses_original_wrong_upstream_finished"
    );
    Ok(())
}

#[test]
fn c3_lifecycle_rejection_hook_begin_rejects_actual_owner_creation() -> TestResult {
    use super::restoration_core::LifecycleOperation;
    let mut fixture = Fixture::new(false)?;
    let baseline = fixture.budget.used();
    let rejection = ResponseLifecycle::reject_operation_for_test(LifecycleOperation::Begin);
    let result = ResponseLifecycle::begin(
        fixture.lease.take().ok_or("c3_hook_lease_missing")?,
        fixture.binding.take().ok_or("c3_hook_binding_missing")?,
        fixture.budget.clone(),
        pair(),
        fixture.attempt.clone(),
        limits(),
    );
    assert!(
        result.err() == Some(RestoreError::RestoreFailed) && fixture.budget.used() == baseline,
        "c3_hook_begin_created_owner_or_leaked_budget"
    );
    drop(rejection);
    Ok(())
}

#[test]
fn c3_lifecycle_rejection_hook_operations_poison_actual_owner() -> TestResult {
    use super::restoration_core::LifecycleOperation as Op;
    for operation in [
        Op::OpenText,
        Op::Delta,
        Op::Done,
        Op::ObserveOriginal,
        Op::Hold,
    ] {
        let mut fixture = ResponsesObservationFixture::new()?;
        let id = accepted(
            fixture.lifecycle.open_text(TextPolicy::Output),
            "c3_hook_open_control",
        )?;
        let native = json!({"output": []});
        let observation = accepted(
            OriginalResponseObservation::json(
                &native,
                UpstreamProtocol::new(Protocol::Responses),
                fixture.attempt.clone(),
                fixture.budget.clone(),
            ),
            "c3_hook_observation_fixture",
        )?;
        let unit =
            PayloadBuffer::try_from_slice(b"synthetic-native-unit", fixture.budget.clone(), 128)
                .map_err(|_| "c3_hook_unit_fixture")?;
        let rejection = ResponseLifecycle::reject_operation_for_test(operation);
        let result = match operation {
            Op::OpenText => fixture.lifecycle.open_text(TextPolicy::Refusal).map(|_| ()),
            Op::Delta => fixture.lifecycle.delta(&id, ORDINARY).map(|_| ()),
            Op::Done => fixture.lifecycle.done(&id, ORDINARY).map(|_| ()),
            Op::ObserveOriginal => fixture.lifecycle.observe_original(&observation),
            Op::Hold => fixture.lifecycle.hold(unit),
            _ => return Err("c3_hook_unexpected_operation"),
        };
        assert!(
            result.err() == Some(RestoreError::RestoreFailed),
            "c3_hook_actual_operation_not_rejected"
        );
        drop(rejection);
        assert!(
            fixture.lifecycle.failure() == Some(RestoreError::RestoreFailed)
                && fixture.lifecycle.open_text(TextPolicy::Output).err()
                    == Some(RestoreError::RestoreFailed)
                && fixture.lifecycle.dependency_tokens().is_empty()
                && fixture.lifecycle.restored_occurrences() == 0
                && fixture.lifecycle.held_units().is_empty()
                && fixture.lifecycle.held_bytes() == 0,
            "c3_hook_owner_failure_not_sticky_or_committed"
        );
        assert!(
            fixture.lifecycle.finish().err() == Some(RestoreError::RestoreFailed),
            "c3_hook_poisoned_owner_finished"
        );
    }
    Ok(())
}

#[test]
fn c3_lifecycle_rejection_hook_finish_consumes_without_inspection() -> TestResult {
    use super::restoration_core::LifecycleOperation;
    let fixture = ResponsesObservationFixture::new()?;
    let rejection = ResponseLifecycle::reject_operation_for_test(LifecycleOperation::Finish);
    assert!(
        fixture.lifecycle.finish().err() == Some(RestoreError::RestoreFailed),
        "c3_hook_finish_created_inspection"
    );
    drop(rejection);
    let control = ResponsesObservationFixture::new()?;
    drop(accepted(
        control.lifecycle.finish(),
        "c3_hook_finish_control",
    )?);
    Ok(())
}

#[test]
fn c3_lifecycle_rejection_hook_nested_guards_restore_fixed_operation() -> TestResult {
    use super::restoration_core::LifecycleOperation;
    let outer = ResponseLifecycle::reject_operation_for_test(LifecycleOperation::Done);
    {
        let inner = ResponseLifecycle::reject_operation_for_test(LifecycleOperation::Delta);
        let mut fixture = ResponsesObservationFixture::new()?;
        let id = accepted(
            fixture.lifecycle.open_text(TextPolicy::Output),
            "c3_hook_nested_open",
        )?;
        assert!(
            fixture.lifecycle.delta(&id, ORDINARY).err() == Some(RestoreError::RestoreFailed),
            "c3_hook_inner_selection_missing"
        );
        accepted(fixture.lifecycle.abort(), "c3_hook_abort_rejected")?;
        drop(inner);
    }
    let mut fixture = ResponsesObservationFixture::new()?;
    let id = accepted(
        fixture.lifecycle.open_text(TextPolicy::Output),
        "c3_hook_outer_open",
    )?;
    drop(accepted(
        fixture.lifecycle.delta(&id, ORDINARY),
        "c3_hook_outer_rejected_other_operation",
    )?);
    assert!(
        fixture.lifecycle.done(&id, ORDINARY).err() == Some(RestoreError::RestoreFailed),
        "c3_hook_outer_selection_not_restored"
    );
    accepted(fixture.lifecycle.abort(), "c3_hook_outer_abort_rejected")?;
    drop(outer);
    let mut control = ResponsesObservationFixture::new()?;
    let id = accepted(
        control.lifecycle.open_text(TextPolicy::Output),
        "c3_hook_reset_open",
    )?;
    drop(accepted(
        control.lifecycle.done(&id, ORDINARY),
        "c3_hook_guard_leaked_selection",
    )?);
    drop(accepted(
        control.lifecycle.finish(),
        "c3_hook_reset_finish",
    )?);
    Ok(())
}
