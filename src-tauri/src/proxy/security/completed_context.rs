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
pub(super) const OPAQUE_ENTRY_METADATA: usize = 128;

// No Debug/Serialize: bindings and witnesses are private memory-only capabilities.
#[derive(Clone)]
pub(super) struct ActualAttemptBinding {
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

pub(super) struct CompletionWitness {
    _private: (),
}

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
    pub(super) response_digest: [u8; 32],
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) struct CompletedCallEntry {
    pub(super) call_digest: [u8; 32],
    pub(super) snapshot_digest: [u8; 32],
}

pub(super) struct CompletedContextRecord {
    pub(super) response_digest: [u8; 32],
    pub(super) provider_fingerprint: [u8; 32],
    pub(super) operation_lineage: Option<OperationLineage>,
    pub(super) policy_revision: u64,
    pub(super) dependencies: Option<StoreVec<MappingEntry>>,
    pub(super) calls: Option<StoreVec<CompletedCallEntry>>,
    pub(super) opaques: Option<StoreVec<[u8; 32]>>,
    pub(super) context_entries: usize,
    pub(super) metadata_bytes: usize,
}

impl CompletedContextRecord {
    pub(super) fn authorizes_attempt(&self, binding: &ActualAttemptBinding) -> bool {
        self.provider_fingerprint == binding.provider_fingerprint
            || self
                .operation_lineage
                .zip(binding.operation_lineage)
                .is_some_and(|(source, target)| source.allows_transfer_to(target))
    }

    pub(super) fn deps_slice(&self) -> &[MappingEntry] {
        self.dependencies
            .as_ref()
            .map(StoreVec::as_slice)
            .unwrap_or(&[])
    }

    pub(super) fn calls_slice(&self) -> &[CompletedCallEntry] {
        self.calls.as_ref().map(StoreVec::as_slice).unwrap_or(&[])
    }

    pub(super) fn opaques_slice(&self) -> &[[u8; 32]] {
        self.opaques.as_ref().map(StoreVec::as_slice).unwrap_or(&[])
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

    pub(super) fn publish_completed_context(
        &self,
        binding: &ActualAttemptBinding,
        _witness: &CompletionWitness,
        response_id: &str,
        output_tokens: &[&str],
        calls: &[CompletedCallSpec<'_>],
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

    pub(super) fn resume_completed_context(
        &mut self,
        binding: &ActualAttemptBinding,
        previous_response_id: Option<&str>,
        required_calls: &[CompletedCallSpec<'_>],
        required_opaques: &[&str],
    ) -> Result<(), TokenError> {
        self.resume_completed_context_extracted(
            binding,
            previous_response_id,
            required_calls,
            required_opaques,
        )
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
