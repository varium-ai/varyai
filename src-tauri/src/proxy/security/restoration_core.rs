//! Shared request authority and response mechanics; no route or completion issuer capability.
use super::{
    completed_context::ActualAttemptBinding,
    payload_budget::PayloadBudget,
    request_plan::PlanError,
    request_preparation::DlpPreparedRequest,
    restore_diagnostics::ContextCheckpoint,
    runtime::SecurityRuntimeConfig,
    tokenization::{DlpRequestLease, DlpTokenStore, TokenError},
};

#[path = "restoration_core/native_response.rs"]
mod native_response;
// Frozen adapter-facing API; production native lanes remain deliberately disabled.
#[allow(unused_imports)]
pub(super) use native_response::{
    stage_native_json, stage_native_json_with_lifecycle, stage_native_sse,
    stage_native_sse_with_lifecycle, NativeJsonObservation, NativeSseFrameObservation,
    NativeSseObservation, NativeSseSnapshotSelection, NativeSseTextStream, StagedNativeJson,
    StagedNativeSse,
};
// Internal deterministic mechanics; not an adapter snapshot attestation API.
pub(super) use native_response::{
    compose_anthropic_snapshots, compose_chat_snapshots, compose_gemini_snapshots,
};

pub(super) use super::output_text_matching::restore_text;

// The transaction remains the existing budgeted, atomic owner; no second store is introduced.
pub(super) use super::request_transaction::prepare_fields_with_history;

pub(super) struct RequestAuthority {
    lease: DlpRequestLease,
    budget: PayloadBudget,
}
#[derive(Debug)]
pub(super) struct AttemptAuthorityError {
    pub(super) error: TokenError,
    pub(super) checkpoint: ContextCheckpoint,
}

impl From<AttemptAuthorityError> for TokenError {
    fn from(error: AttemptAuthorityError) -> Self {
        error.error
    }
}

impl std::fmt::Display for AttemptAuthorityError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.error.code())
    }
}
impl std::error::Error for AttemptAuthorityError {}

impl RequestAuthority {
    pub(super) fn admit_responses(
        store: &DlpTokenStore,
        app: &str,
        session: Option<&str>,
        policy_revision: u64,
        budget: PayloadBudget,
    ) -> Result<Self, TokenError> {
        Ok(Self {
            lease: store.admit_published_policy(app, session, policy_revision)?,
            budget,
        })
    }

    pub(super) fn admit_native_chat(
        store: &DlpTokenStore,
        app: &str,
        session: Option<&str>,
        policy_revision: u64,
        budget: PayloadBudget,
    ) -> Result<Self, TokenError> {
        use super::restoration_contract::{ClientProtocol, ContinuityScope, IngressNamespace};
        Ok(Self {
            lease: store.admit_scoped_published_policy(
                ContinuityScope::Native {
                    client: ClientProtocol::new(crate::proxy::protocol_bridge::Protocol::Chat),
                    ingress: IngressNamespace::Direct,
                },
                app,
                session,
                policy_revision,
            )?,
            budget,
        })
    }

    pub(super) fn admit_native_gemini(
        store: &DlpTokenStore,
        app: &str,
        session: Option<&str>,
        policy_revision: u64,
        budget: PayloadBudget,
    ) -> Result<Self, TokenError> {
        use super::restoration_contract::{ClientProtocol, ContinuityScope, IngressNamespace};
        Ok(Self {
            lease: store.admit_scoped_published_policy(
                ContinuityScope::Native {
                    client: ClientProtocol::new(crate::proxy::protocol_bridge::Protocol::Gemini),
                    ingress: IngressNamespace::Direct,
                },
                app,
                session,
                policy_revision,
            )?,
            budget,
        })
    }

