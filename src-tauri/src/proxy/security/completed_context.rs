//! Completed-context, actual-attempt binding, call snapshot, and opaque lineage ownership.
//!
//! Store-issued completed contexts bind store identity, namespace, generation,
//! policy revision, actual provider fingerprint, full dependency lineage,
//! immutable call/reasoning snapshots, and opaque fingerprints.
use super::{
    token_store_owners::{SharedAllocation, StoreVec},
    tokenization::{DlpRequestLease, MappingEntry, TokenError},
};
use hmac::{Hmac, Mac};
use sha2::Sha256;

pub(super) const MAX_CONTEXT_ENTRIES_PER_NAMESPACE: usize = 1024;
pub(super) const MAX_GLOBAL_CONTEXT_ENTRIES: usize = 16384;
pub(super) const MAX_CALLS_PER_CONTEXT: usize = 128;
pub(super) const CONTEXT_ENTRY_METADATA: usize = 512;
pub(super) const CALL_ENTRY_METADATA: usize = 256;
pub(super) const MAX_DELIVERED_CALL_SNAPSHOTS: usize = 4;
pub(super) const OPAQUE_ENTRY_METADATA: usize = 128;

// No Debug/Serialize: bindings and witnesses are private memory-only capabilities.
#[derive(Clone)]
pub(crate) struct ActualAttemptBinding {
    pub(super) store_id: usize,
    pub(super) namespace: u64,
    pub(super) generation: [u8; 16],
    pub(super) policy_revision: u64,
    pub(super) provider_fingerprint: [u8; 32],
    pub(super) reusable_principal: bool,
    pub(super) operation_lineage: Option<OperationLineage>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) struct OperationLineage {
    family_digest: [u8; 32],
    operation: crate::proxy::forwarder::DlpResponsesOperation,
}

impl OperationLineage {
    pub(super) fn issue(
        key: &[u8; 32],
        provider_id: &str,
        protocol: &str,
        auth_principal: &str,
        descriptor: &crate::proxy::forwarder::DlpOperationDescriptor<'_>,
    ) -> Result<Self, TokenError> {
        Ok(Self {
            family_digest: keyed_digest(
                key,
                b"vary-dlp-operation-family-v1\0",
                &[
                    provider_id.as_bytes(),
                    protocol.as_bytes(),
                    auth_principal.as_bytes(),
                    descriptor.base_prefix.as_bytes(),
                    descriptor.query.as_bytes(),
                    descriptor.route_mode.as_bytes(),
                    b"configured-prefix",
                ],
            )?,
            operation: descriptor.operation,
        })
    }

    fn allows_transfer_to(self, target: Self) -> bool {
        use crate::proxy::forwarder::DlpResponsesOperation::{Compact, Responses};
        self.family_digest == target.family_digest
            && matches!(
                (self.operation, target.operation),
                (Responses, Compact) | (Compact, Responses)
            )
    }
}

#[cfg(test)]
pub(super) struct CompletionWitness {
    _private: (),
}

#[cfg(test)]
impl CompletionWitness {
    pub(super) fn from_verified_completion(permitted: bool) -> Result<Self, TokenError> {
        if !permitted {
            return Err(TokenError::ContextUnavailable);
        }
        Ok(Self { _private: () })
    }
}

#[derive(Clone, Copy)]
pub(super) struct CompletedCallSpec<'a> {
    pub call_id: &'a str,
    pub item_type: &'a str,
    pub name: &'a str,
    pub payload: &'a str,
    pub reasoning: Option<&'a str>,
    pub encrypted_reasoning: Option<&'a str>,
}

#[derive(Clone)]
pub(super) struct CompletedContextHandle {
    pub(super) store_id: usize,
    pub(super) namespace: u64,
    pub(super) generation: [u8; 16],
    pub(super) policy_revision: u64,
    pub(super) provider_fingerprint: [u8; 32],
    pub(super) evidence: CompletedEvidenceIdentity,
}

/// Exact delivered variants share the original call identity and immutable completion.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) struct CompletedCallEntry {
    pub(super) call_digest: [u8; 32],
    pub(super) snapshot_digest: [u8; 32],
    pub(super) delivered_digests: [[u8; 32]; MAX_DELIVERED_CALL_SNAPSHOTS],
    pub(super) delivered_len: usize,
}

