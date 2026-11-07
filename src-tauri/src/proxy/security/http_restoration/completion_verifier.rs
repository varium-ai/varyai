//! Consuming completion checks owned exclusively by the final HTTP wrapper.
use std::{io, sync::Mutex};

use serde_json::Value;
use sha2::{Digest, Sha256};

use super::super::{
    completed_context::ActualAttemptBinding,
    completion_evidence::{CompletionObservation, CompletionTransport, SseCompletion},
    completion_json::{JsonCompletion, JsonCompletionProtocol, NativeSseCompletion},
    payload_budget::PayloadBudget,
    payload_buffer::PayloadBuffer,
    payload_json::PayloadJson,
    protocol_adapters::responses::{OriginalXaiArguments, RestoreError, SseRestorationState},
    restoration_contract::{ClientProtocol, ProtocolPair},
    restoration_core::{AttemptProvenance, OriginalSseProtocol},
    restore_diagnostics::RestoreStage,
};

struct XaiOriginalEvidence {
    native: NativeSseCompletion,
    arguments: Option<OriginalXaiArguments>,
}

/// The observer shell transfers this exact state, never a summary or clone.
pub(super) struct OriginalEvidence {
    provenance: Option<AttemptProvenance>,
    source: OriginalSseProtocol,
    xai_original: Option<XaiOriginalEvidence>,
    sse: SseCompletion,
    json_fallback: Option<JsonCompletion>,
    observations_seen: bool,
    transport: CompletionTransport,
    budget: PayloadBudget,
}

impl OriginalEvidence {
    pub(super) fn chat(budget: PayloadBudget) -> Self {
        Self {
            provenance: None,
            source: OriginalSseProtocol::Chat,
            xai_original: None,
            sse: SseCompletion::chat(),
            json_fallback: None,
            observations_seen: false,
            transport: CompletionTransport::default(),
            budget,
        }
    }

    pub(super) fn anthropic(budget: PayloadBudget) -> Self {
        Self {
            provenance: None,
            source: OriginalSseProtocol::Anthropic,
            xai_original: None,
            sse: SseCompletion::anthropic(),
            json_fallback: Some(JsonCompletion::new(
                JsonCompletionProtocol::AnthropicMessages,
            )),
            observations_seen: false,
            transport: CompletionTransport::default(),
            budget,
        }
    }

    pub(super) fn xai_original(budget: PayloadBudget) -> Self {
        let mut state = Self::chat(budget);
        state.xai_original = Some(XaiOriginalEvidence {
            native: NativeSseCompletion::new(),
            arguments: None,
        });
        state.source = OriginalSseProtocol::Xai;
        state
    }

    pub(super) fn bind_attempt(&mut self, attempt: AttemptProvenance) {
        self.provenance = Some(attempt);
    }

    pub(super) fn budget(&self) -> PayloadBudget {
        self.budget.clone()
    }

    pub(super) fn abort(&mut self) {
        self.transport.abort();
    }

    pub(super) fn observe_xai_original_frame(&mut self, raw: &str) -> Result<(), RestoreError> {
        let xai = self
            .xai_original
            .as_mut()
            .ok_or(RestoreError::RestoreFailed)?;
        if xai.arguments.is_none() {
            xai.arguments = Some(OriginalXaiArguments::new(self.budget.clone())?);
        }
        let arguments = xai.arguments.as_mut().ok_or(RestoreError::RestoreFailed)?;
        observe_native_block(
            &mut xai.native,
            &mut self.transport,
            Some(arguments),
            raw,
            &self.budget,
        )
    }

    pub(super) fn observe_xai_original_end(
        &mut self,
        unfinished_frame: bool,
        unfinished_utf8: bool,
    ) {
        let end = CompletionObservation::EntityEnd {
            reached_eof: true,
            unfinished_frame,
            unfinished_utf8,
        };
        self.transport.observe(&end);
        if let Some(xai) = &mut self.xai_original {
            xai.native.observe(&end, &self.budget);
        } else {
            self.transport.abort();
        }
    }

    pub(super) fn observe_fully_read_anthropic_json(&mut self, raw: &str) -> Option<PayloadJson> {
        if self.observations_seen || self.xai_original.is_some() || self.json_fallback.is_none() {
            self.transport.abort();
            return None;
        }
        self.observations_seen = true;
        let parsed = self
            .json_fallback
            .as_mut()
            .and_then(|json| json.observe_raw_json_scoped(raw, &self.budget));
        self.transport.observe_fully_read_json();
        parsed
    }

    pub(super) fn observe(&mut self, fact: &CompletionObservation<'_>) {
        self.observations_seen = true;
        if self.xai_original.is_some() {
            self.transport.abort();
            return;
        }
        self.transport.observe(fact);
        match fact {
            CompletionObservation::RawJson(raw) => {
                if let Some(json) = &mut self.json_fallback {
                    json.observe_raw_json(raw, &self.budget);
                } else {
                    self.sse.observe(fact, &self.budget);
                }
            }
            CompletionObservation::AnthropicJson(_) => {
                if self.json_fallback.is_none() {
                    self.sse.observe(fact, &self.budget);
                }
            }
            _ => self.sse.observe(fact, &self.budget),
        }
    }

    pub(super) fn has_unsupported_carrier(&self) -> bool {
        self.xai_original.is_none()
            && (self.sse.has_unsupported_carrier()
                || self
                    .json_fallback
                    .as_ref()
                    .is_some_and(JsonCompletion::has_unsupported_carrier))
    }

    pub(super) fn completion_result(&self) -> Result<(), RestoreError> {
        if self.has_unsupported_carrier() {
            return Err(RestoreError::RestoreUnsupported);
        }
        let complete = if let Some(state) = &self.xai_original {
            state.native.permits_completion() && self.transport.permits_completion()
        } else {
            self.transport.permits_completion()
                && (self.sse.permits_completion()
                    || self
                        .json_fallback
                        .as_ref()
                        .is_some_and(JsonCompletion::permits_completion))
        };
        if complete {
            Ok(())
        } else {
            Err(RestoreError::RestoreFailed)
        }
    }
}

pub(super) struct SseCompletionVerifier {
    native: NativeSseCompletion,
    transport: CompletionTransport,
    budget: PayloadBudget,
    attempt: Option<AttemptProvenance>,
    framing: Sha256,
}

impl SseCompletionVerifier {
    pub(super) fn new(state: &SseRestorationState) -> Self {
        Self {
            native: NativeSseCompletion::new(),
            transport: CompletionTransport::default(),
            budget: state.payload_budget(),
            attempt: state.completion_attempt().cloned(),
            framing: Sha256::new(),
        }
    }

    /// Observe the exact final-client input before restoration changes its value.
    pub(super) fn observe_block(&mut self, block: &str) -> Result<(), RestoreError> {
        self.framing.update((block.len() as u64).to_le_bytes());
        self.framing.update(block.as_bytes());
        observe_native_block(
            &mut self.native,
            &mut self.transport,
            None,
            block,
            &self.budget,
        )
    }

