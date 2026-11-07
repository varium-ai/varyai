//! Verifier-backed publication and the only terminal-delivery continuation issuer.
use super::*;
use crate::proxy::security::{
    completed_context::CompletedCallPublications,
    http_restoration::{VerifiedCompletion, VerifiedJsonCompletion},
    payload_budget::Reservation,
    restoration_core::AttemptProvenance,
    tokenization::PendingPublication,
};
use futures::Stream;
use sha2::{Digest, Sha256};
use std::{pin::Pin, sync::Arc};

#[derive(Clone)]
pub(in crate::proxy::security) struct ReleaseIdentity(Arc<ReleaseData>);
struct ReleaseData {
    _attempt: Option<AttemptProvenance>,
    _framing: [u8; 32],
    _charge: Reservation,
}
impl ReleaseIdentity {
    fn new(
        budget: &PayloadBudget,
        attempt: Option<AttemptProvenance>,
        framing: [u8; 32],
    ) -> Result<Self, RestoreError> {
        let bytes = std::mem::size_of::<ReleaseData>()
            .checked_add(2 * std::mem::size_of::<usize>())
            .ok_or(RestoreError::RestoreFailed)?;
        Ok(Self(Arc::new(ReleaseData {
            _attempt: attempt,
            _framing: framing,
            _charge: budget.reserve(bytes).map_err(RestoreError::capacity)?,
        })))
    }
    fn matches(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

pub(super) struct FinalizationPermit {
    release: ReleaseIdentity,
}

// Views carry no issuing authority. The store accepts only the sealed spec by value.
pub(in crate::proxy::security) struct PublicationSpec<'a> {
    binding: &'a ActualAttemptBinding,
    response_id: &'a str,
    output_tokens: &'a [&'a str],
    calls: CompletedCallPublications<'a>,
    opaques: &'a [&'a str],
    release: ReleaseIdentity,
}
pub(in crate::proxy::security) struct PublicationView<'a> {
    pub binding: &'a ActualAttemptBinding,
    pub response_id: &'a str,
    pub output_tokens: &'a [&'a str],
    pub calls: CompletedCallPublications<'a>,
    pub opaques: &'a [&'a str],
}
impl PublicationSpec<'_> {
    pub(in crate::proxy::security) fn view(&self) -> PublicationView<'_> {
        PublicationView {
            binding: self.binding,
            response_id: self.response_id,
            output_tokens: self.output_tokens,
            calls: CompletedCallPublications {
                originals: self.calls.originals,
                delivered: self.calls.delivered,
            },
            opaques: self.opaques,
        }
    }
    pub(in crate::proxy::security) fn release_identity(&self) -> ReleaseIdentity {
        self.release.clone()
    }
}

pub(in crate::proxy::security) struct DeliveryResume {
    release: ReleaseIdentity,
}
impl DeliveryResume {
    pub(in crate::proxy::security) fn matches(&self, release: &ReleaseIdentity) -> bool {
        self.release.matches(release)
    }
}

pub(in crate::proxy::security) struct FinalizedBatch {
    frames: Vec<Bytes>,
    pending: Option<PendingPublication>,
    history: Option<crate::proxy::security::history_provenance::PendingDeliveredHistory>,
    lease: DlpRequestLease,
    binding: ActualAttemptBinding,
    release: ReleaseIdentity,
    _frame_metadata: Option<Reservation>,
}
impl FinalizedBatch {
    pub(in crate::proxy::security) fn into_delivery_stream(
        self,
    ) -> Pin<Box<dyn Stream<Item = Result<Bytes, io::Error>> + Send>> {
        Box::pin(async_stream::stream! {
            let Self { frames, pending, history, lease, binding, release, _frame_metadata } = self;
            // The backing charge and pending owner remain live across every yield.
            let _metadata = _frame_metadata;
            for frame in frames { yield Ok(frame); }
            let resume = DeliveryResume { release };
            if let Some(pending) = pending {
                if pending.activate(resume).is_err() {
                    // Released terminal output cannot be retracted or followed by another terminal.
                    yield Err(io::Error::other("Response stream failed."));
                    return;
                }
            }
            if let Some(history) = history { let _ = lease.publish_delivered_history(&binding, history); }
        })
    }
}