impl CompletedCallEntry {
    pub(super) fn contains_snapshot(&self, digest: &[u8; 32]) -> bool {
        self.snapshot_digest == *digest
            || self.delivered_digests[..self.delivered_len].contains(digest)
    }
}

pub(super) struct CompletedCallPublications<'a> {
    pub(super) originals: &'a [CompletedCallSpec<'a>],
    pub(super) delivered: &'a [CompletedCallSpec<'a>],
}

/// Protocol evidence is not interchangeable with namespace binding or call pairing.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum CompletedEvidenceIdentity {
    Responses([u8; 32]),
    Chat([u8; 32]),
    Anthropic([u8; 32]),
    Gemini([u8; 32]),
}

pub(super) enum CompletedEvidence {
    Responses(ResponsesEvidence),
    Chat(ChatEvidence),
    Anthropic(AnthropicEvidence),
    Gemini(GeminiEvidence),
}

pub(super) struct ResponsesEvidence {
    pub(super) response_digest: [u8; 32],
    pub(super) calls: Option<StoreVec<CompletedCallEntry>>,
    pub(super) opaques: Option<StoreVec<[u8; 32]>>,
}

/// The MAC binds the exact ordered native array, not a Responses cursor or result body.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) struct NativeDeliveredEvidence {
    pub(super) digest: [u8; 32],
    pub(super) item_count: usize,
}

pub(super) struct ChatEvidence(pub(super) NativeDeliveredEvidence);
pub(super) struct AnthropicEvidence(pub(super) NativeDeliveredEvidence);
pub(super) struct GeminiEvidence(pub(super) NativeDeliveredEvidence);

impl CompletedEvidence {
    pub(super) fn identity(&self) -> CompletedEvidenceIdentity {
        match self {
            Self::Responses(evidence) => {
                CompletedEvidenceIdentity::Responses(evidence.response_digest)
            }
            Self::Chat(evidence) => CompletedEvidenceIdentity::Chat(evidence.0.digest),
            Self::Anthropic(evidence) => CompletedEvidenceIdentity::Anthropic(evidence.0.digest),
            Self::Gemini(evidence) => CompletedEvidenceIdentity::Gemini(evidence.0.digest),
        }
    }

    pub(super) fn matches(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Responses(a), Self::Responses(b)) => {
                a.response_digest == b.response_digest
                    && a.calls.as_ref().map(StoreVec::as_slice).unwrap_or(&[])
                        == b.calls.as_ref().map(StoreVec::as_slice).unwrap_or(&[])
                    && a.opaques.as_ref().map(StoreVec::as_slice).unwrap_or(&[])
                        == b.opaques.as_ref().map(StoreVec::as_slice).unwrap_or(&[])
            }
            (Self::Chat(a), Self::Chat(b)) => a.0 == b.0,
            (Self::Anthropic(a), Self::Anthropic(b)) => a.0 == b.0,
            (Self::Gemini(a), Self::Gemini(b)) => a.0 == b.0,
            _ => false,
        }
    }
}

pub(super) struct CompletedContextRecord {
    pub(super) evidence: CompletedEvidence,
    pub(super) provider_fingerprint: [u8; 32],
    pub(super) operation_lineage: Option<OperationLineage>,
    pub(super) policy_revision: u64,
    pub(super) dependencies: Option<StoreVec<MappingEntry>>,
    pub(super) context_entries: usize,
    pub(super) metadata_bytes: usize,
}

impl CompletedContextRecord {
    pub(super) fn responses_evidence(&self) -> Option<&ResponsesEvidence> {
        match &self.evidence {
            CompletedEvidence::Responses(evidence) => Some(evidence),
            _ => None,
        }
    }

    pub(super) fn response_digest(&self) -> Option<[u8; 32]> {
        self.responses_evidence()
            .map(|evidence| evidence.response_digest)
    }