    /// Called only from the continuation after the actual source returned None.
    pub(super) fn finish(
        mut self,
        state: &SseRestorationState,
        original: Option<OriginalEvidence>,
    ) -> Result<VerifiedCompletion, (RestoreError, RestoreStage)> {
        let stage = RestoreStage::CompletionEvidence;
        let attempt = self
            .attempt
            .take()
            .ok_or((RestoreError::RestoreFailed, stage))?;
        let end = CompletionObservation::EntityEnd {
            reached_eof: true,
            unfinished_frame: false,
            unfinished_utf8: false,
        };
        self.native.observe(&end, &self.budget);
        self.transport.observe(&end);
        state
            .validate_original_evidence(original.as_ref().and_then(|evidence| {
                evidence
                    .provenance
                    .as_ref()
                    .map(|attempt| (attempt, evidence.source))
            }))
            .map_err(|error| (error, stage))?;
        if let Some(original) = original {
            original
                .completion_result()
                .map_err(|error| (error, stage))?;
            if let Some(xai) = original.xai_original {
                let mut arguments = xai.arguments.ok_or((RestoreError::RestoreFailed, stage))?;
                let output = arguments
                    .take_completed_output()
                    .map_err(|error| (error, stage))?;
                state
                    .verify_original_xai_output(output.as_value())
                    .map_err(|error| (error, RestoreStage::SseSnapshot))?;
            }
        }
        if !self.native.permits_completion() || !self.transport.permits_completion() {
            return Err((RestoreError::RestoreFailed, stage));
        }
        Ok(VerifiedCompletion {
            attempt,
            framing: self.framing.finalize().into(),
        })
    }
}

// Test-only bridge into the real verifier; never constructs proof fields directly.
#[cfg(test)]
impl VerifiedCompletion {
    pub(in crate::proxy::security) fn verify_frames_for_test(
        state: &SseRestorationState,
        blocks: &[String],
        original: Option<&mut super::UpstreamSseEvidenceTracker>,
    ) -> Result<Self, (RestoreError, RestoreStage)> {
        let mut verifier = SseCompletionVerifier::new(state);
        for block in blocks {
            verifier
                .observe_block(block)
                .map_err(|error| (error, RestoreStage::CompletionEvidence))?;
        }
        let original = original
            .map(|tracker| tracker.take())
            .transpose()
            .map_err(|error| (error, RestoreStage::CompletionEvidence))?;
        verifier.finish(state, original)
    }
}

#[cfg(test)]
impl super::UpstreamSseEvidenceTracker {
    pub(in crate::proxy::security) fn bind_for_test(&mut self, state: &SseRestorationState) {
        if let Some(attempt) = state.completion_attempt() {
            self.bind_attempt(attempt.clone());
        }
    }
}

fn observe_native_block(
    native: &mut NativeSseCompletion,
    transport: &mut CompletionTransport,
    mut original_arguments: Option<&mut OriginalXaiArguments>,
    block: &str,
    budget: &PayloadBudget,
) -> Result<(), RestoreError> {
    let raw = CompletionObservation::RawFrame(block);
    native.observe(&raw, budget);
    transport.observe(&raw);
    let mut data =
        PayloadBuffer::new(budget.clone(), 1024 * 1024).map_err(|_| RestoreError::RestoreFailed)?;
    let mut event_name = "";
    let mut count = 0usize;
    for line in block.lines() {
        if let Some(value) = line.strip_prefix("event:") {
            event_name = value.trim();
        } else if let Some(value) = line
            .strip_prefix("data:")
            .or_else(|| (line == "data").then_some(""))
        {
            if count != 0 {
                data.append(b"\n")
                    .map_err(|_| RestoreError::RestoreFailed)?;
            }
            count += 1;
            data.append(value.strip_prefix(' ').unwrap_or(value).as_bytes())
                .map_err(|_| RestoreError::RestoreFailed)?;
        }
    }
    if count != 0 {
        let raw = std::str::from_utf8(data.as_slice()).map_err(|_| RestoreError::RestoreFailed)?;
        if raw.trim() != "[DONE]" {
            let parsed = PayloadJson::try_from_str(raw, budget.clone(), budget.limit())
                .map_err(|_| RestoreError::RestoreFailed)?;
            if let Some(arguments) = original_arguments.as_mut() {
                arguments.observe(parsed.as_value())?;
            }
            let event = CompletionObservation::NativeEvent {
                event_name,
                data: parsed.as_value(),
            };
            native.observe(&event, budget);
            transport.observe(&event);
        }
    }
    Ok(())
}

/// Private fields and consuming validation prevent proof cloning and reuse.
pub(in crate::proxy::security) struct VerifiedCompletion {
    attempt: AttemptProvenance,
    framing: [u8; 32],
}

impl VerifiedCompletion {
    pub(in crate::proxy::security) fn validate_sse(
        self,
        state: &SseRestorationState,
    ) -> Result<(), RestoreError> {
        let same_attempt = state
            .completion_attempt()
            .is_some_and(|actual| self.attempt.matches(actual));
        if same_attempt && self.framing == state.completion_frame_digest() {
            Ok(())
        } else {
            Err(RestoreError::RestoreFailed)
        }
    }
}

// Keep the owned verifier inline; boxing would add an unbudgeted request allocation.
#[allow(clippy::large_enum_variant)]
enum JsonEvidence {
    Unobserved,
    Json {
        completion: JsonCompletion,
        inspection: super::responses_adapter::ResponsesJsonInspection,
    },
    Sse {
        original: OriginalEvidence,
        inspection: super::responses_adapter::ResponsesJsonInspection,
    },
    Failed(RestoreError),
}

pub(super) struct JsonCompletionVerifier {
    evidence: Mutex<JsonEvidence>,
}

impl JsonCompletionVerifier {
    pub(super) fn new() -> Self {
        Self {
            evidence: Mutex::new(JsonEvidence::Unobserved),
        }
    }

    pub(super) fn is_unobserved(&self) -> Result<bool, RestoreError> {
        self.evidence
            .lock()
            .map(|state| matches!(*state, JsonEvidence::Unobserved))
            .map_err(|_| RestoreError::RestoreFailed)
    }

    fn record(&self, evidence: Result<JsonEvidence, RestoreError>) -> Result<(), RestoreError> {
        let mut state = self
            .evidence
            .lock()
            .map_err(|_| RestoreError::RestoreFailed)?;
        let evidence = if matches!(*state, JsonEvidence::Unobserved) {
            evidence
        } else {
            Err(RestoreError::RestoreFailed)
        };
        match evidence {
            Ok(evidence) => {
                *state = evidence;
                Ok(())
            }
            Err(error) => {
                *state = JsonEvidence::Failed(error);
                Err(error)
            }
        }
    }