impl SseRestorationState {
    pub(in crate::proxy::security) fn finalize(
        mut self,
        proof: VerifiedCompletion,
    ) -> Result<FinalizedBatch, (RestoreError, RestoreStage)> {
        self.failure_stage = RestoreStage::CompletionEvidence;
        proof
            .validate_sse(&self)
            .map_err(|error| (error, self.failure_stage))?;
        if let Some(error) = self.failed {
            return Err((error, self.failure_stage));
        }
        let permit = FinalizationPermit {
            release: ReleaseIdentity::new(
                &self.budget,
                self.completion_attempt().cloned(),
                self.completion_frame_digest(),
            )
            .map_err(|error| (error, self.failure_stage))?,
        };
        let frames = self
            .finalize_and_flush_inner(&permit)
            .map_err(|error| (error, self.failure_stage))?;
        Ok(FinalizedBatch {
            frames,
            pending: self.pending_publication.take(),
            history: self.pending_delivered_history.take(),
            lease: self.lease,
            binding: self.binding,
            release: permit.release,
            _frame_metadata: self.frame_metadata.take(),
        })
    }
}

impl StagedJsonResponse<'_> {
    pub(in crate::proxy::security) fn encode_and_publish(
        self,
        lease: &DlpRequestLease,
        binding: &ActualAttemptBinding,
        proof: VerifiedJsonCompletion,
        budget: &PayloadBudget,
        max: usize,
    ) -> Result<(RestoredJsonResponse, PayloadBuffer), RestoreError> {
        let attempt = self.output_tokens.0.attempt();
        proof.validate_json(self.source, binding, attempt)?;
        // This owner cannot issue activation permission until the mandatory charged encoding succeeds.
        let encoded = serialize_json_with_budget(self.as_value(), budget, max)?;
        let raw =
            std::str::from_utf8(encoded.as_slice()).map_err(|_| RestoreError::RestoreFailed)?;
        let mut completion =
            super::super::completion_json::JsonCompletion::new(self.completion_protocol);
        drop(completion.observe_raw_json_scoped(raw, budget));
        if !completion.permits_completion() {
            return Err(RestoreError::RestoreFailed);
        }
        let permit = FinalizationPermit {
            release: ReleaseIdentity::new(budget, Some(attempt.clone()), [0; 32])?,
        };
        let pending = prepare_snapshot(
            self.source,
            DeliveredToolSnapshots {
                response: self.as_value(),
                tracks: &[],
                events: &[],
            },
            &self.output_tokens,
            lease,
            binding,
            &permit,
            budget,
        )?;
        if let Some(pending) = pending {
            pending
                .activate(DeliveryResume {
                    release: permit.release,
                })
                .map_err(|_| RestoreError::RestoreFailed)?;
        }
        Ok((self.restored, encoded))
    }
}

#[cfg(test)]
pub(super) fn fixture_permit(budget: &PayloadBudget) -> Result<FinalizationPermit, RestoreError> {
    Ok(FinalizationPermit {
        release: ReleaseIdentity::new(budget, None, [0; 32])?,
    })
}
#[cfg(test)]
pub(super) fn activate_fixture(pending: PendingPublication) -> Result<(), RestoreError> {
    pending
        .activate_fixture()
        .map(|_| ())
        .map_err(|_| RestoreError::RestoreFailed)
}
#[cfg(test)]
pub(super) fn publish_fixture(
    response: &Value,
    delivery: DeliveredToolSnapshots<'_>,
    output_tokens: &[PayloadText],
    lease: &DlpRequestLease,
    binding: &ActualAttemptBinding,
    _witness: &CompletionWitness,
    budget: &PayloadBudget,
) -> Result<(), RestoreError> {
    let permit = fixture_permit(budget)?;
    if let Some(pending) = prepare_snapshot(
        response,
        delivery,
        output_tokens,
        lease,
        binding,
        &permit,
        budget,
    )? {
        activate_fixture(pending)?;
    }
    Ok(())
}