    pub(super) fn authorizes_attempt(&self, binding: &ActualAttemptBinding) -> bool {
        self.provider_fingerprint == binding.provider_fingerprint
            || self
                .operation_lineage
                .zip(binding.operation_lineage)
                .is_some_and(|(source, target)| source.allows_transfer_to(target))
    }

    /// Native replay requires an exact binding; Responses operation transfer is excluded.
    pub(super) fn validate_native_replay(
        &self,
        binding: &ActualAttemptBinding,
        client: super::restoration_contract::ClientProtocol,
        evidence: &CompletedEvidence,
    ) -> Result<(), TokenError> {
        use crate::proxy::protocol_bridge::Protocol;

        let native_variant = matches!(
            (&self.evidence, client.protocol()),
            (CompletedEvidence::Chat(_), Protocol::Chat)
                | (CompletedEvidence::Anthropic(_), Protocol::Anthropic)
                | (CompletedEvidence::Gemini(_), Protocol::Gemini)
        );
        if !native_variant
            || !binding.reusable_principal
            || binding.operation_lineage.is_some()
            || self.operation_lineage.is_some()
            || self.provider_fingerprint != binding.provider_fingerprint
            || self.policy_revision != binding.policy_revision
            || self.context_entries != 1
            || !self.evidence.matches(evidence)
        {
            return Err(TokenError::ContextUnavailable);
        }
        Ok(())
    }

    pub(super) fn deps_slice(&self) -> &[MappingEntry] {
        self.dependencies
            .as_ref()
            .map(StoreVec::as_slice)
            .unwrap_or(&[])
    }

    pub(super) fn calls_slice(&self) -> &[CompletedCallEntry] {
        self.responses_evidence()
            .and_then(|evidence| evidence.calls.as_ref())
            .map(StoreVec::as_slice)
            .unwrap_or(&[])
    }

    pub(super) fn opaques_slice(&self) -> &[[u8; 32]] {
        self.responses_evidence()
            .and_then(|evidence| evidence.opaques.as_ref())
            .map(StoreVec::as_slice)
            .unwrap_or(&[])
    }

    pub(super) fn find_call(&self, call_digest: &[u8; 32]) -> Option<&CompletedCallEntry> {
        let slice = self.calls_slice();
        slice
            .binary_search_by_key(call_digest, |e| e.call_digest)
            .ok()
            .map(|idx| &slice[idx])
    }

    pub(super) fn contains_opaque(&self, opaque_digest: &[u8; 32]) -> bool {
        self.opaques_slice().binary_search(opaque_digest).is_ok()
    }
}

pub(super) type ContextEntry = SharedAllocation<CompletedContextRecord>;

pub(super) fn valid_external_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 256 && !id.chars().any(|ch| ch.is_whitespace() || ch.is_control())
}

pub(super) fn keyed_digest(
    key: &[u8; 32],
    domain: &[u8],
    parts: &[&[u8]],
) -> Result<[u8; 32], TokenError> {
    let mut mac = Hmac::<Sha256>::new_from_slice(key).map_err(|_| TokenError::Unavailable)?;
    mac.update(domain);
    for part in parts {
        let len = u64::try_from(part.len()).map_err(|_| TokenError::CapacityExceeded)?;
        mac.update(&len.to_be_bytes());
        mac.update(part);
    }
    Ok(mac.finalize().into_bytes().into())
}

pub(super) fn provider_binding_digest(
    key: &[u8; 32],
    provider_id: &str,
    protocol: &str,
    target_url: &str,
    auth_principal: Option<&str>,
) -> Result<[u8; 32], TokenError> {
    keyed_digest(
        key,
        b"vary-dlp-provider-binding-v1\0",
        &[
            provider_id.as_bytes(),
            protocol.as_bytes(),
            target_url.as_bytes(),
            auth_principal.unwrap_or("").as_bytes(),
        ],
    )
}

pub(super) fn response_id_digest(
    key: &[u8; 32],
    response_id: &str,
) -> Result<[u8; 32], TokenError> {
    keyed_digest(key, b"vary-dlp-response-id-v1\0", &[response_id.as_bytes()])
}

