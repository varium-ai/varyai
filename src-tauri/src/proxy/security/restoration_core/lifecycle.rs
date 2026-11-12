//! Shared C3 lifecycle ownership with staged text and decoded-argument transactions.
//! No protocol parser, completion issuer, delivery witness, publisher or history-resume API.
use std::sync::Arc;

use super::{restore_text, AttemptProvenance, StreamTextState, ToolBarrier};
use crate::proxy::security::{
    completed_context::ActualAttemptBinding,
    payload_budget::{PayloadBudget, Reservation},
    payload_buffer::PayloadBuffer,
    payload_metadata::PayloadVec,
    payload_text::PayloadText,
    response_restoration::{restore_json_arguments_string, RestoreError},
    restoration_contract::{
        OriginalResponseObservation, ProtocolPair, RestoreDecision, TextPolicy,
    },
    tokenization::{DlpRequestLease, TokenError},
};

/// Test-only fault selection; never request data, evidence or publication authority.
#[cfg(test)]
#[derive(Clone, Copy, PartialEq, Eq)]
pub(in crate::proxy::security) enum LifecycleOperation {
    Begin,
    OpenText,
    Delta,
    Done,
    ObserveOriginal,
    Hold,
    Finish,
}

#[cfg(test)]
thread_local! {
    static REJECT_OPERATION: std::cell::Cell<Option<LifecycleOperation>> = const {
        std::cell::Cell::new(None)
    };
    static BEGIN_COUNT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Restores the previous thread-local selection; cannot cross a thread boundary.
#[cfg(test)]
pub(in crate::proxy::security) struct LifecycleRejectionGuard {
    previous: Option<LifecycleOperation>,
    _thread: std::marker::PhantomData<std::rc::Rc<()>>,
}

#[cfg(test)]
impl Drop for LifecycleRejectionGuard {
    fn drop(&mut self) {
        REJECT_OPERATION.with(|operation| operation.set(self.previous));
    }
}

#[cfg(test)]
fn rejects(operation: LifecycleOperation) -> bool {
    REJECT_OPERATION.with(|selected| selected.get() == Some(operation))
}

#[derive(Clone, Copy)]
pub(in crate::proxy::security) struct LifecycleLimits {
    pub(in crate::proxy::security) max_text_bytes: usize,
    pub(in crate::proxy::security) max_text_streams: usize,
    pub(in crate::proxy::security) max_held_bytes: usize,
    pub(in crate::proxy::security) max_held_units: usize,
}

/// An owner-bound ordinal, not a native index or a reusable restoration capability.
/// Identity allocation stays charged while either the lifecycle or an ID retains it.
pub(in crate::proxy::security) struct TextStreamId {
    owner: Arc<Reservation>,
    ordinal: usize,
}

struct LifecycleTextState {
    policy: TextPolicy,
    state: StreamTextState,
    terminal: bool,
}

/// Private fields intentionally bind all authority and mechanics to one non-Clone owner.
/// Adapters retain native indices, item kinds, signatures and protocol terminal rules.
pub(in crate::proxy::security) struct ResponseLifecycle<T = PayloadBuffer> {
    lease: DlpRequestLease,
    binding: ActualAttemptBinding,
    budget: PayloadBudget,
    pair: ProtocolPair,
    attempt: AttemptProvenance,
    identity: Arc<Reservation>,
    limits: LifecycleLimits,
    texts: PayloadVec<LifecycleTextState>,
    barrier: ToolBarrier<T>,
    dependencies: PayloadVec<PayloadText>,
    occurrences: usize,
    held_bytes: usize,
    failure: Option<RestoreError>,
}

/// Consumed mechanics retained for inspection; never completion or release authority.
/// There is deliberately no method returning a proof, spec, lease, binding or publisher.
pub(in crate::proxy::security) struct FinishedInspection<T = PayloadBuffer> {
    owner: ResponseLifecycle<T>,
}

impl ResponseLifecycle {
    #[cfg(test)]
    pub(in crate::proxy::security) fn begin_count_for_test() -> usize {
        BEGIN_COUNT.with(std::cell::Cell::get)
    }

    #[cfg(test)]
    pub(in crate::proxy::security) fn reject_operation_for_test(
        operation: LifecycleOperation,
    ) -> LifecycleRejectionGuard {
        LifecycleRejectionGuard {
            previous: REJECT_OPERATION.with(|selected| selected.replace(Some(operation))),
            _thread: std::marker::PhantomData,
        }
    }

    pub(in crate::proxy::security) fn begin(
        lease: DlpRequestLease,
        binding: ActualAttemptBinding,
        budget: PayloadBudget,
        pair: ProtocolPair,
        attempt: AttemptProvenance,
        limits: LifecycleLimits,
    ) -> Result<Self, RestoreError> {
        Self::begin_native(lease, binding, budget, pair, attempt, limits)
    }

    /// Derive the byte count from the exact charged native buffer owner.
    pub(in crate::proxy::security) fn hold(
        &mut self,
        unit: PayloadBuffer,
    ) -> Result<(), RestoreError> {
        let bytes = unit.len();
        self.hold_native(unit, bytes)
    }
}

impl<T> ResponseLifecycle<T> {
    pub(in crate::proxy::security) fn begin_native(
        lease: DlpRequestLease,
        binding: ActualAttemptBinding,
        budget: PayloadBudget,
        pair: ProtocolPair,
        attempt: AttemptProvenance,
        limits: LifecycleLimits,
    ) -> Result<Self, RestoreError> {
        #[cfg(test)]
        if rejects(LifecycleOperation::Begin) {
            return Err(RestoreError::RestoreFailed);
        }
        // Validate current authority only, with protocol-specific namespace separation.
        // Optional completed-history eligibility is unrelated to current restoration.
        lease
            .require_current_attempt_binding(pair.client(), &binding)
            .map_err(RestoreError::scan_error)?;
        let charge = std::mem::size_of::<Reservation>()
            .checked_add(2 * std::mem::size_of::<usize>())
            .ok_or_else(|| RestoreError::capacity(TokenError::CapacityExceeded))?;
        let identity = Arc::new(budget.reserve(charge).map_err(RestoreError::capacity)?);
        let texts = PayloadVec::new(budget.clone()).map_err(RestoreError::capacity)?;
        let barrier =
            ToolBarrier::new(budget.clone(), limits.max_held_bytes, limits.max_held_units)
                .map_err(RestoreError::capacity)?;
        let dependencies = PayloadVec::new(budget.clone()).map_err(RestoreError::capacity)?;
        #[cfg(test)]
        BEGIN_COUNT.with(|count| count.set(count.get() + 1));
        Ok(Self {
            lease,
            binding,
            budget,
            pair,
            attempt,
            identity,
            limits,
            texts,
            barrier,
            dependencies,
            occurrences: 0,
            held_bytes: 0,
            failure: None,
        })
    }

    /// Fix the text policy before any output or dependency can be committed.
    pub(in crate::proxy::security) fn open_text(
        &mut self,
        policy: TextPolicy,
    ) -> Result<TextStreamId, RestoreError> {
        if let Some(error) = self.failure {
            return Err(error);
        }
        #[cfg(test)]
        if rejects(LifecycleOperation::OpenText) {
            return Err(self.record_failure(RestoreError::RestoreFailed));
        }
        let ordinal = self.texts.len();
        if ordinal >= self.limits.max_text_streams {
            return Err(self.record_failure(RestoreError::capacity(TokenError::CapacityExceeded)));
        }
        let state = StreamTextState::new(self.budget.clone(), self.limits.max_text_bytes)
            .map_err(|error| self.record_failure(error))?;
        self.texts
            .push(LifecycleTextState {
                policy,
                state,
                terminal: false,
            })
            .map_err(|error| self.record_failure(RestoreError::capacity(error)))?;
        // The charged vector owns the state; the ID shares only this owner's identity.
        Ok(TextStreamId {
            owner: Arc::clone(&self.identity),
            ordinal,
        })
    }

    /// Decoded deltas only; framing and arbitrary TCP-byte decoding belong to adapters.
    pub(in crate::proxy::security) fn delta(
        &mut self,
        id: &TextStreamId,
        text: &str,
    ) -> Result<RestoreDecision, RestoreError> {
        if let Some(error) = self.failure {
            return Err(error);
        }
        #[cfg(test)]
        if rejects(LifecycleOperation::Delta) {
            return Err(self.record_failure(RestoreError::RestoreFailed));
        }
        let staged = (|| {
            let (policy, mut state) = self.stage_text(id)?;
            let (combined, _) =
                state.observe_delta(text, self.budget.clone(), self.limits.max_text_bytes)?;
            let combined = std::str::from_utf8(combined.as_slice())
                .map_err(|_| RestoreError::RestoreFailed)?;
            let (prefix, tail) = if policy == TextPolicy::Reasoning {
                (combined, "")
            } else {
                crate::proxy::security::output_text_matching::split_prefix_and_tail(combined)
            };
            let mut dependencies = self.stage_dependencies()?;
            let mut occurrences = self.occurrences;
            let restored = self.restore(policy, prefix, &mut dependencies, &mut occurrences)?;
            state.commit_delta(tail, restored.as_str(), self.budget.clone())?;
            let decision = if restored.as_str() == text {
                RestoreDecision::Unchanged
            } else if restored.as_str().is_empty() {
                RestoreDecision::Hold
            } else {
                RestoreDecision::Patch(restored)
            };
            Ok((state, dependencies, occurrences, decision))
        })();
        match staged {
            Ok((state, dependencies, occurrences, decision)) => {
                // All reservations, copies and validation precede the sole owner commit.
                let Some(target) = self.texts.get_mut(id.ordinal) else {
                    return Err(self.record_failure(RestoreError::RestoreFailed));
                };
                target.state = state;
                self.dependencies = dependencies;
                self.occurrences = occurrences;
                Ok(decision)
            }
            Err(error) => Err(self.record_failure(error)),
        }
    }

    /// Reconcile a full snapshot without counting already restored deltas twice.
    pub(in crate::proxy::security) fn done(
        &mut self,
        id: &TextStreamId,
        text: &str,
    ) -> Result<RestoreDecision, RestoreError> {
        if let Some(error) = self.failure {
            return Err(error);
        }
        #[cfg(test)]
        if rejects(LifecycleOperation::Done) {
            return Err(self.record_failure(RestoreError::RestoreFailed));
        }
        let staged = (|| {
            let (policy, mut state) = self.stage_text(id)?;
            // An incomplete reference at the end is literal, but remains private until done.
            state.flush_tail()?;
            state.validate_done(text)?;
            let mut dependencies = self.stage_dependencies()?;
            let mut occurrences = if state.saw_deltas {
                0
            } else {
                self.occurrences
            };
            let restored = self.restore(policy, text, &mut dependencies, &mut occurrences)?;
            let saw_deltas = state.record_done(
                text,
                restored.as_str(),
                self.budget.clone(),
                self.limits.max_text_bytes,
            )?;
            if saw_deltas {
                occurrences = self.occurrences;
            }
            let decision = if restored.as_str() == text {
                RestoreDecision::Unchanged
            } else {
                RestoreDecision::Patch(restored)
            };
            Ok((state, dependencies, occurrences, decision))
        })();
        match staged {
            Ok((state, dependencies, occurrences, decision)) => {
                let Some(target) = self.texts.get_mut(id.ordinal) else {
                    return Err(self.record_failure(RestoreError::RestoreFailed));
                };
                target.state = state;
                target.terminal = true;
                self.dependencies = dependencies;
                self.occurrences = occurrences;
                Ok(decision)
            }
            Err(error) => Err(self.record_failure(error)),
        }
    }

    /// Reconcile an open stream without asserting a protocol terminal.
    pub(in crate::proxy::security) fn snapshot(
        &mut self,
        id: &TextStreamId,
        text: &str,
    ) -> Result<RestoreDecision, RestoreError> {
        if let Some(error) = self.failure {
            return Err(error);
        }
        let staged = (|| {
            let (policy, mut state) = self.stage_text(id)?;
            if state.saw_deltas && state.raw_deltas.as_slice() != text.as_bytes() {
                return Err(RestoreError::RestoreFailed);
            }
            let mut dependencies = self.stage_dependencies()?;
            let mut occurrences = if state.saw_deltas {
                0
            } else {
                self.occurrences
            };
            let restored = self.restore(policy, text, &mut dependencies, &mut occurrences)?;
            if state.saw_deltas {
                // Snapshot caches must agree with the stream's own emitted prefix and tail.
                let prefix_len = state.restored_deltas.len();
                if restored.as_str().as_bytes().get(..prefix_len)
                    != Some(state.restored_deltas.as_slice())
                    || restored.as_str().as_bytes().get(prefix_len..)
                        != Some(state.pending_tail.as_slice())
                {
                    return Err(RestoreError::RestoreFailed);
                }
                occurrences = self.occurrences;
            } else {
                let tail = if policy == TextPolicy::Reasoning {
                    ""
                } else {
                    crate::proxy::security::output_text_matching::split_prefix_and_tail(text).1
                };
                let prefix_len = restored
                    .as_str()
                    .len()
                    .checked_sub(tail.len())
                    .ok_or(RestoreError::RestoreFailed)?;
                let prefix = restored
                    .as_str()
                    .get(..prefix_len)
                    .ok_or(RestoreError::RestoreFailed)?;
                state
                    .raw_deltas
                    .append(text.as_bytes())
                    .map_err(RestoreError::capacity)?;
                state.commit_delta(tail, prefix, self.budget.clone())?;
                // The first snapshot is an open accumulated baseline, not a terminal.
                state.saw_deltas = true;
            }
            state.raw_done = Some(
                PayloadText::try_from_str(text, self.budget.clone(), self.limits.max_text_bytes)
                    .map_err(RestoreError::capacity)?,
            );
            state.restored_done = Some(
                PayloadText::try_from_str(
                    restored.as_str(),
                    self.budget.clone(),
                    self.limits.max_text_bytes,
                )
                .map_err(RestoreError::capacity)?,
            );
            let decision = if restored.as_str() == text {
                RestoreDecision::Unchanged
            } else {
                RestoreDecision::Patch(restored)
            };
            Ok((state, dependencies, occurrences, decision))
        })();
        match staged {
            Ok((state, dependencies, occurrences, decision)) => {
                let Some(target) = self.texts.get_mut(id.ordinal) else {
                    return Err(self.record_failure(RestoreError::RestoreFailed));
                };
                target.state = state;
                self.dependencies = dependencies;
                self.occurrences = occurrences;
                Ok(decision)
            }
            Err(error) => Err(self.record_failure(error)),
        }
    }

    /// Restore decoded object argument string leaves as one ledger transaction.
    pub(in crate::proxy::security) fn arguments(
        &mut self,
        text: &str,
    ) -> Result<PayloadText, RestoreError> {
        if let Some(error) = self.failure {
            return Err(error);
        }
        let staged = (|| {
            let mut dependencies = self.stage_dependencies()?;
            let mut occurrences = self.occurrences;
            // The strict helper owns parsing, decoded leaves, rebuild and final encoding.
            // Its mutable ledger is staged here; even a final-copy fault cannot commit it.
            let restored = restore_json_arguments_string(
                text,
                &self.lease,
                &self.budget,
                &mut dependencies,
                &mut occurrences,
            )?;
            Ok((dependencies, occurrences, restored))
        })();
        match staged {
            Ok((dependencies, occurrences, restored)) => {
                self.dependencies = dependencies;
                self.occurrences = occurrences;
                Ok(restored)
            }
            Err(error) => Err(self.record_failure(error)),
        }
    }

    /// Inspect borrowed facts for the exact attempt and actual upstream, even at zero maps.
    /// Observations never attest EOF, protocol completion, conversion or client delivery.
    pub(in crate::proxy::security) fn observe_original(
        &mut self,
        observation: &OriginalResponseObservation<'_>,
    ) -> Result<(), RestoreError> {
        if let Some(error) = self.failure {
            return Err(error);
        }
        #[cfg(test)]
        if rejects(LifecycleOperation::ObserveOriginal) {
            return Err(self.record_failure(RestoreError::RestoreFailed));
        }
        if self
            .lease
            .require_current_attempt_binding(self.pair.client(), &self.binding)
            .is_err()
            || observation.upstream() != self.pair.upstream()
            || !self.attempt.matches(observation.attempt())
        {
            return Err(self.record_failure(RestoreError::RestoreFailed));
        }
        Ok(())
    }

    /// Retain a charged native owner using the adapter's measured serialized size.
    pub(in crate::proxy::security) fn hold_native(
        &mut self,
        unit: T,
        actual_bytes: usize,
    ) -> Result<(), RestoreError> {
        if let Some(error) = self.failure {
            return Err(error);
        }
        #[cfg(test)]
        if rejects(LifecycleOperation::Hold) {
            return Err(self.record_failure(RestoreError::RestoreFailed));
        }
        let next_bytes = self
            .held_bytes
            .checked_add(actual_bytes)
            .filter(|total| *total <= self.limits.max_held_bytes)
            .ok_or_else(|| {
                self.record_failure(RestoreError::capacity(TokenError::CapacityExceeded))
            })?;
        if self.barrier.len() >= self.limits.max_held_units {
            return Err(self.record_failure(RestoreError::capacity(TokenError::CapacityExceeded)));
        }
        // The barrier checks its limits and acquires metadata before adopting the owner.
        self.barrier
            .hold(unit, actual_bytes)
            .map_err(|error| self.record_failure(RestoreError::capacity(error)))?;
        self.held_bytes = next_bytes;
        Ok(())
    }

    pub(in crate::proxy::security) fn barrier_is_active(&self) -> bool {
        self.barrier.is_active()
    }

    pub(in crate::proxy::security) fn hold_all(&mut self) -> Result<(), RestoreError> {
        if let Some(error) = self.failure {
            return Err(error);
        }
        self.barrier.hold_all();
        Ok(())
    }

    pub(in crate::proxy::security) fn deferred_owner(
        &mut self,
    ) -> Result<super::DeferredUnits<T>, RestoreError> {
        if let Some(error) = self.failure {
            return Err(error);
        }
        self.barrier
            .deferred_owner()
            .map_err(|error| self.record_failure(RestoreError::capacity(error)))
    }

    pub(in crate::proxy::security) fn adopt_deferred(
        &mut self,
        deferred: &mut super::DeferredUnits<T>,
    ) -> Result<(), RestoreError> {
        if let Some(error) = self.failure {
            return Err(error);
        }
        let bytes = deferred.retained_bytes();
        // The barrier validates lineage and bounds before either owner can be modified.
        self.barrier
            .adopt_deferred(deferred)
            .map_err(|error| self.record_failure(RestoreError::capacity(error)))?;
        self.held_bytes = bytes;
        Ok(())
    }

    /// Borrow stream mechanics without handing out mutable restoration authority.
    pub(in crate::proxy::security) fn text_state(
        &self,
        id: &TextStreamId,
    ) -> Result<&StreamTextState, RestoreError> {
        if !Arc::ptr_eq(&self.identity, &id.owner) {
            return Err(RestoreError::RestoreFailed);
        }
        self.texts
            .as_slice()
            .get(id.ordinal)
            .map(|text| &text.state)
            .ok_or(RestoreError::RestoreFailed)
    }

    pub(in crate::proxy::security) fn mark_text_released(
        &mut self,
        id: &TextStreamId,
    ) -> Result<(), RestoreError> {
        if let Some(error) = self.failure {
            return Err(error);
        }
        let length = match self.text_state(id) {
            Ok(state) => state.restored_deltas.len(),
            Err(error) => return Err(self.record_failure(error)),
        };
        let Some(target) = self.texts.get_mut(id.ordinal) else {
            return Err(self.record_failure(RestoreError::RestoreFailed));
        };
        target.state.released_deltas_len = length;
        Ok(())
    }

    pub(in crate::proxy::security) fn flush_text_tail(
        &mut self,
        id: &TextStreamId,
    ) -> Result<(), RestoreError> {
        if let Some(error) = self.failure {
            return Err(error);
        }
        let staged = (|| {
            let (_, mut state) = self.stage_text(id)?;
            state.flush_tail()?;
            Ok(state)
        })();
        match staged {
            Ok(state) => {
                let Some(target) = self.texts.get_mut(id.ordinal) else {
                    return Err(self.record_failure(RestoreError::RestoreFailed));
                };
                target.state.restored_deltas = state.restored_deltas;
                target.state.pending_tail = state.pending_tail;
                Ok(())
            }
            Err(error) => Err(self.record_failure(error)),
        }
    }

    pub(in crate::proxy::security) fn dependency_tokens(&self) -> &[PayloadText] {
        self.dependencies.as_slice()
    }

    pub(in crate::proxy::security) fn restored_occurrences(&self) -> usize {
        self.occurrences
    }

    pub(in crate::proxy::security) fn held_units(&self) -> &[T] {
        self.barrier.units()
    }

    pub(in crate::proxy::security) fn held_bytes(&self) -> usize {
        self.held_bytes
    }

    pub(in crate::proxy::security) fn failure(&self) -> Option<RestoreError> {
        self.failure
    }

    /// Consume mechanics into inspection ownership, never into a completion claim.
    pub(in crate::proxy::security) fn finish(self) -> Result<FinishedInspection<T>, RestoreError> {
        #[cfg(test)]
        let mut owner = self;
        #[cfg(not(test))]
        let owner = self;
        #[cfg(test)]
        if rejects(LifecycleOperation::Finish) {
            return Err(owner.record_failure(RestoreError::RestoreFailed));
        }
        if let Some(error) = owner.failure {
            return Err(error);
        }
        if owner.texts.as_slice().iter().any(|text| !text.terminal) {
            return Err(RestoreError::RestoreFailed);
        }
        Ok(FinishedInspection { owner })
    }

    /// Consume the owner and release its budget obligations, including after sticky failure.
    pub(in crate::proxy::security) fn abort(self) -> Result<(), RestoreError> {
        drop(self);
        Ok(())
    }

    /// Keep every old owner charged and untouched until the staged operation succeeds.
    fn stage_text(&self, id: &TextStreamId) -> Result<(TextPolicy, StreamTextState), RestoreError> {
        if !Arc::ptr_eq(&self.identity, &id.owner) {
            return Err(RestoreError::RestoreFailed);
        }
        let text = self
            .texts
            .as_slice()
            .get(id.ordinal)
            .ok_or(RestoreError::RestoreFailed)?;
        let state = &text.state;
        if text.terminal {
            return Err(RestoreError::RestoreFailed);
        }
        Ok((
            text.policy,
            StreamTextState {
                saw_deltas: state.saw_deltas,
                raw_deltas: state
                    .raw_deltas
                    .try_clone()
                    .map_err(RestoreError::capacity)?,
                restored_deltas: state
                    .restored_deltas
                    .try_clone()
                    .map_err(RestoreError::capacity)?,
                pending_tail: state
                    .pending_tail
                    .try_clone()
                    .map_err(RestoreError::capacity)?,
                raw_done: None,
                restored_done: None,
                released_deltas_len: state.released_deltas_len,
            },
        ))
    }

    fn stage_dependencies(&self) -> Result<PayloadVec<PayloadText>, RestoreError> {
        let mut staged = PayloadVec::new(self.budget.clone()).map_err(RestoreError::capacity)?;
        for reference in self.dependencies.as_slice() {
            let owned = PayloadText::try_from_str(
                reference.as_str(),
                self.budget.clone(),
                crate::proxy::security::token_format::MAX_REFERENCE_BYTES,
            )
            .map_err(RestoreError::capacity)?;
            staged.push(owned).map_err(RestoreError::capacity)?;
        }
        Ok(staged)
    }

    fn restore(
        &self,
        policy: TextPolicy,
        text: &str,
        dependencies: &mut PayloadVec<PayloadText>,
        occurrences: &mut usize,
    ) -> Result<PayloadText, RestoreError> {
        match policy {
            TextPolicy::Reasoning => {
                PayloadText::try_from_str(text, self.budget.clone(), self.limits.max_text_bytes)
                    .map_err(RestoreError::capacity)
            }
            TextPolicy::Output | TextPolicy::Refusal | TextPolicy::ToolArguments => {
                restore_text(text, &self.lease, &self.budget, dependencies, occurrences)
            }
        }
    }

    fn record_failure(&mut self, error: RestoreError) -> RestoreError {
        *self.failure.get_or_insert(error)
    }
}

impl<T> FinishedInspection<T> {
    pub(in crate::proxy::security) fn attempt(&self) -> &AttemptProvenance {
        &self.owner.attempt
    }

    pub(in crate::proxy::security) fn pair(&self) -> ProtocolPair {
        self.owner.pair
    }

    pub(in crate::proxy::security) fn dependency_tokens(&self) -> &[PayloadText] {
        self.owner.dependency_tokens()
    }

    pub(in crate::proxy::security) fn restored_occurrences(&self) -> usize {
        self.owner.restored_occurrences()
    }

    pub(in crate::proxy::security) fn held_units(&self) -> &[T] {
        self.owner.held_units()
    }

    pub(in crate::proxy::security) fn text_state(
        &self,
        id: &TextStreamId,
    ) -> Result<&StreamTextState, RestoreError> {
        if !Arc::ptr_eq(&self.owner.identity, &id.owner) {
            return Err(RestoreError::RestoreFailed);
        }
        self.owner
            .texts
            .as_slice()
            .get(id.ordinal)
            .map(|text| &text.state)
            .ok_or(RestoreError::RestoreFailed)
    }
}

#[cfg(test)]
#[path = "../tests/c3_lifecycle_transaction_tests.rs"]
mod transaction_tests;