pub(super) fn prepare_snapshot(
    response: &Value,
    delivery: DeliveredToolSnapshots<'_>,
    output_tokens: &[PayloadText],
    lease: &DlpRequestLease,
    binding: &ActualAttemptBinding,
    permit: &FinalizationPermit,
    budget: &PayloadBudget,
) -> Result<Option<PendingPublication>, RestoreError> {
    if !binding.reusable_principal {
        return Ok(None);
    }
    let response_id = response
        .get("id")
        .and_then(Value::as_str)
        .filter(|id| valid_external_id(id))
        .ok_or(RestoreError::RestoreFailed)?;
    let mut tokens = PayloadVec::new(budget.clone()).map_err(|_| RestoreError::RestoreFailed)?;
    for token in output_tokens {
        tokens
            .push(token.as_str())
            .map_err(|_| RestoreError::RestoreFailed)?;
    }
    let mut calls = PayloadVec::new(budget.clone()).map_err(|_| RestoreError::RestoreFailed)?;
    let mut opaques = PayloadVec::new(budget.clone()).map_err(|_| RestoreError::RestoreFailed)?;
    for item in response
        .get("output")
        .and_then(Value::as_array)
        .ok_or(RestoreError::RestoreFailed)?
    {
        let kind = item
            .get("type")
            .and_then(Value::as_str)
            .ok_or(RestoreError::RestoreFailed)?;
        if matches!(kind, "function_call" | "custom_tool_call") {
            calls
                .push(completed_call_spec(item)?)
                .map_err(|_| RestoreError::RestoreFailed)?;
        }
        for field in ["encrypted_content", "encrypted_reasoning"] {
            if let Some(text) = item
                .get(field)
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
            {
                opaques
                    .push(text)
                    .map_err(|_| RestoreError::RestoreFailed)?;
            }
        }
    }
    let mut delivered = PayloadVec::new(budget.clone()).map_err(|_| RestoreError::RestoreFailed)?;
    for (index, item) in delivery
        .response
        .get("output")
        .and_then(Value::as_array)
        .ok_or(RestoreError::RestoreFailed)?
        .iter()
        .enumerate()
    {
        if !matches!(
            item.get("type").and_then(Value::as_str),
            Some("function_call" | "custom_tool_call")
        ) {
            continue;
        }
        let spec = completed_call_spec(item)?;
        delivered
            .push(spec)
            .map_err(|_| RestoreError::RestoreFailed)?;
        if let Some(track) = delivery
            .tracks
            .iter()
            .find(|track| track.output_index == index as u64 && !track.payload_replaced)
        {
            // Only complete carriers validated against this final snapshot may join its record.
            let delta = if track.saw_deltas {
                Some(
                    std::str::from_utf8(track.raw_deltas.as_slice())
                        .map_err(|_| RestoreError::RestoreFailed)?,
                )
            } else {
                None
            };
            for payload in [
                delta,
                track.raw_done.as_ref().map(PayloadText::as_str),
                track
                    .raw_item_done
                    .as_ref()
                    .filter(|_| {
                        delivery.events.iter().any(|event| {
                            let data = event.data.as_value();
                            if event.event_name.as_str() != "response.output_item.done"
                                || data.get("output_index").and_then(Value::as_u64)
                                    != Some(index as u64)
                            {
                                return false;
                            }
                            let Some(raw) = data.get("item") else {
                                return false;
                            };
                            // Replay does not fill missing reasoning or rewrite opaque siblings.
                            raw.get("reasoning_content").and_then(Value::as_str) == spec.reasoning
                                && raw.get("encrypted_reasoning").and_then(Value::as_str)
                                    == spec.encrypted_reasoning
                        })
                    })
                    .map(PayloadText::as_str),
            ]
            .into_iter()
            .flatten()
            {
                delivered
                    .push(CompletedCallSpec { payload, ..spec })
                    .map_err(|_| RestoreError::RestoreFailed)?;
            }
        }
    }
    let spec = PublicationSpec {
        binding,
        response_id,
        output_tokens: tokens.as_slice(),
        calls: CompletedCallPublications {
            originals: calls.as_slice(),
            delivered: delivered.as_slice(),
        },
        opaques: opaques.as_slice(),
        release: permit.release.clone(),
    };
    lease
        .prepare_publication(spec)
        .map(Some)
        .map_err(|_| RestoreError::RestoreFailed)
}

/// Native snapshot specifications can only be issued by consuming finalization.
/// Read-only views carry no publication or activation capability.
pub(in crate::proxy::security) struct NativePublicationSpec<'a> {
    binding: &'a ActualAttemptBinding,
    client: crate::proxy::security::restoration_contract::ClientProtocol,
    delivered: &'a Value,
    output_tokens: PayloadVec<&'a str>,
    release: ReleaseIdentity,
}
pub(in crate::proxy::security) struct NativePublicationView<'a> {
    pub binding: &'a ActualAttemptBinding,
    pub client: crate::proxy::security::restoration_contract::ClientProtocol,
    pub delivered: &'a Value,
    pub output_tokens: &'a [&'a str],
}
impl NativePublicationSpec<'_> {
    pub(in crate::proxy::security) fn view(&self) -> NativePublicationView<'_> {
        NativePublicationView {
            binding: self.binding,
            client: self.client,
            delivered: self.delivered,
            output_tokens: self.output_tokens.as_slice(),
        }
    }
    pub(in crate::proxy::security) fn release_identity(&self) -> ReleaseIdentity {
        self.release.clone()
    }
}