    pub(super) fn admit_native_anthropic(
        store: &DlpTokenStore,
        app: &str,
        session: Option<&str>,
        policy_revision: u64,
        budget: PayloadBudget,
    ) -> Result<Self, TokenError> {
        use super::restoration_contract::{ClientProtocol, ContinuityScope, IngressNamespace};
        Ok(Self {
            lease: store.admit_scoped_published_policy(
                ContinuityScope::Native {
                    client: ClientProtocol::new(crate::proxy::protocol_bridge::Protocol::Anthropic),
                    ingress: IngressNamespace::Direct,
                },
                app,
                session,
                policy_revision,
            )?,
            budget,
        })
    }

    pub(super) fn lease(&self) -> &DlpRequestLease {
        &self.lease
    }
    pub(super) fn budget(&self) -> PayloadBudget {
        self.budget.clone()
    }
    #[cfg(test)]
    pub(super) fn lease_mut_for_responses(&mut self) -> &mut DlpRequestLease {
        &mut self.lease
    }
    #[cfg(test)]
    pub(super) fn from_test_lease(lease: DlpRequestLease, budget: PayloadBudget) -> Self {
        Self { lease, budget }
    }

    pub(super) fn prepare_responses_request<T>(
        &mut self,
        body: &serde_json::Value,
        runtime: &SecurityRuntimeConfig,
        restore_rule_ids: &[&str],
        validate: impl FnMut(&serde_json::Value, &[u8], &[&str]) -> Result<T, TokenError>,
    ) -> Result<DlpPreparedRequest<T>, PlanError> {
        super::protocol_adapters::responses::prepare_request(
            &mut self.lease,
            body,
            runtime,
            restore_rule_ids,
            &[],
            self.budget.clone(),
            validate,
        )
    }

    pub(super) fn fork_bound_attempt(
        &self,
        provider: &str,
        protocol: &str,
        target: &str,
        principal: Option<&str>,
        operation: Option<&crate::proxy::forwarder::DlpOperationDescriptor<'_>>,
    ) -> Result<(DlpRequestLease, ActualAttemptBinding), AttemptAuthorityError> {
        let lease = self
            .lease
            .fork_attempt()
            .map_err(|error| AttemptAuthorityError {
                error,
                checkpoint: ContextCheckpoint::AttemptFork,
            })?;
        let binding = lease
            .bind_actual_operation_attempt(provider, protocol, target, principal, operation)
            .map_err(|error| AttemptAuthorityError {
                error,
                checkpoint: ContextCheckpoint::AttemptBinding,
            })?;
        Ok((lease, binding))
    }
}

/// Correlates original observations to one actual attempt; grants no mapping authority.
#[derive(Clone)]
pub(super) struct AttemptProvenance(std::sync::Arc<super::payload_budget::Reservation>);

impl AttemptProvenance {
    pub(super) fn new(budget: &PayloadBudget) -> Result<Self, TokenError> {
        #[cfg(test)]
        if provenance_fault::rejects() {
            return Err(TokenError::CapacityExceeded);
        }
        let charge = std::mem::size_of::<super::payload_budget::Reservation>()
            .checked_add(2 * std::mem::size_of::<usize>())
            .ok_or(TokenError::CapacityExceeded)?;
        Ok(Self(std::sync::Arc::new(budget.reserve(charge)?)))
    }