pub(super) fn call_id_digest(key: &[u8; 32], call_id: &str) -> Result<[u8; 32], TokenError> {
    keyed_digest(key, b"vary-dlp-call-id-v1\0", &[call_id.as_bytes()])
}

pub(super) fn call_snapshot_digest(
    key: &[u8; 32],
    spec: &CompletedCallSpec<'_>,
) -> Result<[u8; 32], TokenError> {
    let has_reasoning: &[u8] = if spec.reasoning.is_some() { &[1] } else { &[0] };
    let has_encrypted: &[u8] = if spec.encrypted_reasoning.is_some() {
        &[1]
    } else {
        &[0]
    };
    keyed_digest(
        key,
        b"vary-dlp-call-snapshot-v1\0",
        &[
            spec.call_id.as_bytes(),
            spec.item_type.as_bytes(),
            spec.name.as_bytes(),
            spec.payload.as_bytes(),
            has_reasoning,
            spec.reasoning.unwrap_or("").as_bytes(),
            has_encrypted,
            spec.encrypted_reasoning.unwrap_or("").as_bytes(),
        ],
    )
}

pub(super) fn opaque_content_digest(
    key: &[u8; 32],
    encrypted_content: &str,
) -> Result<[u8; 32], TokenError> {
    keyed_digest(
        key,
        b"vary-dlp-opaque-content-v1\0",
        &[encrypted_content.as_bytes()],
    )
}

/// Inspect a bounded ordered array without copying or retaining native plaintext.
/// Namespace keys are derived independently of mapping and provider-binding keys.
pub(super) fn native_delivered_evidence(
    store_key: &[u8; 32],
    binding: &ActualAttemptBinding,
    client: super::restoration_contract::ClientProtocol,
    ingress: super::restoration_contract::IngressNamespace,
    delivered: &serde_json::Value,
    fragment_limit: usize,
) -> Result<CompletedEvidence, TokenError> {
    use crate::proxy::protocol_bridge::Protocol;
    use std::io::{self, Write};

    struct DigestWriter<'a> {
        mac: Option<&'a mut Hmac<Sha256>>,
        bytes: usize,
        limit: usize,
    }
    impl Write for DigestWriter<'_> {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            let total = self
                .bytes
                .checked_add(bytes.len())
                .filter(|total| *total <= self.limit)
                .ok_or_else(|| io::Error::other("Native evidence capacity exceeded."))?;
            if let Some(mac) = self.mac.as_mut() {
                mac.update(bytes);
            }
            self.bytes = total;
            Ok(bytes.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    fn field(mac: &mut Hmac<Sha256>, bytes: &[u8]) -> Result<(), TokenError> {
        let len = u64::try_from(bytes.len()).map_err(|_| TokenError::CapacityExceeded)?;
        mac.update(&len.to_be_bytes());
        mac.update(bytes);
        Ok(())
    }

    let protocol: &[u8] = match client.protocol() {
        Protocol::Chat => b"openai_chat",
        Protocol::Anthropic => b"anthropic",
        Protocol::Gemini => b"gemini_native",
        Protocol::Responses => return Err(TokenError::ContextUnavailable),
    };
    let items = delivered.as_array().ok_or(TokenError::ContextUnavailable)?;
    if items.is_empty() || items.len() > MAX_CALLS_PER_CONTEXT {
        return Err(TokenError::CapacityExceeded);
    }
    // Count every serialized native item before the MAC pass. The bracket and
    // separator arithmetic is checked, and every item obeys the fragment ceiling.
    let mut array_bytes = 2usize;
    for (index, item) in items.iter().enumerate() {
        let mut counter = DigestWriter {
            mac: None,
            bytes: 0,
            limit: fragment_limit,
        };
        serde_json::to_writer(&mut counter, item).map_err(|_| TokenError::CapacityExceeded)?;
        array_bytes = array_bytes
            .checked_add(counter.bytes)
            .and_then(|total| total.checked_add(usize::from(index > 0)))
            .ok_or(TokenError::CapacityExceeded)?;
    }
    let namespace_key = keyed_digest(
        store_key,
        b"",
        &[
            b"native-evidence-key-v1\0",
            &binding.namespace.to_be_bytes(),
            &binding.generation,
        ],
    )?;
    let mut mac =
        Hmac::<Sha256>::new_from_slice(&namespace_key).map_err(|_| TokenError::Unavailable)?;
    field(&mut mac, b"native-delivered-v1\0")?;
    field(&mut mac, protocol)?;
    field(
        &mut mac,
        match ingress {
            super::restoration_contract::IngressNamespace::Direct => b"direct",
            super::restoration_contract::IngressNamespace::Gateway => b"gateway",
        },
    )?;
    mac.update(
        &u64::try_from(array_bytes)
            .map_err(|_| TokenError::CapacityExceeded)?
            .to_be_bytes(),
    );
    {
        let mut writer = DigestWriter {
            mac: Some(&mut mac),
            bytes: 0,
            limit: array_bytes,
        };
        serde_json::to_writer(&mut writer, delivered).map_err(|_| TokenError::CapacityExceeded)?;
        if writer.bytes != array_bytes {
            return Err(TokenError::ContextUnavailable);
        }
    }
    let evidence = NativeDeliveredEvidence {
        digest: mac.finalize().into_bytes().into(),
        item_count: items.len(),
    };
    Ok(match client.protocol() {
        Protocol::Chat => CompletedEvidence::Chat(ChatEvidence(evidence)),
        Protocol::Anthropic => CompletedEvidence::Anthropic(AnthropicEvidence(evidence)),
        Protocol::Gemini => CompletedEvidence::Gemini(GeminiEvidence(evidence)),
        Protocol::Responses => return Err(TokenError::ContextUnavailable),
    })
}