/// Opaque native JSON delivery, created only after the JSON owner activates its slot.
/// Its eventual single-chunk stream must carry no pending owner or activation gate.
pub(in crate::proxy::security) struct FinalizedNativeJson {
    wire: Bytes,
}

impl FinalizedNativeJson {
    pub(in crate::proxy::security) fn into_delivery_stream(
        self,
    ) -> Pin<Box<dyn Stream<Item = Result<Bytes, io::Error>> + Send>> {
        Box::pin(futures::stream::once(async move { Ok(self.wire) }))
    }
}

/// Verify and encode before activating the exact slot, prior to any body poll.
pub(in crate::proxy::security) fn finalize_native_json(
    stage: crate::proxy::security::restoration_core::StagedNativeJson<'_>,
    lease: &DlpRequestLease,
    proof: crate::proxy::security::http_restoration::VerifiedNativeJson,
    attempt: &AttemptProvenance,
    original: Bytes,
    max: usize,
) -> Result<FinalizedNativeJson, RestoreError> {
    lease
        .require_native_attempt_binding(stage.client(), stage.binding())
        .map_err(|_| RestoreError::RestoreFailed)?;
    let prepared = prepare_native_json_spec(stage, proof, attempt, original, max)?;
    let pending = lease
        .prepare_native_publication(prepared.spec()?)
        .map_err(|_| RestoreError::RestoreFailed)?;
    pending
        .activate(DeliveryResume {
            release: prepared.release.clone(),
        })
        .map_err(|_| RestoreError::RestoreFailed)?;
    Ok(FinalizedNativeJson {
        wire: prepared.wire,
    })
}

// This consuming owner and its inspection tests stay inside the sole finalizer issuer.
// Exact original JSON parsing and mandatory final encoding use only the stage budget.
// A private JSON owner may issue DeliveryResume only after that encoding succeeds;
// it then prepares and activates the same slot before returning opaque delivery.
// No test helper exports bytes plus pending state, a release identity, or permission.
struct PreparedNativeJsonSpec<'a> {
    wire: Bytes,
    stage: crate::proxy::security::restoration_core::StagedNativeJson<'a>,
    release: ReleaseIdentity,
}

impl PreparedNativeJsonSpec<'_> {
    fn spec(&self) -> Result<NativePublicationSpec<'_>, RestoreError> {
        let mut output_tokens =
            PayloadVec::new(self.stage.budget()).map_err(RestoreError::capacity)?;
        for token in self.stage.output_tokens() {
            output_tokens
                .push(token.as_str())
                .map_err(RestoreError::capacity)?;
        }
        Ok(NativePublicationSpec {
            binding: self.stage.binding(),
            client: self.stage.client(),
            delivered: self.stage.delivered(),
            output_tokens,
            release: self.release.clone(),
        })
    }
}

// Compare the exact serialized Value representation used by the native verifier,
// not object equality, which can ignore the order bound into its source digest.
fn native_json_source_digest(source: &Value) -> Result<[u8; 32], RestoreError> {
    struct DigestWriter(Sha256);
    impl io::Write for DigestWriter {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.0.update(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    let mut writer = DigestWriter(Sha256::new());
    serde_json::to_writer(&mut writer, source).map_err(|_| RestoreError::RestoreFailed)?;
    Ok(writer.0.finalize().into())
}

fn prepare_native_json_spec<'a>(
    stage: crate::proxy::security::restoration_core::StagedNativeJson<'a>,
    proof: crate::proxy::security::http_restoration::VerifiedNativeJson,
    attempt: &AttemptProvenance,
    original: Bytes,
    max: usize,
) -> Result<PreparedNativeJsonSpec<'a>, RestoreError> {
    stage.check_lifecycle_attempt(attempt)?;
    proof.check_source(stage.source(), stage.binding(), attempt, stage.client())?;
    let budget = stage.budget();
    {
        let raw =
            std::str::from_utf8(original.as_ref()).map_err(|_| RestoreError::RestoreFailed)?;
        // The strict parser rejects incomplete entities, trailing data and duplicate
        // keys, with both its retained tree and scratch charged to the stage budget.
        let parsed = PayloadJson::try_from_str(raw, budget.clone(), budget.limit())
            .map_err(RestoreError::capacity)?;
        if native_json_source_digest(parsed.as_value())?
            != native_json_source_digest(stage.source())?
        {
            return Err(RestoreError::RestoreFailed);
        }
    }
    // Even unchanged delivery must pass the final size and charged-allocation gate.
    let encoded = serialize_json_with_budget(stage.as_value(), &budget, max)?;
    let bytes = if stage.changed() {
        encoded.as_slice()
    } else {
        original.as_ref()
    };
    proof.validate_restored(
        stage.source(),
        stage.binding(),
        attempt,
        stage.client(),
        bytes,
        &budget,
    )?;
    let mut framing = Sha256::new();
    framing.update(b"native-json-delivery-v1\0");
    framing.update((bytes.len() as u64).to_le_bytes());
    framing.update(bytes);
    let release = ReleaseIdentity::new(&budget, Some(attempt.clone()), framing.finalize().into())?;
    let wire = if stage.changed() {
        struct ChargedEncoding(PayloadBuffer);
        impl AsRef<[u8]> for ChargedEncoding {
            fn as_ref(&self) -> &[u8] {
                self.0.as_slice()
            }
        }
        drop(original);
        Bytes::from_owner(ChargedEncoding(encoded))
    } else {
        // Keep the exact original allocation and its owner, never a reencoded copy.
        drop(encoded);
        original
    };
    Ok(PreparedNativeJsonSpec {
        wire,
        stage,
        release,
    })
}