    // Original protocol, binding and attempt are independent verifier inputs.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn observe_json(
        &self,
        expected: Option<JsonCompletionProtocol>,
        protocol: JsonCompletionProtocol,
        raw: &str,
        budget: &PayloadBudget,
        lease: &super::DlpRequestLease,
        binding: &ActualAttemptBinding,
        pair: ProtocolPair,
        attempt: &AttemptProvenance,
    ) -> Result<(), RestoreError> {
        let matches_protocol = matches!(
            (expected, protocol),
            (
                Some(JsonCompletionProtocol::NativeResponses),
                JsonCompletionProtocol::NativeResponses
            ) | (
                Some(JsonCompletionProtocol::ResponsesCompact),
                JsonCompletionProtocol::ResponsesCompact
            ) | (
                Some(JsonCompletionProtocol::ChatCompletions),
                JsonCompletionProtocol::ChatCompletions
            ) | (
                Some(JsonCompletionProtocol::AnthropicMessages),
                JsonCompletionProtocol::AnthropicMessages
            )
        );
        let mut completion = JsonCompletion::new(protocol);
        let parsed = completion.observe_raw_json_scoped(raw, budget);
        let evidence = if matches_protocol && completion.permits_completion() {
            (|| {
                let parsed = parsed.ok_or(RestoreError::RestoreFailed)?;
                let original =
                    super::super::restoration_contract::OriginalResponseObservation::json(
                        parsed.as_value(),
                        pair.upstream(),
                        attempt.clone(),
                        budget.clone(),
                    )?;
                let mut inspection = super::responses_adapter::ResponsesJsonInspection::begin(
                    lease,
                    binding,
                    budget,
                    pair,
                    attempt.clone(),
                )?;
                inspection.observe_original(&original)?;
                Ok(JsonEvidence::Json {
                    completion,
                    inspection,
                })
            })()
        } else if matches_protocol && completion.has_unsupported_carrier() {
            Err(RestoreError::RestoreUnsupported)
        } else {
            Err(RestoreError::RestoreFailed)
        };
        self.record(evidence)
    }

    pub(super) fn observe_sse(
        &self,
        evidence: Result<
            (
                OriginalEvidence,
                super::responses_adapter::ResponsesJsonInspection,
            ),
            RestoreError,
        >,
    ) -> Result<(), RestoreError> {
        self.record(evidence.and_then(|(original, inspection)| {
            original.completion_result()?;
            Ok(JsonEvidence::Sse {
                original,
                inspection,
            })
        }))
    }

    #[cfg(test)]
    pub(super) fn permits_completion(&self) -> bool {
        self.evidence.lock().is_ok_and(|state| match &*state {
            JsonEvidence::Json { completion, .. } => completion.permits_completion(),
            JsonEvidence::Sse { original, .. } => original.completion_result().is_ok(),
            _ => false,
        })
    }

    pub(super) fn finish(
        self,
        source: &Value,
        binding: &ActualAttemptBinding,
        status: u16,
    ) -> Result<
        (
            VerifiedJsonCompletion,
            super::responses_adapter::ResponsesJsonInspection,
        ),
        RestoreError,
    > {
        if status != 200 {
            return Err(RestoreError::RestoreFailed);
        }
        let inspection = match self
            .evidence
            .into_inner()
            .map_err(|_| RestoreError::RestoreFailed)?
        {
            JsonEvidence::Json {
                completion,
                inspection,
            } if completion.permits_completion() => inspection,
            JsonEvidence::Sse {
                original,
                inspection,
            } => {
                original.completion_result()?;
                inspection
            }
            JsonEvidence::Failed(error) => return Err(error),
            _ => return Err(RestoreError::RestoreFailed),
        };
        Ok((
            VerifiedJsonCompletion {
                source: json_digest(source)?,
                binding: binding.clone(),
                attempt: inspection.attempt().clone(),
            },
            inspection,
        ))
    }
}

pub(in crate::proxy::security) struct VerifiedJsonCompletion {
    source: [u8; 32],
    binding: ActualAttemptBinding,
    attempt: AttemptProvenance,
}

impl VerifiedJsonCompletion {
    pub(in crate::proxy::security) fn validate_json(
        self,
        source: &Value,
        binding: &ActualAttemptBinding,
        attempt: &AttemptProvenance,
    ) -> Result<(), RestoreError> {
        let verified = &self.binding;
        let same_binding = verified.store_id == binding.store_id
            && verified.namespace == binding.namespace
            && verified.generation == binding.generation
            && verified.policy_revision == binding.policy_revision
            && verified.provider_fingerprint == binding.provider_fingerprint
            && verified.reusable_principal == binding.reusable_principal
            && verified.operation_lineage == binding.operation_lineage;
        if same_binding && self.attempt.matches(attempt) && self.source == json_digest(source)? {
            Ok(())
        } else {
            Err(RestoreError::RestoreFailed)
        }
    }
}