/// Replay has no adapter-owned budget, so bound traversal before serialization and MAC work.
/// Reuse the publication evidence format without copying native strings or widening authority.
pub(super) fn native_replay_evidence(
    store_key: &[u8; 32],
    binding: &ActualAttemptBinding,
    client: super::restoration_contract::ClientProtocol,
    ingress: super::restoration_contract::IngressNamespace,
    delivered: &serde_json::Value,
    fragment_limit: usize,
) -> Result<CompletedEvidence, TokenError> {
    use serde_json::Value;

    fn inspect(value: &Value, depth: usize, remaining: &mut usize) -> Result<(), TokenError> {
        if depth > 64 {
            return Err(TokenError::CapacityExceeded);
        }
        *remaining = remaining
            .checked_sub(1)
            .ok_or(TokenError::CapacityExceeded)?;
        match value {
            Value::String(text) => {
                *remaining = remaining
                    .checked_sub(text.len())
                    .ok_or(TokenError::CapacityExceeded)?;
            }
            Value::Array(items) => {
                for item in items {
                    inspect(item, depth + 1, remaining)?;
                }
            }
            Value::Object(fields) => {
                for (key, value) in fields {
                    *remaining = remaining
                        .checked_sub(key.len())
                        .ok_or(TokenError::CapacityExceeded)?;
                    inspect(value, depth + 1, remaining)?;
                }
            }
            _ => {}
        }
        Ok(())
    }

    if client.protocol() == crate::proxy::protocol_bridge::Protocol::Responses {
        return Err(TokenError::ContextUnavailable);
    }
    let items = delivered.as_array().ok_or(TokenError::ContextUnavailable)?;
    if items.is_empty() {
        return Err(TokenError::ContextUnavailable);
    }
    if items.len() > MAX_CALLS_PER_CONTEXT {
        return Err(TokenError::CapacityExceeded);
    }
    let mut remaining = 4 * 1024 * 1024;
    inspect(delivered, 0, &mut remaining)?;
    native_delivered_evidence(
        store_key,
        binding,
        client,
        ingress,
        delivered,
        fragment_limit,
    )
}

impl DlpRequestLease {
    pub(super) fn fork_attempt(&self) -> Result<DlpRequestLease, TokenError> {
        self.fork_attempt_extracted()
    }

    pub(super) fn bind_actual_attempt(
        &self,
        provider_id: &str,
        protocol: &str,
        target_url: &str,
        auth_principal: Option<&str>,
    ) -> Result<ActualAttemptBinding, TokenError> {
        self.bind_actual_attempt_extracted(provider_id, protocol, target_url, auth_principal)
    }