    pub(super) fn matches(&self, other: &Self) -> bool {
        std::sync::Arc::ptr_eq(&self.0, &other.0)
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum OriginalSseProtocol {
    Chat,
    Anthropic,
    Xai,
}

/// Expected provenance is retained even when an attempt has no reversible mappings.
pub(super) struct SseEvidenceRequirement {
    attempt: AttemptProvenance,
    source: Option<OriginalSseProtocol>,
}

impl SseEvidenceRequirement {
    pub(super) fn new(
        attempt: AttemptProvenance,
        protocol: Option<super::completion_json::JsonCompletionProtocol>,
        mode: super::response_restoration::ArgumentNormalizationMode,
    ) -> Self {
        use super::{
            completion_json::JsonCompletionProtocol as P,
            response_restoration::ArgumentNormalizationMode as M,
        };
        let source = match protocol {
            Some(P::ChatCompletions) => Some(OriginalSseProtocol::Chat),
            Some(P::AnthropicMessages) => Some(OriginalSseProtocol::Anthropic),
            Some(P::NativeResponses) if mode == M::XaiWholeFloat => Some(OriginalSseProtocol::Xai),
            _ => None,
        };
        Self { attempt, source }
    }

    pub(super) fn attempt(&self) -> &AttemptProvenance {
        &self.attempt
    }
    pub(super) fn validate(
        &self,
        observed: Option<(&AttemptProvenance, OriginalSseProtocol)>,
    ) -> Result<(), super::response_restoration::RestoreError> {
        match (self.source, observed) {
            (None, None) => Ok(()),
            (Some(expected), Some((attempt, source)))
                if expected == source && self.attempt.matches(attempt) =>
            {
                Ok(())
            }
            _ => Err(super::response_restoration::RestoreError::RestoreFailed),
        }
    }
}

#[cfg(test)]
pub(super) mod provenance_fault {
    thread_local! {
        static REJECT: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    }
    pub(super) fn rejects() -> bool {
        REJECT.with(std::cell::Cell::get)
    }
    pub(crate) struct Guard(bool);
    pub(crate) fn reject() -> Guard {
        Guard(REJECT.with(|flag| flag.replace(true)))
    }
    impl Drop for Guard {
        fn drop(&mut self) {
            REJECT.with(|flag| flag.set(self.0));
        }
    }
}

/// Charged decoded text state, independent of event names and native indices.
/// Protocol adapters retain their own lifecycle rules and immutable-unit checks.
pub(super) struct StreamTextState {
    pub(super) saw_deltas: bool,
    pub(super) raw_deltas: super::payload_buffer::PayloadBuffer,
    pub(super) restored_deltas: super::payload_buffer::PayloadBuffer,
    pub(super) pending_tail: super::payload_buffer::PayloadBuffer,
    pub(super) raw_done: Option<super::payload_text::PayloadText>,
    pub(super) restored_done: Option<super::payload_text::PayloadText>,
    pub(super) released_deltas_len: usize,
}

impl StreamTextState {
    pub(super) fn new(
        budget: PayloadBudget,
        limit: usize,
    ) -> Result<Self, super::response_restoration::RestoreError> {
        use super::{payload_buffer::PayloadBuffer, response_restoration::RestoreError};
        Ok(Self {
            saw_deltas: false,
            raw_deltas: PayloadBuffer::new(budget.clone(), limit)
                .map_err(RestoreError::scan_error)?,
            restored_deltas: PayloadBuffer::new(budget.clone(), limit)
                .map_err(RestoreError::scan_error)?,
            pending_tail: PayloadBuffer::new(budget, 64).map_err(RestoreError::scan_error)?,
            raw_done: None,
            restored_done: None,
            released_deltas_len: 0,
        })
    }

    pub(super) fn observe_delta(
        &mut self,
        delta: &str,
        budget: PayloadBudget,
        limit: usize,
    ) -> Result<
        (super::payload_buffer::PayloadBuffer, usize),
        super::response_restoration::RestoreError,
    > {
        use super::{payload_buffer::PayloadBuffer, response_restoration::RestoreError};
        if self.raw_done.is_some() {
            return Err(RestoreError::RestoreFailed);
        }
        let length = self
            .pending_tail
            .len()
            .checked_add(delta.len())
            .ok_or(RestoreError::RestoreFailed)?;
        let mut combined = PayloadBuffer::with_capacity(length, budget, limit)
            .map_err(RestoreError::scan_error)?;
        combined
            .append(self.pending_tail.as_slice())
            .and_then(|_| combined.append(delta.as_bytes()))
            .map_err(RestoreError::scan_error)?;
        self.raw_deltas
            .append(delta.as_bytes())
            .map_err(RestoreError::scan_error)?;
        self.saw_deltas = true;
        Ok((combined, self.pending_tail.len()))
    }

    pub(super) fn commit_delta(
        &mut self,
        tail: &str,
        restored: &str,
        budget: PayloadBudget,
    ) -> Result<(), super::response_restoration::RestoreError> {
        use super::{payload_buffer::PayloadBuffer, response_restoration::RestoreError};
        let mut next_tail = PayloadBuffer::with_capacity(tail.len(), budget, 64)
            .map_err(RestoreError::scan_error)?;
        next_tail
            .append(tail.as_bytes())
            .map_err(RestoreError::scan_error)?;
        self.restored_deltas
            .append(restored.as_bytes())
            .map_err(RestoreError::scan_error)?;
        self.pending_tail = next_tail;
        Ok(())
    }

    pub(super) fn flush_tail(&mut self) -> Result<(), super::response_restoration::RestoreError> {
        if !self.pending_tail.as_slice().is_empty() {
            self.restored_deltas
                .append(self.pending_tail.as_slice())
                .map_err(super::response_restoration::RestoreError::capacity)?;
            self.pending_tail.clear();
        }
        Ok(())
    }
}

/// Budgeted unreleased native units; holding is not completion or publication authority.
/// The adapter owns each native shape and the sealed finalizer owns delivery.
pub(super) struct ToolBarrier<T> {
    active: bool,
    bytes: usize,
    units: super::payload_metadata::PayloadVec<T>,
    max_bytes: usize,
    max_units: usize,
    budget: PayloadBudget,
    deferred_owner: Option<std::sync::Arc<super::payload_budget::Reservation>>,
}

/// Opaque deferred units minted by one barrier, with its limits and budget lineage.
pub(super) struct DeferredUnits<T> {
    units: super::payload_metadata::PayloadVec<T>,
    bytes: usize,
    max_bytes: usize,
    max_units: usize,
    owner: std::sync::Arc<super::payload_budget::Reservation>,
}

impl<T> DeferredUnits<T> {
    pub(super) fn push(&mut self, unit: T, actual_bytes: usize) -> Result<(), TokenError> {
        let next_bytes = self
            .bytes
            .checked_add(actual_bytes)
            .filter(|bytes| *bytes <= self.max_bytes)
            .ok_or(TokenError::CapacityExceeded)?;
        if self.units.len() >= self.max_units {
            return Err(TokenError::CapacityExceeded);
        }
        self.units.push(unit)?;
        self.bytes = next_bytes;
        Ok(())
    }
    pub(super) fn units(&self) -> &[T] {
        self.units.as_slice()
    }
    pub(super) fn len(&self) -> usize {
        self.units.len()
    }
    pub(super) fn retained_bytes(&self) -> usize {
        self.bytes
    }
    pub(super) fn clear(&mut self) {
        // Dropping units releases their payloads, not the metadata backing or identity.
        while self.units.pop().is_some() {}
        self.bytes = 0;
    }
    #[cfg(test)]
    pub(super) fn push_unchecked_for_test(
        &mut self,
        unit: T,
        actual_bytes: usize,
    ) -> Result<(), TokenError> {
        // Malformed bounds fixtures must reach the real checked transfer.
        let next_bytes = self
            .bytes
            .checked_add(actual_bytes)
            .ok_or(TokenError::CapacityExceeded)?;
        self.units.push(unit)?;
        self.bytes = next_bytes;
        Ok(())
    }
}

impl<T> ToolBarrier<T> {
    pub(super) fn new(
        budget: PayloadBudget,
        max_bytes: usize,
        max_units: usize,
    ) -> Result<Self, TokenError> {
        Ok(Self {
            active: false,
            bytes: 0,
            units: super::payload_metadata::PayloadVec::new(budget.clone())?,
            max_bytes,
            max_units,
            budget,
            deferred_owner: None,
        })
    }
    pub(super) fn is_active(&self) -> bool {
        self.active
    }
    pub(super) fn hold_all(&mut self) {
        self.active = true;
    }
    pub(super) fn units(&self) -> &[T] {
        self.units.as_slice()
    }
    pub(super) fn len(&self) -> usize {
        self.units.len()
    }
    pub(super) fn hold(&mut self, unit: T, bytes: usize) -> Result<(), TokenError> {
        let next_bytes = self
            .bytes
            .checked_add(bytes)
            .filter(|bytes| *bytes <= self.max_bytes)
            .ok_or(TokenError::CapacityExceeded)?;
        if self.units.len() >= self.max_units {
            return Err(TokenError::CapacityExceeded);
        }
        self.units.push(unit)?;
        self.bytes = next_bytes;
        self.active = true;
        Ok(())
    }
    pub(super) fn deferred_owner(&mut self) -> Result<DeferredUnits<T>, TokenError> {
        let units = super::payload_metadata::PayloadVec::new(self.budget.clone())?;
        let owner = match self.deferred_owner.as_ref() {
            Some(owner) => std::sync::Arc::clone(owner),
            None => {
                // Charge the identity only when the first deferred owner is minted.
                let charge = std::mem::size_of::<super::payload_budget::Reservation>()
                    .checked_add(2 * std::mem::size_of::<usize>())
                    .ok_or(TokenError::CapacityExceeded)?;
                std::sync::Arc::new(self.budget.reserve(charge)?)
            }
        };
        self.deferred_owner = Some(std::sync::Arc::clone(&owner));
        Ok(DeferredUnits {
            units,
            bytes: 0,
            max_bytes: self.max_bytes,
            max_units: self.max_units,
            owner,
        })
    }
    pub(super) fn adopt_deferred(
        &mut self,
        deferred: &mut DeferredUnits<T>,
    ) -> Result<(), TokenError> {
        let matching_owner = self
            .deferred_owner
            .as_ref()
            .is_some_and(|owner| std::sync::Arc::ptr_eq(owner, &deferred.owner));
        if !matching_owner
            || !self.units.as_slice().is_empty()
            || self.bytes != 0
            || deferred.max_bytes != self.max_bytes
            || deferred.max_units != self.max_units
            || deferred.units.len() > self.max_units
            || deferred.bytes > self.max_bytes
        {
            return Err(TokenError::CapacityExceeded);
        }
        std::mem::swap(&mut self.units, &mut deferred.units);
        self.bytes = deferred.bytes;
        deferred.bytes = 0;
        Ok(())
    }
    #[cfg(test)]
    pub(super) fn retained_bytes(&self) -> usize {
        self.bytes
    }
}

#[cfg(test)]
#[path = "tests/c3_deferred_units_tests.rs"]
mod c3_deferred_units_tests;

impl StreamTextState {
    pub(super) fn validate_done(
        &self,
        raw: &str,
    ) -> Result<(), super::response_restoration::RestoreError> {
        use super::response_restoration::RestoreError;
        if self.raw_done.is_some()
            || !self.pending_tail.as_slice().is_empty()
            || (self.saw_deltas && self.raw_deltas.as_slice() != raw.as_bytes())
        {
            return Err(RestoreError::RestoreFailed);
        }
        Ok(())
    }

    pub(super) fn record_done(
        &mut self,
        raw: &str,
        restored: &str,
        budget: PayloadBudget,
        limit: usize,
    ) -> Result<bool, super::response_restoration::RestoreError> {
        use super::{payload_text::PayloadText, response_restoration::RestoreError};
        if self.saw_deltas && self.restored_deltas.as_slice() != restored.as_bytes() {
            return Err(RestoreError::RestoreFailed);
        }
        let raw_done = PayloadText::try_from_str(raw, budget.clone(), limit)
            .map_err(RestoreError::scan_error)?;
        let restored_done =
            PayloadText::try_from_str(restored, budget, limit).map_err(RestoreError::scan_error)?;
        self.raw_done = Some(raw_done);
        self.restored_done = Some(restored_done);
        Ok(self.saw_deltas)
    }
}

#[path = "restoration_core/lifecycle.rs"]
mod lifecycle;
pub(super) use lifecycle::{FinishedInspection, LifecycleLimits, ResponseLifecycle, TextStreamId};

#[cfg(test)]
pub(super) use lifecycle::LifecycleOperation;