fn json_digest(source: &Value) -> Result<[u8; 32], RestoreError> {
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

const NATIVE_ENTITY_LIMIT: usize = 1024 * 1024;

struct NativeVerifierContext {
    pair: ProtocolPair,
    attempt: AttemptProvenance,
    binding: ActualAttemptBinding,
    budget: PayloadBudget,
    failed: Option<RestoreError>,
}

impl NativeVerifierContext {
    fn new(
        pair: ProtocolPair,
        attempt: AttemptProvenance,
        binding: ActualAttemptBinding,
        budget: PayloadBudget,
    ) -> Self {
        Self {
            pair,
            attempt,
            binding,
            budget,
            failed: None,
        }
    }

    fn check(&self, actual: &AttemptProvenance) -> Result<(), RestoreError> {
        use crate::proxy::protocol_bridge::Protocol;
        if let Some(error) = self.failed {
            return Err(error);
        }
        // This primitive supports the native Chat fixture, not a registry grant.
        if self.pair.client().protocol() != Protocol::Chat
            || self.pair.upstream().protocol() != Protocol::Chat
        {
            return Err(RestoreError::RestoreUnsupported);
        }
        if !self.attempt.matches(actual) {
            return Err(RestoreError::RestoreFailed);
        }
        Ok(())
    }

    fn record<T>(&mut self, result: Result<T, RestoreError>) -> Result<T, RestoreError> {
        match result {
            Ok(value) => Ok(value),
            Err(error) => Err(*self.failed.get_or_insert(error)),
        }
    }

    fn into_proof(self, source: [u8; 32]) -> Result<NativeProof, RestoreError> {
        self.check(&self.attempt)?;
        Ok(NativeProof {
            source,
            attempt: self.attempt,
            binding: self.binding,
            client: self.pair.client(),
        })
    }
}

struct NativeProof {
    source: [u8; 32],
    attempt: AttemptProvenance,
    binding: ActualAttemptBinding,
    client: ClientProtocol,
}

impl NativeProof {
    fn validate(
        &self,
        source: [u8; 32],
        binding: &ActualAttemptBinding,
        attempt: &AttemptProvenance,
        client: ClientProtocol,
    ) -> Result<(), RestoreError> {
        let verified = &self.binding;
        let same_binding = verified.store_id == binding.store_id
            && verified.namespace == binding.namespace
            && verified.generation == binding.generation
            && verified.policy_revision == binding.policy_revision
            && verified.provider_fingerprint == binding.provider_fingerprint
            && verified.reusable_principal == binding.reusable_principal
            && verified.operation_lineage == binding.operation_lineage;
        if same_binding
            && self.attempt.matches(attempt)
            && self.client == client
            && self.source == source
        {
            Ok(())
        } else {
            Err(RestoreError::RestoreFailed)
        }
    }
}

struct NativeJsonEvidence {
    completion: JsonCompletion,
    transport: CompletionTransport,
    source: [u8; 32],
    attempt: AttemptProvenance,
}

impl NativeJsonEvidence {
    fn observe(
        raw: &str,
        actual: &AttemptProvenance,
        budget: &PayloadBudget,
    ) -> Result<Self, RestoreError> {
        let mut completion = JsonCompletion::new(JsonCompletionProtocol::ChatCompletions);
        completion.observe_raw_json(raw, budget);
        if completion.has_unsupported_carrier() {
            return Err(RestoreError::RestoreUnsupported);
        }
        if !completion.permits_completion() {
            return Err(RestoreError::RestoreFailed);
        }
        let parsed = PayloadJson::try_from_str(raw, budget.clone(), NATIVE_ENTITY_LIMIT)
            .map_err(|_| RestoreError::RestoreFailed)?;
        let mut transport = CompletionTransport::default();
        transport.observe_fully_read_json();
        Ok(Self {
            completion,
            transport,
            source: native_json_digest(parsed.as_value())?,
            attempt: actual.clone(),
        })
    }

    fn validate(&self, context: &NativeVerifierContext) -> Result<(), RestoreError> {
        context.check(&self.attempt)?;
        if self.completion.permits_completion() && self.transport.permits_completion() {
            Ok(())
        } else {
            Err(RestoreError::RestoreFailed)
        }
    }
}

pub(super) struct NativeJsonVerifier {
    context: NativeVerifierContext,
    original: Option<NativeJsonEvidence>,
    client: Option<NativeJsonEvidence>,
}

impl NativeJsonVerifier {
    pub(super) fn new(
        pair: ProtocolPair,
        provenance: AttemptProvenance,
        binding: ActualAttemptBinding,
        budget: PayloadBudget,
    ) -> Self {
        Self {
            context: NativeVerifierContext::new(pair, provenance, binding, budget),
            original: None,
            client: None,
        }
    }

    /// Consume the original entity, including the read that establishes actual EOF.
    pub(super) fn consume_upstream_json<R: io::Read>(
        &mut self,
        source: R,
        actual: &AttemptProvenance,
    ) -> Result<(), RestoreError> {
        let result = self.context.check(actual).and_then(|()| {
            if self.original.is_some() {
                return Err(RestoreError::RestoreFailed);
            }
            let bytes = consume_native_entity(source, &self.context.budget)?;
            let raw =
                std::str::from_utf8(bytes.as_slice()).map_err(|_| RestoreError::RestoreFailed)?;
            NativeJsonEvidence::observe(raw, actual, &self.context.budget)
        });
        self.original = Some(self.context.record(result)?);
        Ok(())
    }

    /// Inspect the fully owned client JSON before any restoration changes its value.
    pub(super) fn observe_client_json(
        &mut self,
        source: &Value,
        actual: &AttemptProvenance,
    ) -> Result<(), RestoreError> {
        let result = self.context.check(actual).and_then(|()| {
            if self.client.is_some() {
                return Err(RestoreError::RestoreFailed);
            }
            // Preflight depth/nodes and charge the independent serializer input.
            let owned = PayloadJson::try_from_value(
                source,
                self.context.budget.clone(),
                NATIVE_ENTITY_LIMIT,
            )
            .map_err(|_| RestoreError::RestoreFailed)?;
            let bytes = serialize_native_json(owned.as_value(), &self.context.budget)?;
            let raw =
                std::str::from_utf8(bytes.as_slice()).map_err(|_| RestoreError::RestoreFailed)?;
            NativeJsonEvidence::observe(raw, actual, &self.context.budget)
        });
        self.client = Some(self.context.record(result)?);
        Ok(())
    }

    pub(super) fn finish(self) -> Result<VerifiedNativeJson, RestoreError> {
        self.context.check(&self.context.attempt)?;
        let original = self.original.ok_or(RestoreError::RestoreFailed)?;
        let client = self.client.ok_or(RestoreError::RestoreFailed)?;
        original.validate(&self.context)?;
        client.validate(&self.context)?;
        if original.source != client.source {
            return Err(RestoreError::RestoreFailed);
        }
        Ok(VerifiedNativeJson {
            evidence: self.context.into_proof(client.source)?,
        })
    }
}

pub(in crate::proxy::security) struct VerifiedNativeJson {
    evidence: NativeProof,
}

impl VerifiedNativeJson {
    /// Reject foreign identity before any finalizer allocation; this grants no release.
    pub(in crate::proxy::security) fn check_source(
        &self,
        source: &Value,
        binding: &ActualAttemptBinding,
        attempt: &AttemptProvenance,
        client: ClientProtocol,
    ) -> Result<(), RestoreError> {
        self.evidence
            .validate(native_json_digest(source)?, binding, attempt, client)
    }

    pub(in crate::proxy::security) fn validate(
        self,
        source: &Value,
        binding: &ActualAttemptBinding,
        attempt: &AttemptProvenance,
        client: ClientProtocol,
    ) -> Result<(), RestoreError> {
        self.evidence
            .validate(native_json_digest(source)?, binding, attempt, client)
    }

    /// Consume the original HTTP proof before checking the restored JSON value.
    /// The existing strict parser rechecks inner arguments; no new proof is issued.
    pub(in crate::proxy::security) fn validate_restored(
        self,
        source: &Value,
        binding: &ActualAttemptBinding,
        attempt: &AttemptProvenance,
        client: ClientProtocol,
        restored: &[u8],
        budget: &PayloadBudget,
    ) -> Result<(), RestoreError> {
        self.validate(source, binding, attempt, client)?;
        let raw = std::str::from_utf8(restored).map_err(|_| RestoreError::RestoreFailed)?;
        NativeJsonEvidence::observe(raw, attempt, budget)?;
        Ok(())
    }
}

struct NativeSseEvidence {
    completion: SseCompletion,
    transport: CompletionTransport,
    done: bool,
    source: [u8; 32],
    attempt: AttemptProvenance,
}

impl NativeSseEvidence {
    fn consume<R: io::Read>(
        source: R,
        actual: &AttemptProvenance,
        budget: &PayloadBudget,
    ) -> Result<Self, RestoreError> {
        let bytes = consume_native_entity(source, budget)?;
        let raw = std::str::from_utf8(bytes.as_slice()).map_err(|_| RestoreError::RestoreFailed)?;
        let mut evidence = Self {
            completion: SseCompletion::chat(),
            transport: CompletionTransport::default(),
            done: false,
            source: framed_native_digest(b"native-sse-entity-v1\0", bytes.as_slice()),
            attempt: actual.clone(),
        };
        let mut frame_start = 0usize;
        let mut offset = 0usize;
        for line in raw.split_inclusive('\n') {
            offset += line.len();
            let content = line.strip_suffix('\n').unwrap_or(line);
            let content = content.strip_suffix('\r').unwrap_or(content);
            if content.contains('\r') {
                return Err(RestoreError::RestoreFailed);
            }
            if content.is_empty() {
                evidence.observe_block(&raw[frame_start..offset], budget)?;
                frame_start = offset;
            }
        }
        // A terminal marker in an unfinished frame is not a completed entity.
        if frame_start != raw.len() {
            return Err(RestoreError::RestoreFailed);
        }
        // consume_native_entity returned only after the source actually returned 0.
        let end = CompletionObservation::EntityEnd {
            reached_eof: true,
            unfinished_frame: false,
            unfinished_utf8: false,
        };
        evidence.completion.observe(&end, budget);
        evidence.transport.observe(&end);
        evidence.check_completion()?;
        Ok(evidence)
    }

    fn observe_block(&mut self, block: &str, budget: &PayloadBudget) -> Result<(), RestoreError> {
        let raw = CompletionObservation::RawFrame(block);
        self.completion.observe(&raw, budget);
        self.transport.observe(&raw);
        let mut data = PayloadBuffer::new(budget.clone(), NATIVE_ENTITY_LIMIT)
            .map_err(|_| RestoreError::RestoreFailed)?;
        let mut count = 0usize;
        for line in block.lines() {
            if let Some(value) = line
                .strip_prefix("data:")
                .or_else(|| (line == "data").then_some(""))
            {
                if count != 0 {
                    data.append(b"\n")
                        .map_err(|_| RestoreError::RestoreFailed)?;
                }
                count += 1;
                data.append(value.strip_prefix(' ').unwrap_or(value).as_bytes())
                    .map_err(|_| RestoreError::RestoreFailed)?;
            }
        }
        if count != 0 {
            if self.done {
                return Err(RestoreError::RestoreFailed);
            }
            let encoded =
                std::str::from_utf8(data.as_slice()).map_err(|_| RestoreError::RestoreFailed)?;
            if encoded.trim() == "[DONE]" {
                self.completion
                    .observe(&CompletionObservation::ChatDone, budget);
                self.done = true;
            } else {
                let parsed =
                    PayloadJson::try_from_str(encoded, budget.clone(), NATIVE_ENTITY_LIMIT)
                        .map_err(|_| RestoreError::RestoreFailed)?;
                self.completion
                    .observe(&CompletionObservation::ChatChunk(parsed.as_value()), budget);
            }
        }
        if self.completion.has_unsupported_carrier() {
            return Err(RestoreError::RestoreUnsupported);
        }
        Ok(())
    }

    fn check_completion(&self) -> Result<(), RestoreError> {
        if self.done && self.completion.permits_completion() && self.transport.permits_completion()
        {
            Ok(())
        } else {
            Err(RestoreError::RestoreFailed)
        }
    }

    fn validate(&self, context: &NativeVerifierContext) -> Result<(), RestoreError> {
        context.check(&self.attempt)?;
        self.check_completion()
    }
}

pub(super) struct NativeSseVerifier {
    context: NativeVerifierContext,
    original: Option<NativeSseEvidence>,
    client: Option<NativeSseEvidence>,
}

impl NativeSseVerifier {
    pub(super) fn new(
        pair: ProtocolPair,
        provenance: AttemptProvenance,
        binding: ActualAttemptBinding,
        budget: PayloadBudget,
    ) -> Self {
        Self {
            context: NativeVerifierContext::new(pair, provenance, binding, budget),
            original: None,
            client: None,
        }
    }

    /// Consume raw original frames and the actual entity end, not caller EOF claims.
    pub(super) fn consume_upstream_sse<R: io::Read>(
        &mut self,
        source: R,
        actual: &AttemptProvenance,
    ) -> Result<(), RestoreError> {
        let result = self.context.check(actual).and_then(|()| {
            if self.original.is_some() {
                return Err(RestoreError::RestoreFailed);
            }
            NativeSseEvidence::consume(source, actual, &self.context.budget)
        });
        self.original = Some(self.context.record(result)?);
        Ok(())
    }

    /// Consume exact client framing independently of original-protocol evidence.
    pub(super) fn consume_client_sse<R: io::Read>(
        &mut self,
        source: R,
        actual: &AttemptProvenance,
    ) -> Result<(), RestoreError> {
        let result = self.context.check(actual).and_then(|()| {
            if self.client.is_some() {
                return Err(RestoreError::RestoreFailed);
            }
            NativeSseEvidence::consume(source, actual, &self.context.budget)
        });
        self.client = Some(self.context.record(result)?);
        Ok(())
    }

    pub(super) fn finish(self) -> Result<VerifiedNativeSse, RestoreError> {
        self.context.check(&self.context.attempt)?;
        let original = self.original.ok_or(RestoreError::RestoreFailed)?;
        let client = self.client.ok_or(RestoreError::RestoreFailed)?;
        original.validate(&self.context)?;
        client.validate(&self.context)?;
        if original.source != client.source {
            return Err(RestoreError::RestoreFailed);
        }
        Ok(VerifiedNativeSse {
            evidence: self.context.into_proof(client.source)?,
        })
    }
}

pub(in crate::proxy::security) struct VerifiedNativeSse {
    evidence: NativeProof,
}

impl VerifiedNativeSse {
    pub(in crate::proxy::security) fn validate(
        self,
        source: &[u8],
        binding: &ActualAttemptBinding,
        attempt: &AttemptProvenance,
        client: ClientProtocol,
    ) -> Result<(), RestoreError> {
        self.evidence.validate(
            framed_native_digest(b"native-sse-entity-v1\0", source),
            binding,
            attempt,
            client,
        )
    }

    /// Recheck actual restored wire with the same strict tool and terminal parser.
    /// This consumes an existing HTTP proof; it cannot create EOF or new authority.
    pub(in crate::proxy::security) fn validate_restored<R: io::Read>(
        self,
        source: &[u8],
        binding: &ActualAttemptBinding,
        attempt: &AttemptProvenance,
        client: ClientProtocol,
        restored: R,
        budget: &PayloadBudget,
    ) -> Result<(), RestoreError> {
        self.validate(source, binding, attempt, client)?;
        NativeSseEvidence::consume(restored, attempt, budget)?;
        Ok(())
    }
}

fn consume_native_entity<R: io::Read>(
    mut source: R,
    budget: &PayloadBudget,
) -> Result<PayloadBuffer, RestoreError> {
    // Account for the bounded read scratch as well as all retained entity copies.
    let _scratch_charge = budget
        .reserve(4096)
        .map_err(|_| RestoreError::RestoreFailed)?;
    let mut scratch = [0u8; 4096];
    let mut bytes = PayloadBuffer::new(budget.clone(), NATIVE_ENTITY_LIMIT)
        .map_err(|_| RestoreError::RestoreFailed)?;
    loop {
        // Interrupted/cancellation is a failure, never a retry or fabricated EOF.
        let count = source
            .read(&mut scratch)
            .map_err(|_| RestoreError::RestoreFailed)?;
        if count == 0 {
            return Ok(bytes);
        }
        let chunk = scratch.get(..count).ok_or(RestoreError::RestoreFailed)?;
        bytes
            .append(chunk)
            .map_err(|_| RestoreError::RestoreFailed)?;
    }
}

fn serialize_native_json(
    source: &Value,
    budget: &PayloadBudget,
) -> Result<PayloadBuffer, RestoreError> {
    struct SizeWriter(usize);
    impl io::Write for SizeWriter {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.0 = self
                .0
                .checked_add(bytes.len())
                .filter(|size| *size <= NATIVE_ENTITY_LIMIT)
                .ok_or_else(|| io::Error::other("native JSON size limit"))?;
            Ok(bytes.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    struct BufferWriter(PayloadBuffer);
    impl io::Write for BufferWriter {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.0
                .append(bytes)
                .map_err(|_| io::Error::other("native JSON capacity"))?;
            Ok(bytes.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    let mut size = SizeWriter(0);
    serde_json::to_writer(&mut size, source).map_err(|_| RestoreError::RestoreFailed)?;
    let mut writer = BufferWriter(
        PayloadBuffer::with_capacity(size.0, budget.clone(), NATIVE_ENTITY_LIMIT)
            .map_err(|_| RestoreError::RestoreFailed)?,
    );
    serde_json::to_writer(&mut writer, source).map_err(|_| RestoreError::RestoreFailed)?;
    Ok(writer.0)
}

fn framed_native_digest(domain: &[u8], source: &[u8]) -> [u8; 32] {
    let mut digest = Sha256::new();
    digest.update(domain);
    digest.update((source.len() as u64).to_le_bytes());
    digest.update(source);
    digest.finalize().into()
}

fn native_json_digest(source: &Value) -> Result<[u8; 32], RestoreError> {
    Ok(framed_native_digest(
        b"native-json-value-v1\0",
        &json_digest(source)?,
    ))
}

#[cfg(test)]
mod c5_native_completion_tests {
    use super::*;
    use crate::proxy::{
        protocol_bridge::Protocol,
        security::{
            restoration_contract::{ContinuityScope, IngressNamespace, UpstreamProtocol},
            tokenization::{DlpRequestLease, DlpTokenStore},
        },
    };
    use std::{
        cell::Cell,
        io::{Cursor, Read},
        rc::Rc,
    };

    type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

    struct Fixture {
        store: DlpTokenStore,
        lease: DlpRequestLease,
        budget: PayloadBudget,
        attempt: AttemptProvenance,
        binding: ActualAttemptBinding,
    }

    impl Fixture {
        fn new() -> TestResult<Self> {
            let store = DlpTokenStore::new();
            let lease = store.admit_scoped_published_policy(
                ContinuityScope::Native {
                    client: ClientProtocol::new(Protocol::Chat),
                    ingress: IngressNamespace::Direct,
                },
                "native-fixture",
                Some("native-completion-session"),
                1,
            )?;
            let binding = lease.bind_actual_attempt(
                "native-completion-provider",
                "openai_chat",
                "https://native-fixture.invalid/v1/chat/completions",
                Some("native-fixture-principal"),
            )?;
            let budget = PayloadBudget::with_limit(1024 * 1024);
            let attempt = AttemptProvenance::new(&budget)?;
            assert_eq!(store.stats()?.entries, 0, "fixture must have zero mappings");
            Ok(Self {
                store,
                lease,
                budget,
                attempt,
                binding,
            })
        }

        fn json(&self) -> NativeJsonVerifier {
            NativeJsonVerifier::new(
                pair(Protocol::Chat, Protocol::Chat),
                self.attempt.clone(),
                self.binding.clone(),
                self.budget.clone(),
            )
        }

        fn sse(&self) -> NativeSseVerifier {
            NativeSseVerifier::new(
                pair(Protocol::Chat, Protocol::Chat),
                self.attempt.clone(),
                self.binding.clone(),
                self.budget.clone(),
            )
        }

        fn json_proof(&self, raw: &str, client: &Value) -> TestResult<VerifiedNativeJson> {
            let mut verifier = self.json();
            require(verifier.consume_upstream_json(Cursor::new(raw.as_bytes()), &self.attempt))?;
            require(verifier.observe_client_json(client, &self.attempt))?;
            require(verifier.finish())
        }

        fn sse_proof(&self, upstream: &[u8], client: &[u8]) -> TestResult<VerifiedNativeSse> {
            let mut verifier = self.sse();
            require(verifier.consume_upstream_sse(Cursor::new(upstream), &self.attempt))?;
            require(verifier.consume_client_sse(Cursor::new(client), &self.attempt))?;
            require(verifier.finish())
        }
    }

    fn require<T>(result: Result<T, RestoreError>) -> TestResult<T> {
        result.map_err(|_| io::Error::other("native completion fixture was rejected").into())
    }

    fn pair(client: Protocol, upstream: Protocol) -> ProtocolPair {
        ProtocolPair::new(ClientProtocol::new(client), UpstreamProtocol::new(upstream))
    }

    fn chat_json() -> Value {
        serde_json::json!({
            "id": "chatcmpl_native_fixture",
            "object": "chat.completion",
            "choices": [{
                "index": 0,
                "message": {"role": "assistant", "content": "synthetic native text"},
                "finish_reason": "stop"
            }]
        })
    }

    fn chat_sse() -> Vec<u8> {
        let chunk = serde_json::json!({
            "id": "chatcmpl_native_fixture",
            "object": "chat.completion.chunk",
            "choices": [{
                "index": 0,
                "delta": {"role": "assistant", "content": "synthetic native text"},
                "finish_reason": null
            }]
        });
        let terminal = serde_json::json!({
            "id": "chatcmpl_native_fixture",
            "object": "chat.completion.chunk",
            "choices": [{"index": 0, "delta": {}, "finish_reason": "stop"}]
        });
        format!("data: {chunk}\n\ndata: {terminal}\n\ndata: [DONE]\n\n").into_bytes()
    }

    // Reaching protocol terminals is deliberately independent from reaching EOF.
    struct EntityReader {
        bytes: Cursor<Vec<u8>>,
        eof_reads: Rc<Cell<usize>>,
        terminal_error: Option<io::ErrorKind>,
        max_chunk: usize,
    }

    impl EntityReader {
        fn new(bytes: Vec<u8>, terminal_error: Option<io::ErrorKind>) -> (Self, Rc<Cell<usize>>) {
            let eof_reads = Rc::new(Cell::new(0));
            (
                Self {
                    bytes: Cursor::new(bytes),
                    eof_reads: Rc::clone(&eof_reads),
                    terminal_error,
                    max_chunk: 7,
                },
                eof_reads,
            )
        }
    }

    impl Read for EntityReader {
        fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
            if output.is_empty() {
                return Ok(0);
            }
            let length = output.len().min(self.max_chunk);
            let count = self.bytes.read(&mut output[..length])?;
            if count == 0 {
                if let Some(kind) = self.terminal_error {
                    return Err(io::Error::new(kind, "synthetic entity read failure"));
                }
                self.eof_reads.set(self.eof_reads.get() + 1);
            }
            Ok(count)
        }
    }

    #[test]
    fn c5_native_completion_json_chat_positive_consumes_real_upstream_eof() -> TestResult {
        let fixture = Fixture::new()?;
        let client = chat_json();
        let (reader, eof_reads) = EntityReader::new(serde_json::to_vec(&client)?, None);
        let mut verifier = fixture.json();
        require(verifier.consume_upstream_json(reader, &fixture.attempt))?;
        assert!(eof_reads.get() > 0, "JSON must read through actual EOF");
        require(verifier.observe_client_json(&client, &fixture.attempt))?;
        let verified = require(verifier.finish())?;
        require(verified.validate(
            &client,
            &fixture.binding,
            &fixture.attempt,
            ClientProtocol::new(Protocol::Chat),
        ))?;
        assert_eq!(fixture.store.stats()?.entries, 0);
        Ok(())
    }

    #[test]
    fn c5_native_completion_json_requires_original_and_client() -> TestResult {
        let fixture = Fixture::new()?;
        let client = chat_json();
        assert!(fixture.json().finish().is_err());
        let mut no_original = fixture.json();
        require(no_original.observe_client_json(&client, &fixture.attempt))?;
        assert!(no_original.finish().is_err());
        let mut no_client = fixture.json();
        require(
            no_client
                .consume_upstream_json(Cursor::new(serde_json::to_vec(&client)?), &fixture.attempt),
        )?;
        assert!(no_client.finish().is_err());
        Ok(())
    }

    #[test]
    fn c5_native_completion_json_rejects_incomplete_original_and_client() -> TestResult {
        let fixture = Fixture::new()?;
        let valid = chat_json();
        for reason in [
            Value::Null,
            Value::from("length"),
            Value::from("content_filter"),
        ] {
            let mut incomplete = valid.clone();
            incomplete["choices"][0]["finish_reason"] = reason;
            let mut upstream = fixture.json();
            assert!(upstream
                .consume_upstream_json(
                    Cursor::new(serde_json::to_vec(&incomplete)?),
                    &fixture.attempt,
                )
                .is_err());
            let _ = upstream.observe_client_json(&valid, &fixture.attempt);
            assert!(upstream.finish().is_err());
            let mut client = fixture.json();
            require(client.consume_upstream_json(
                Cursor::new(serde_json::to_vec(&valid)?),
                &fixture.attempt,
            ))?;
            assert!(client
                .observe_client_json(&incomplete, &fixture.attempt)
                .is_err());
            assert!(client.finish().is_err());
        }
        Ok(())
    }

    #[test]
    fn c5_native_completion_json_rejects_malformed_or_multiple_entities() -> TestResult {
        let fixture = Fixture::new()?;
        let valid = chat_json();
        let raw = serde_json::to_string(&valid)?;
        for invalid in [
            String::new(),
            raw[..raw.len() - 1].to_owned(),
            format!("{raw}{raw}"),
        ] {
            let mut verifier = fixture.json();
            assert!(verifier
                .consume_upstream_json(Cursor::new(invalid.as_bytes()), &fixture.attempt)
                .is_err());
            let _ = verifier.observe_client_json(&valid, &fixture.attempt);
            assert!(verifier.finish().is_err());
        }
        Ok(())
    }

    #[test]
    fn c5_native_completion_json_rejects_read_failure_after_valid_body() -> TestResult {
        let fixture = Fixture::new()?;
        let valid = chat_json();
        let (reader, eof_reads) = EntityReader::new(
            serde_json::to_vec(&valid)?,
            Some(io::ErrorKind::ConnectionReset),
        );
        let mut verifier = fixture.json();
        assert!(verifier
            .consume_upstream_json(reader, &fixture.attempt)
            .is_err());
        assert_eq!(eof_reads.get(), 0);
        let _ = verifier.observe_client_json(&valid, &fixture.attempt);
        assert!(verifier.finish().is_err());
        Ok(())
    }

    #[test]
    fn c5_native_completion_json_wrong_attempt_poisoned_even_without_mappings() -> TestResult {
        let fixture = Fixture::new()?;
        let wrong = AttemptProvenance::new(&fixture.budget)?;
        let valid = chat_json();
        let raw = serde_json::to_vec(&valid)?;
        let mut upstream = fixture.json();
        assert!(upstream
            .consume_upstream_json(Cursor::new(&raw), &wrong)
            .is_err());
        let _ = upstream.observe_client_json(&valid, &fixture.attempt);
        assert!(upstream.finish().is_err());
        let mut client = fixture.json();
        require(client.consume_upstream_json(Cursor::new(&raw), &fixture.attempt))?;
        assert!(client.observe_client_json(&valid, &wrong).is_err());
        assert!(client.finish().is_err());
        Ok(())
    }

    #[test]
    fn c5_native_completion_json_binds_digest_attempt_binding_and_client() -> TestResult {
        let fixture = Fixture::new()?;
        let valid = chat_json();
        let raw = serde_json::to_string(&valid)?;
        let wrong_attempt = AttemptProvenance::new(&fixture.budget)?;
        let mut changed = valid.clone();
        changed["choices"][0]["message"]["content"] = Value::from("different synthetic text");
        assert!(fixture
            .json_proof(&raw, &valid)?
            .validate(
                &changed,
                &fixture.binding,
                &fixture.attempt,
                ClientProtocol::new(Protocol::Chat),
            )
            .is_err());
        assert!(fixture
            .json_proof(&raw, &valid)?
            .validate(
                &valid,
                &fixture.binding,
                &wrong_attempt,
                ClientProtocol::new(Protocol::Chat),
            )
            .is_err());
        let foreign = Fixture::new()?;
        assert!(fixture
            .json_proof(&raw, &valid)?
            .validate(
                &valid,
                &foreign.binding,
                &fixture.attempt,
                ClientProtocol::new(Protocol::Chat),
            )
            .is_err());
        let wrong_provider = fixture.lease.bind_actual_attempt(
            "other-native-provider",
            "openai_chat",
            "https://native-fixture.invalid/v1/chat/completions",
            Some("native-fixture-principal"),
        )?;
        assert!(fixture
            .json_proof(&raw, &valid)?
            .validate(
                &valid,
                &wrong_provider,
                &fixture.attempt,
                ClientProtocol::new(Protocol::Chat),
            )
            .is_err());
        assert!(fixture
            .json_proof(&raw, &valid)?
            .validate(
                &valid,
                &fixture.binding,
                &fixture.attempt,
                ClientProtocol::new(Protocol::Responses),
            )
            .is_err());
        Ok(())
    }

    #[test]
    fn c5_native_completion_json_duplicate_observation_cannot_reset_failure() -> TestResult {
        let fixture = Fixture::new()?;
        let valid = chat_json();
        let raw = serde_json::to_vec(&valid)?;
        let mut upstream = fixture.json();
        require(upstream.consume_upstream_json(Cursor::new(&raw), &fixture.attempt))?;
        assert!(upstream
            .consume_upstream_json(Cursor::new(&raw), &fixture.attempt)
            .is_err());
        let _ = upstream.observe_client_json(&valid, &fixture.attempt);
        assert!(upstream.finish().is_err());
        let mut client = fixture.json();
        require(client.consume_upstream_json(Cursor::new(&raw), &fixture.attempt))?;
        require(client.observe_client_json(&valid, &fixture.attempt))?;
        assert!(client
            .observe_client_json(&valid, &fixture.attempt)
            .is_err());
        assert!(client.finish().is_err());
        Ok(())
    }

    #[test]
    fn c5_native_completion_sse_chat_positive_consumes_both_actual_eofs() -> TestResult {
        let fixture = Fixture::new()?;
        let source = chat_sse();
        let (upstream, upstream_eof) = EntityReader::new(source.clone(), None);
        let (client, client_eof) = EntityReader::new(source.clone(), None);
        let mut verifier = fixture.sse();
        require(verifier.consume_upstream_sse(upstream, &fixture.attempt))?;
        require(verifier.consume_client_sse(client, &fixture.attempt))?;
        assert!(upstream_eof.get() > 0 && client_eof.get() > 0);
        let verified = require(verifier.finish())?;
        require(verified.validate(
            &source,
            &fixture.binding,
            &fixture.attempt,
            ClientProtocol::new(Protocol::Chat),
        ))?;
        Ok(())
    }

    #[test]
    fn c5_native_completion_sse_requires_original_and_client_entities() -> TestResult {
        let fixture = Fixture::new()?;
        let source = chat_sse();
        assert!(fixture.sse().finish().is_err());
        let mut no_original = fixture.sse();
        require(no_original.consume_client_sse(Cursor::new(&source), &fixture.attempt))?;
        assert!(no_original.finish().is_err());
        let mut no_client = fixture.sse();
        require(no_client.consume_upstream_sse(Cursor::new(&source), &fixture.attempt))?;
        assert!(no_client.finish().is_err());
        Ok(())
    }

    #[test]
    fn c5_native_completion_sse_requires_finish_done_and_complete_framing() -> TestResult {
        let fixture = Fixture::new()?;
        let source = chat_sse();
        let encoded = std::str::from_utf8(&source)?;
        let unfinished = encoded.replace("\"finish_reason\":\"stop\"", "\"finish_reason\":null");
        let no_done = encoded.replace("data: [DONE]\n\n", "");
        let truncated = encoded.trim_end_matches('\n').to_owned();
        let post_done = format!("{encoded}data: {{\"id\":\"unexpected\"}}\n\n");
        for invalid in [
            unfinished.as_bytes(),
            no_done.as_bytes(),
            truncated.as_bytes(),
            post_done.as_bytes(),
        ] {
            let mut upstream = fixture.sse();
            let _ = upstream.consume_upstream_sse(Cursor::new(invalid), &fixture.attempt);
            let _ = upstream.consume_client_sse(Cursor::new(&source), &fixture.attempt);
            assert!(upstream.finish().is_err());
            let mut client = fixture.sse();
            let _ = client.consume_upstream_sse(Cursor::new(&source), &fixture.attempt);
            let _ = client.consume_client_sse(Cursor::new(invalid), &fixture.attempt);
            assert!(client.finish().is_err());
        }
        Ok(())
    }

    #[test]
    fn c5_native_completion_sse_rejects_cancellation_after_done_before_eof() -> TestResult {
        let fixture = Fixture::new()?;
        let source = chat_sse();
        for kind in [io::ErrorKind::Interrupted, io::ErrorKind::ConnectionReset] {
            let (reader, eof_reads) = EntityReader::new(source.clone(), Some(kind));
            let mut upstream = fixture.sse();
            assert!(upstream
                .consume_upstream_sse(reader, &fixture.attempt)
                .is_err());
            assert_eq!(eof_reads.get(), 0);
            let _ = upstream.consume_client_sse(Cursor::new(&source), &fixture.attempt);
            assert!(upstream.finish().is_err());
            let (reader, eof_reads) = EntityReader::new(source.clone(), Some(kind));
            let mut client = fixture.sse();
            require(client.consume_upstream_sse(Cursor::new(&source), &fixture.attempt))?;
            assert!(client.consume_client_sse(reader, &fixture.attempt).is_err());
            assert_eq!(eof_reads.get(), 0);
            assert!(client.finish().is_err());
        }
        Ok(())
    }

    #[test]
    fn c5_native_completion_sse_rejects_invalid_utf8_and_partial_frame() -> TestResult {
        let fixture = Fixture::new()?;
        let source = chat_sse();
        let mut invalid_utf8 = source.clone();
        invalid_utf8.extend_from_slice(&[0xf0, 0x9f]);
        let mut partial_frame = source.clone();
        partial_frame.extend_from_slice(b"data: {");
        for invalid in [invalid_utf8, partial_frame] {
            let mut upstream = fixture.sse();
            let _ = upstream.consume_upstream_sse(Cursor::new(&invalid), &fixture.attempt);
            let _ = upstream.consume_client_sse(Cursor::new(&source), &fixture.attempt);
            assert!(upstream.finish().is_err());
            let mut client = fixture.sse();
            let _ = client.consume_upstream_sse(Cursor::new(&source), &fixture.attempt);
            let _ = client.consume_client_sse(Cursor::new(&invalid), &fixture.attempt);
            assert!(client.finish().is_err());
        }
        Ok(())
    }

    #[test]
    fn c5_native_completion_sse_wrong_attempt_poisoned_even_without_mappings() -> TestResult {
        let fixture = Fixture::new()?;
        let wrong = AttemptProvenance::new(&fixture.budget)?;
        let source = chat_sse();
        let mut upstream = fixture.sse();
        assert!(upstream
            .consume_upstream_sse(Cursor::new(&source), &wrong)
            .is_err());
        let _ = upstream.consume_client_sse(Cursor::new(&source), &fixture.attempt);
        assert!(upstream.finish().is_err());
        let mut client = fixture.sse();
        require(client.consume_upstream_sse(Cursor::new(&source), &fixture.attempt))?;
        assert!(client
            .consume_client_sse(Cursor::new(&source), &wrong)
            .is_err());
        assert!(client.finish().is_err());
        Ok(())
    }

    #[test]
    fn c5_native_completion_sse_binds_exact_framing_attempt_binding_and_client() -> TestResult {
        let fixture = Fixture::new()?;
        let source = chat_sse();
        let changed = std::str::from_utf8(&source)?
            .replace('\n', "\r\n")
            .into_bytes();
        assert!(fixture
            .sse_proof(&source, &source)?
            .validate(
                &changed,
                &fixture.binding,
                &fixture.attempt,
                ClientProtocol::new(Protocol::Chat),
            )
            .is_err());
        let wrong_attempt = AttemptProvenance::new(&fixture.budget)?;
        assert!(fixture
            .sse_proof(&source, &source)?
            .validate(
                &source,
                &fixture.binding,
                &wrong_attempt,
                ClientProtocol::new(Protocol::Chat),
            )
            .is_err());
        let foreign = Fixture::new()?;
        assert!(fixture
            .sse_proof(&source, &source)?
            .validate(
                &source,
                &foreign.binding,
                &fixture.attempt,
                ClientProtocol::new(Protocol::Chat),
            )
            .is_err());
        assert!(fixture
            .sse_proof(&source, &source)?
            .validate(
                &source,
                &fixture.binding,
                &fixture.attempt,
                ClientProtocol::new(Protocol::Anthropic),
            )
            .is_err());
        Ok(())
    }

    #[test]
    fn c5_native_completion_sse_duplicate_entity_cannot_reset_failure() -> TestResult {
        let fixture = Fixture::new()?;
        let source = chat_sse();
        let mut upstream = fixture.sse();
        require(upstream.consume_upstream_sse(Cursor::new(&source), &fixture.attempt))?;
        assert!(upstream
            .consume_upstream_sse(Cursor::new(&source), &fixture.attempt)
            .is_err());
        let _ = upstream.consume_client_sse(Cursor::new(&source), &fixture.attempt);
        assert!(upstream.finish().is_err());
        let mut client = fixture.sse();
        require(client.consume_upstream_sse(Cursor::new(&source), &fixture.attempt))?;
        require(client.consume_client_sse(Cursor::new(&source), &fixture.attempt))?;
        assert!(client
            .consume_client_sse(Cursor::new(&source), &fixture.attempt)
            .is_err());
        assert!(client.finish().is_err());
        Ok(())
    }

    #[test]
    fn c5_native_completion_json_rejects_unrelated_complete_original() -> TestResult {
        let fixture = Fixture::new()?;
        let original = chat_json();
        let mut unrelated = original.clone();
        unrelated["choices"][0]["message"]["content"] = Value::from("unrelated synthetic text");
        let mut verifier = fixture.json();
        require(verifier.consume_upstream_json(
            Cursor::new(serde_json::to_vec(&original)?),
            &fixture.attempt,
        ))?;
        let _ = verifier.observe_client_json(&unrelated, &fixture.attempt);
        assert!(verifier.finish().is_err());
        Ok(())
    }

    #[test]
    fn c5_native_completion_sse_rejects_unrelated_complete_original() -> TestResult {
        let fixture = Fixture::new()?;
        let original = chat_sse();
        let unrelated = std::str::from_utf8(&original)?
            .replace("synthetic native text", "unrelated synthetic text")
            .into_bytes();
        let mut verifier = fixture.sse();
        require(verifier.consume_upstream_sse(Cursor::new(&original), &fixture.attempt))?;
        let _ = verifier.consume_client_sse(Cursor::new(&unrelated), &fixture.attempt);
        assert!(verifier.finish().is_err());
        Ok(())
    }

    #[test]
    fn c5_native_completion_budget_failure_is_sticky_and_releases_allocations() -> TestResult {
        let fixture = Fixture::new()?;
        let original = chat_json();
        let json = serde_json::to_vec(&original)?;
        let sse = chat_sse();
        require(
            fixture
                .json_proof(std::str::from_utf8(&json)?, &original)?
                .validate(
                    &original,
                    &fixture.binding,
                    &fixture.attempt,
                    ClientProtocol::new(Protocol::Chat),
                ),
        )?;
        for limit in [0, 64, 1024] {
            let budget = PayloadBudget::with_limit(limit);
            let mut verifier = NativeJsonVerifier::new(
                pair(Protocol::Chat, Protocol::Chat),
                fixture.attempt.clone(),
                fixture.binding.clone(),
                budget.clone(),
            );
            assert!(verifier
                .consume_upstream_json(Cursor::new(&json), &fixture.attempt)
                .is_err());
            let _ = verifier.observe_client_json(&original, &fixture.attempt);
            assert!(verifier.finish().is_err());
            assert_eq!(budget.used(), 0);
            let mut verifier = NativeSseVerifier::new(
                pair(Protocol::Chat, Protocol::Chat),
                fixture.attempt.clone(),
                fixture.binding.clone(),
                budget.clone(),
            );
            assert!(verifier
                .consume_upstream_sse(Cursor::new(&sse), &fixture.attempt)
                .is_err());
            let _ = verifier.consume_client_sse(Cursor::new(&sse), &fixture.attempt);
            assert!(verifier.finish().is_err());
            assert_eq!(budget.used(), 0);
        }
        Ok(())
    }

    #[test]
    fn c5_native_completion_rejects_all_unapproved_protocol_pairs() -> TestResult {
        let fixture = Fixture::new()?;
        let json = serde_json::to_vec(&chat_json())?;
        let sse = chat_sse();
        let protocols = [
            Protocol::Chat,
            Protocol::Responses,
            Protocol::Anthropic,
            Protocol::Gemini,
        ];
        for client in protocols {
            for upstream in protocols {
                if client == Protocol::Chat && upstream == Protocol::Chat {
                    continue;
                }
                let mut json_verifier = NativeJsonVerifier::new(
                    pair(client, upstream),
                    fixture.attempt.clone(),
                    fixture.binding.clone(),
                    fixture.budget.clone(),
                );
                assert!(matches!(
                    json_verifier.consume_upstream_json(Cursor::new(&json), &fixture.attempt),
                    Err(RestoreError::RestoreUnsupported)
                ));
                assert!(json_verifier.finish().is_err());
                let mut sse_verifier = NativeSseVerifier::new(
                    pair(client, upstream),
                    fixture.attempt.clone(),
                    fixture.binding.clone(),
                    fixture.budget.clone(),
                );
                assert!(matches!(
                    sse_verifier.consume_upstream_sse(Cursor::new(&sse), &fixture.attempt),
                    Err(RestoreError::RestoreUnsupported)
                ));
                assert!(sse_verifier.finish().is_err());
            }
        }
        Ok(())
    }
}