    pub(super) fn bind_actual_operation_attempt(
        &self,
        provider_id: &str,
        protocol: &str,
        target_url: &str,
        auth_principal: Option<&str>,
        operation: Option<&crate::proxy::forwarder::DlpOperationDescriptor<'_>>,
    ) -> Result<ActualAttemptBinding, TokenError> {
        self.bind_actual_operation_attempt_extracted(
            provider_id,
            protocol,
            target_url,
            auth_principal,
            operation,
        )
    }

    #[cfg(test)]
    pub(super) fn publish_completed_context(
        &self,
        binding: &ActualAttemptBinding,
        witness: &CompletionWitness,
        response_id: &str,
        output_tokens: &[&str],
        calls: &[CompletedCallSpec<'_>],
        opaques: &[&str],
    ) -> Result<CompletedContextHandle, TokenError> {
        self.publish_completed_context_variants(
            binding,
            witness,
            response_id,
            output_tokens,
            CompletedCallPublications {
                originals: calls,
                delivered: &[],
            },
            opaques,
        )
    }

    #[cfg(test)]
    pub(super) fn publish_completed_context_variants(
        &self,
        binding: &ActualAttemptBinding,
        _witness: &CompletionWitness,
        response_id: &str,
        output_tokens: &[&str],
        calls: CompletedCallPublications<'_>,
        opaques: &[&str],
    ) -> Result<CompletedContextHandle, TokenError> {
        self.publish_completed_context_extracted(
            binding,
            response_id,
            output_tokens,
            calls,
            opaques,
        )
    }

    /// Inspect exact native delivery without importing dependencies or cursor authority.
    pub(super) fn verify_native_delivered(
        &self,
        binding: &ActualAttemptBinding,
        client: super::restoration_contract::ClientProtocol,
        delivered: &serde_json::Value,
    ) -> Result<CompletedContextHandle, TokenError> {
        self.verify_native_delivered_extracted(binding, client, delivered)
    }

    /// Explicitly import only the immutable dependencies of exact visible native evidence.
    pub(super) fn import_native_completed_context(
        &mut self,
        binding: &ActualAttemptBinding,
        client: super::restoration_contract::ClientProtocol,
        handle: &CompletedContextHandle,
    ) -> Result<(), TokenError> {
        self.import_native_completed_context_extracted(binding, client, handle)
    }

    pub(super) fn resume_completed_context(
        &mut self,
        binding: &ActualAttemptBinding,
        previous_response_id: Option<&str>,
        required_calls: &[CompletedCallSpec<'_>],
        required_opaques: &[&str],
    ) -> Result<(), TokenError> {
        let mut reason = None;
        self.resume_completed_context_extracted(
            binding,
            previous_response_id,
            required_calls,
            required_opaques,
            &mut reason,
        )
    }

    pub(super) fn resume_completed_context_diagnosed(
        &mut self,
        binding: &ActualAttemptBinding,
        previous_response_id: Option<&str>,
        required_calls: &[CompletedCallSpec<'_>],
        required_opaques: &[&str],
        reason: &mut Option<super::restore_diagnostics::ContextResumeReason>,
    ) -> Result<(), TokenError> {
        let result = self.resume_completed_context_extracted(
            binding,
            previous_response_id,
            required_calls,
            required_opaques,
            reason,
        );
        if result == Err(TokenError::ContextUnavailable) && reason.is_none() {
            *reason = Some(super::restore_diagnostics::ContextResumeReason::Unknown);
        }
        result
    }

    pub(super) fn resume_completed_tokens(
        &mut self,
        binding: &ActualAttemptBinding,
        referenced_tokens: &[&str],
    ) -> Result<(), TokenError> {
        self.resume_completed_tokens_extracted(binding, referenced_tokens)
    }

    pub(super) fn verify_output_call_ids(
        &self,
        binding: &ActualAttemptBinding,
        previous_response_id: Option<&str>,
        orphan_output_call_ids: &[&str],
    ) -> Result<(), TokenError> {
        self.verify_output_call_ids_extracted(binding, previous_response_id, orphan_output_call_ids)
    }
}