/// Unconstructible outside this sole issuer; never returned to an adapter.
pub(in crate::proxy::security) struct NativeSseFinalizationKey {
    _private: (),
}

/// Opaque native SSE delivery: pending authority remains owned across all yields.
pub(in crate::proxy::security) struct FinalizedNativeSse {
    reversed: PayloadVec<PayloadBuffer>,
    pending: PendingPublication,
    release: ReleaseIdentity,
}
impl FinalizedNativeSse {
    pub(in crate::proxy::security) fn into_delivery_stream(
        self,
    ) -> Pin<Box<dyn Stream<Item = Result<Bytes, io::Error>> + Send>> {
        Box::pin(async_stream::stream! {
            let Self { mut reversed, pending, release } = self;
            struct ChargedFrame(PayloadBuffer);
            impl AsRef<[u8]> for ChargedFrame {
                fn as_ref(&self) -> &[u8] { self.0.as_slice() }
            }
            while let Some(frame) = reversed.pop() {
                yield Ok(Bytes::from_owner(ChargedFrame(frame)));
            }
            // Polling after the last yield, not handing out DONE, authorizes activation.
            if pending.activate(DeliveryResume { release }).is_err() {
                yield Err(io::Error::other("Response stream failed."));
            }
        })
    }
}

/// Consume exact staged wire only after original HTTP proof and restored-wire checks.
/// No bytes/pending pair, release key, or activation capability leaves this issuer.
pub(in crate::proxy::security) fn finalize_native_sse(
    stage: crate::proxy::security::restoration_core::StagedNativeSse<'_>,
    lease: &DlpRequestLease,
    proof: crate::proxy::security::http_restoration::VerifiedNativeSse,
    attempt: &AttemptProvenance,
    max: usize,
) -> Result<FinalizedNativeSse, RestoreError> {
    stage.check_lifecycle_attempt(attempt)?;
    lease
        .require_native_attempt_binding(stage.client(), stage.binding())
        .map_err(|_| RestoreError::RestoreFailed)?;
    let key = NativeSseFinalizationKey { _private: () };
    let budget = stage.budget();
    proof.validate_restored(
        stage.entity(),
        stage.binding(),
        attempt,
        stage.client(),
        stage.finalization_reader(&key, max)?,
        &budget,
    )?;
    let release = ReleaseIdentity::new(&budget, Some(attempt.clone()), stage.release_digest())?;
    let mut output_tokens = PayloadVec::new(budget).map_err(RestoreError::capacity)?;
    for token in stage.output_tokens() {
        output_tokens
            .push(token.as_str())
            .map_err(RestoreError::capacity)?;
    }
    let spec = NativePublicationSpec {
        binding: stage.binding(),
        client: stage.client(),
        delivered: stage.delivered(),
        output_tokens,
        release: release.clone(),
    };
    let pending = lease
        .prepare_native_publication(spec)
        .map_err(|_| RestoreError::RestoreFailed)?;
    // Transfer charged encodings by value; metadata allocation failure drops the slot.
    let reversed = stage.into_finalization_frames(&key)?;
    Ok(FinalizedNativeSse {
        reversed,
        pending,
        release,
    })
}

#[cfg(test)]
#[path = "../tests/c5_native_finalization_tests.rs"]
mod c5_native_finalization_tests;

#[cfg(test)]
#[path = "../tests/c5_native_sse_finalization_tests.rs"]
mod c5_native_sse_finalization_tests;
