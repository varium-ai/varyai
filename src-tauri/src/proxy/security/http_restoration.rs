//! Private HTTP restoration gateway, actual-attempt preparation, and response wrapper.
//!
//! No `unwrap()`/`expect()` is permitted here. All persistent rule backing uses
//! `StoreVec`/`StoreText`/`SharedAllocation` charged to the store's ledger, and
//! all transient request/response buffers charge `PayloadBudget`.
use axum::{
    body::Body,
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::Response,
};
use bytes::Bytes;
use futures::stream::{Stream, StreamExt};
use serde_json::{json, Value};

use crate::app_config::AppType;
use crate::proxy::session::extract_codex_restoration_session_id;

use super::{
    completed_context::{valid_external_id, ActualAttemptBinding, CompletedCallSpec},
    completion_evidence::CompletionObservation,
    completion_json::JsonCompletionProtocol,
    extractor::extract_payload_texts,
    history_provenance::{
        history_target_for_path, visit_references_with_policy, visit_request_references,
        DlpHistoryMapping, DlpHistoryProjection, HistoryLeafPolicy,
    },
    inspect_and_intercept_mut_with_headers,
    output_text_matching::visit_complete_references as visit_token_references,
    payload_budget::PayloadBudget,
    payload_buffer::PayloadBuffer,
    payload_json::{JsonStringReplacement, PayloadJson},
    payload_metadata::PayloadVec,
    payload_text::PayloadText,
    protocol_adapters::responses::{self as responses_adapter, RequestHistoryClassification},
    protocol_adapters::responses::{
        stage_json_response_bound, ArgumentNormalizationMode, RestoreError, SseRestorationState,
    },
    request_carriers::CodexRequestCarriers,
    request_detection::detect_spans_with_budget,
    request_plan::{EffectiveAction, PlanError},
    restoration_audit::{PendingRequestAudit, RequestAuditState},
    restoration_core::RequestAuthority,
    restore_diagnostics::{ContextCheckpoint, RestoreStage},
    runtime::{admit_security_runtime, SecurityRuntimeConfig},
    token_store_owners::{SharedAllocation, StoreText, StoreVec},
    tokenization::{DlpRequestLease, DlpTokenStore, TokenError},
};
mod completion_verifier;
mod native_chat;
#[cfg(test)]
pub(in crate::proxy::security) use native_chat::capture_test_support;
mod native_driver;
#[cfg(test)]
pub(in crate::proxy::security) mod test_transport;
pub(in crate::proxy::security) use completion_verifier::{
    VerifiedCompletion, VerifiedJsonCompletion, VerifiedNativeJson, VerifiedNativeSse,
};

#[cfg(test)]
use super::{
    completed_context::CompletionWitness, protocol_adapters::responses::stage_json_response,
};

struct GatewayState {
    store: DlpTokenStore,
    rules: StoreVec<StoreText>,
    budget: PayloadBudget,
}

#[derive(Clone, Default)]
pub(crate) struct RestorationGateway {
    inner: Option<Result<SharedAllocation<GatewayState>, TokenError>>,
}

impl RestorationGateway {
    pub(crate) fn disabled() -> Self {
        Self { inner: None }
    }

    pub(crate) fn new_default() -> Self {
        Self::with_store_and_rules(DlpTokenStore::new(), &[], PayloadBudget::default())
    }

    pub(crate) fn is_enabled(&self) -> bool {
        self.inner.is_some()
    }

    pub(super) fn with_store_and_rules(
        store: DlpTokenStore,
        rule_ids: &[&str],
        budget: PayloadBudget,
    ) -> Self {
        let state = (|| {
            let ledger = store.allocation_ledger()?;
            let mut rules = if rule_ids.is_empty() {
                StoreVec::new(ledger.clone(), 4096)
            } else {
                StoreVec::with_capacity(rule_ids.len(), ledger.clone(), 4096)?
            };
            for &id in rule_ids {
                let text = StoreText::try_from_str(id, ledger.clone(), 256)?;
                rules.push_without_growth(text)?;
            }
            SharedAllocation::new(
                GatewayState {
                    store,
                    rules,
                    budget,
                },
                ledger,
            )
        })();
        Self { inner: Some(state) }
    }

    #[cfg(test)]
    pub(super) fn with_test_store_and_rules(
        store: DlpTokenStore,
        rule_ids: &[&str],
        budget: PayloadBudget,
    ) -> Self {
        Self::with_store_and_rules(store, rule_ids, budget)
    }

    #[cfg(test)]
    pub(crate) fn with_rules(rule_ids: &[&str], budget_bytes: usize) -> Self {
        Self::with_store_and_rules(
            DlpTokenStore::new(),
            rule_ids,
            PayloadBudget::with_limit(budget_bytes),
        )
    }

    #[cfg(test)]
    pub(crate) fn with_test_rules(rule_ids: &[&str], budget_bytes: usize) -> Self {
        Self::with_rules(rule_ids, budget_bytes)
    }

    fn state(&self) -> Result<Option<&GatewayState>, TokenError> {
        match &self.inner {
            None => Ok(None),
            Some(Ok(alloc)) => Ok(Some(alloc.get()?)),
            Some(Err(err)) => Err(*err),
        }
    }
}

pub(crate) struct DlpHttpRequest {
    authority: RequestAuthority,
    runtime: SecurityRuntimeConfig,
    restore_rules: StoreVec<StoreText>,
    source_body: PayloadJson,
    pretokenized: bool,
    native_protocol: Option<crate::proxy::protocol_bridge::Protocol>,
    endpoint_is_compact: bool,
    audit_state: std::sync::Arc<RequestAuditState>,
    initial_audit: std::sync::Arc<PendingRequestAudit>,
}

// Retain alongside the independently rebuilt source tree, and drop the tree first.
// No clone/debug/serialization exposes or duplicates its reservation.
pub(crate) struct DlpSourceBodyOwner {
    _reservation: super::payload_budget::Reservation,
    // Test-only drop witness runs after the actual reservation is released.
    #[cfg(test)]
    _probe: source_copy_test_support::OwnerProbe,
}

// Allocation authority only: this owner grants no token/restoration authority.
pub(crate) struct DlpMappingBodyOwner {
    _reservation: super::payload_budget::Reservation,
    #[cfg(test)]
    _probe: mapping_copy_test_support::OwnerProbe,
}

// Retains only the moved prepared input allocation. Converter output and later
// growth need their own accounting; this owner grants no restoration authority.
pub(crate) struct DlpTransformInputOwner {
    _reservation: super::payload_budget::Reservation,
}

// Allocation-only authority for the independently rebuilt outbound tail.
// Keep the final JSON alive no longer than this non-Clone owner.
pub(crate) struct DlpOutboundTailOwner {
    _reservation: super::payload_budget::Reservation,
}

// Allocation-only authority for the independent Chat prompt-cache candidate.
// The moved explicit key remains covered separately by the transform-input owner.
// Allocation-only authority for the independent request namespace candidate.
// It grants no authority to later xAI mutations or response restoration tables.
pub(crate) struct DlpNamespaceOwner {
    _reservation: super::payload_budget::Reservation,
}

pub(crate) struct DlpPromptCacheOwner {
    _reservation: super::payload_budget::Reservation,
}

pub(crate) struct DlpHistoryTransform<'a> {
    lease: &'a DlpRequestLease,
    binding: &'a ActualAttemptBinding,
    origin: &'a Value,
}
impl DlpHistoryTransform<'_> {
    pub(crate) fn projection<'a>(&'a self, body: &'a Value) -> Option<DlpHistoryProjection<'a>> {
        let mut projection = self.lease.history_projection(self.binding, body)?;
        projection.restrict_to_origin(self.origin);
        Some(projection)
    }
}

pub(crate) struct DlpPreparedAttempt {
    pub(super) lease: DlpRequestLease,
    pub(super) binding: ActualAttemptBinding,
    pub(super) budget: PayloadBudget,
    pub(crate) endpoint_is_compact: bool,
    upstream_json_protocol: Option<JsonCompletionProtocol>,
    provenance: super::restoration_core::AttemptProvenance,
    json_completion: completion_verifier::JsonCompletionVerifier,
    audit_state: std::sync::Arc<RequestAuditState>,
    initial_audit: std::sync::Arc<PendingRequestAudit>,
    pending_audit: PendingRequestAudit,
    upstream_payload_owner: Option<super::UpstreamPayloadOwner>,
    history_mapping: Option<DlpHistoryMapping>,
    native_history_digest: Option<[u8; 32]>,
    native_protocol: Option<crate::proxy::protocol_bridge::Protocol>,
    native_original: Option<native_chat::OriginalCapture>,
    native_error_safety: Option<super::upstream_error::NativeErrorSafety>,
    sse_evidence: std::sync::Mutex<Option<PreparedSseEvidence>>,
}
impl DlpPreparedAttempt {
    pub(crate) async fn guard_upstream_response(
        &mut self,
        response: crate::proxy::hyper_client::ProxyResponse,
    ) -> Result<crate::proxy::hyper_client::ProxyResponse, DlpForwardError> {
        if !self.has_restoration_lease() && self.native_protocol.is_none() {
            return Ok(response);
        }
        // Capture before eager JSON guarding so original evidence comes from the
        // actual transport, not from a decoded/rebuilt response or terminal marker.
        let response = if self.native_protocol.is_some() {
            let (response, capture) = native_chat::capture_original(
                response,
                &self.budget,
                &self.provenance,
                &self.binding,
                self.native_protocol
                    .ok_or(DlpForwardError::RestoreUnsupported)?,
                self.native_error_safety.take(),
            )?;
            self.native_original = Some(capture);
            response
        } else {
            response
        };
        let guarded =
            super::upstream_guard::guard_upstream_response(response, &self.budget).await?;
        self.upstream_payload_owner = Some(guarded.owner);
        Ok(guarded.response)
    }

    pub(crate) fn upstream_payload_owner(&self) -> Option<super::UpstreamPayloadOwner> {
        self.upstream_payload_owner.clone()
    }

    /// The wire body is an independent allocation, even when the JSON is already owned.
    /// Return Bytes carrying its charge through transport clones and cancellation.
    pub(crate) fn encode_outbound_body(
        &self,
        body: &Value,
    ) -> Result<Option<Bytes>, DlpForwardError> {
        self.validate_final_send_history(body)?;
        if !self.has_restoration_lease() && self.native_protocol.is_none() {
            return Ok(None);
        }
        encode_outbound_with_budget(body, self.budget.clone()).map(Some)
    }

    /// Encode the independent xAI response rewrite without granting completion authority.
    /// Empty leases keep the ordinary serializer and acquire no payload owner.
    pub(crate) fn encode_response_rewrite_body(
        &self,
        body: &Value,
    ) -> Result<Option<Bytes>, RestoreError> {
        if !self.has_restoration_lease() {
            return Ok(None);
        }
        encode_outbound_with_budget(body, self.budget.clone())
            .map(Some)
            .map_err(|_| RestoreError::RestoreFailed)
    }

    #[cfg(test)]
    pub(super) fn audit_events_for_test(
        &self,
        provider_id: &str,
        request_id: Option<&str>,
    ) -> Vec<super::aggregation::PendingAuditEvent> {
        let mut events = Vec::new();
        self.audit_state.record_with(
            &self.initial_audit,
            provider_id,
            request_id,
            false,
            |event| events.push(event),
        );
        self.audit_state.record_with(
            &self.pending_audit,
            provider_id,
            request_id,
            false,
            |event| events.push(event),
        );
        events
    }

    pub(crate) fn record_permitted_attempt(&self, provider_id: &str, request_id: Option<&str>) {
        self.audit_state
            .record_permitted_attempt(&self.initial_audit, provider_id, request_id);
        self.audit_state
            .record_permitted_attempt(&self.pending_audit, provider_id, request_id);
    }

    /// Recheck the exact transport authority after all header/account overrides.
    /// Operation-family continuity never authorizes a different actual send.
    pub(crate) fn validate_final_send_binding(
        &self,
        provider_id: &str,
        protocol: &str,
        target_url: &str,
        auth_principal: Option<&str>,
    ) -> Result<(), DlpForwardError> {
        let actual = self
            .lease
            .bind_actual_attempt(provider_id, protocol, target_url, auth_principal)
            .map_err(DlpForwardError::from_token_error)?;
        if actual.store_id != self.binding.store_id
            || actual.namespace != self.binding.namespace
            || actual.generation != self.binding.generation
            || actual.policy_revision != self.binding.policy_revision
            || actual.provider_fingerprint != self.binding.provider_fingerprint
            || actual.reusable_principal != self.binding.reusable_principal
        {
            return Err(DlpForwardError::ContextUnavailable);
        }
        Ok(())
    }

    pub(crate) fn validate_final_send_history(&self, body: &Value) -> Result<(), DlpForwardError> {
        native_chat::validate_native_history_digest(self.native_history_digest, body)?;
        if self
            .history_mapping
            .as_ref()
            .is_some_and(|mapping| !mapping.validates_all(&self.lease, &self.binding, body))
        {
            return Err(DlpForwardError::ContextUnavailable);
        }
        Ok(())
    }

    /// Diagnostic suppression is conservative and grants no forwarding authority.
    pub(crate) fn suppress_payload_diagnostics(&self) -> bool {
        true
    }

    pub(crate) fn has_restoration_lease(&self) -> bool {
        !self.lease.is_empty()
    }
}

pub(super) fn encode_outbound_with_budget(
    body: &Value,
    budget: PayloadBudget,
) -> Result<Bytes, DlpForwardError> {
    let buffer = super::request_preparation::encode_candidate(body, budget)
        .map_err(DlpForwardError::from_token_error)?;
    Ok(bytes_with_owner(buffer, ()))
}

pub(crate) enum RestorationAdmission {
    Passthrough,
    Intercepted(Response),
    Admitted(Box<DlpHttpRequest>),
}

pub(super) const CLIENT_PROCESSING_ERROR_MESSAGE: &str =
    "The local proxy could not process the request. Please retry.";

/// Structural reasons a supported request cannot use reversible DLP. These are
/// user-actionable categories only: never a rule name, matched value or internal label.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RestoreUnsupportedCause {
    /// Route, method or response framing outside the enabled native cells.
    UnsupportedEndpoint,
    /// Signed or otherwise immutable content that a mandatory patch would have to edit.
    ImmutableSignedUnit,
}

impl RestoreUnsupportedCause {
    pub(crate) const fn client_message(self) -> &'static str {
        match self {
            Self::UnsupportedEndpoint => {
                "This route or response framing is not supported by reversible DLP. Supported direct routes are /chat/completions, /v1/messages, generateContent and streamGenerateContent with alt=sse. Use one of them, or disable reversible DLP for this provider."
            }
            Self::ImmutableSignedUnit => {
                "This request contains signed or otherwise immutable content that reversible DLP cannot modify. Remove that content, or disable reversible DLP for this provider."
            }
        }
    }
}
pub(super) const CLIENT_PROCESSING_ERROR_CODE: &str = "proxy_processing_failed";
/// Client-visible code for a route or framing that reversible DLP does not support.
pub(super) const CLIENT_UNSUPPORTED_ENDPOINT_CODE: &str = "dlp_restore_unsupported";
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DlpForwardError {
    ContextUnavailable,
    PolicyConflict,
    RestoreUnsupported,
    RestoreUnsupportedFor(RestoreUnsupportedCause),
    RedactionFailed,
    Unavailable,
    CapacityExceeded,
    TokenCollision,
    Blocked,
    RestoreFailed,
    RestoreFailedAt(super::restore_diagnostics::RestoreStage),
    /// Status-only upstream refusal; its untrusted body is never retained or decoded.
    UpstreamStatus(StatusCode),
}

/// HTTP-private decision, called only at the two optional-history boundaries below.
/// Other context errors and mandatory processing never enter this whitelist.
fn optional_history_miss(error: DlpForwardError) -> Result<(), DlpForwardError> {
    match error {
        DlpForwardError::ContextUnavailable => Ok(()),
        other => Err(other),
    }
}

impl DlpForwardError {
    /// Preserve the original refusal; only fixed request-side boundaries are logged.
    pub(super) fn at_context_checkpoint(self, checkpoint: ContextCheckpoint) -> Self {
        if self == Self::ContextUnavailable {
            super::restore_diagnostics::log_context_failure(checkpoint);
        }
        self
    }

    pub(super) fn from_token_error(err: TokenError) -> Self {
        match err {
            TokenError::Unavailable => Self::Unavailable,
            TokenError::ContextUnavailable => Self::ContextUnavailable,
            TokenError::CapacityExceeded => Self::CapacityExceeded,
            TokenError::TokenCollision => Self::TokenCollision,
            TokenError::RedactionFailed => Self::RedactionFailed,
        }
    }

    /// Attributable form for the direct native path only. Legacy Responses and the
    /// conversion facades keep the generic projection so their accepted contract holds.
    pub(super) fn from_plan_error_native(err: PlanError) -> Self {
        match err {
            PlanError::ImmutableContent => {
                Self::RestoreUnsupportedFor(RestoreUnsupportedCause::ImmutableSignedUnit)
            }
            other => Self::from_plan_error(other),
        }
    }

    pub(super) fn from_plan_error(err: PlanError) -> Self {
        match err {
            PlanError::Blocked => Self::Blocked,
            PlanError::PolicyConflict => Self::PolicyConflict,
            PlanError::Unsupported => Self::RestoreUnsupported,
            PlanError::Token(te) => Self::from_token_error(te),
            // Non-native callers keep the generic projection for this cause.
            PlanError::ImmutableContent => Self::RestoreUnsupported,
        }
    }
    pub(crate) fn status_code(self) -> StatusCode {
        match self {
            Self::UpstreamStatus(status) => status,
            Self::RestoreFailed | Self::RestoreFailedAt(_) => StatusCode::BAD_GATEWAY,
            Self::ContextUnavailable
            | Self::PolicyConflict
            | Self::RestoreUnsupported
            | Self::RestoreUnsupportedFor(_)
            | Self::RedactionFailed
            | Self::Blocked => StatusCode::BAD_REQUEST,
            Self::Unavailable | Self::CapacityExceeded | Self::TokenCollision => {
                StatusCode::SERVICE_UNAVAILABLE
            }
        }
    }

    pub(crate) fn error_type(self) -> &'static str {
        match self {
            Self::UpstreamStatus(_) => "upstream_error",
            Self::RestoreFailed | Self::RestoreFailedAt(_) => "server_error",
            Self::ContextUnavailable
            | Self::PolicyConflict
            | Self::RestoreUnsupported
            | Self::RestoreUnsupportedFor(_)
            | Self::RedactionFailed
            | Self::Blocked => "invalid_request_error",
            Self::Unavailable | Self::CapacityExceeded | Self::TokenCollision => "server_error",
        }
    }

    pub(crate) fn code(self) -> &'static str {
        match self {
            Self::UpstreamStatus(_) => "upstream_error",
            Self::RestoreFailed | Self::RestoreFailedAt(_) => "dlp_restore_failed",
            Self::ContextUnavailable => "dlp_context_unavailable",
            Self::PolicyConflict => "dlp_policy_conflict",
            Self::RestoreUnsupported | Self::RestoreUnsupportedFor(_) => "dlp_restore_unsupported",
            Self::RedactionFailed => "dlp_redaction_failed",
            Self::Unavailable => "dlp_unavailable",
            Self::CapacityExceeded => "dlp_capacity_exceeded",
            Self::TokenCollision => "dlp_token_collision",
            Self::Blocked => "dlp_blocked",
        }
    }

    pub(crate) fn message(self) -> &'static str {
        match self {
            Self::UpstreamStatus(_) => {
                "Upstream request failed. The upstream error body was withheld for safety."
            }
            Self::RestoreFailed => super::restore_diagnostics::RestoreStage::Unknown.message(),
            Self::RestoreFailedAt(stage) => stage.message(),
            Self::ContextUnavailable => "Reversible DLP session or cursor context is unavailable.",
            Self::PolicyConflict => {
                "Reversible DLP token conflicts with an irreversible security rule."
            }
            Self::RestoreUnsupported => {
                "Reversible DLP tokenization is not supported for this endpoint or carrier."
            }
            Self::RestoreUnsupportedFor(cause) => cause.client_message(),
            Self::RedactionFailed => "Reversible DLP request preparation failed.",
            Self::Unavailable => "Reversible DLP service is temporarily unavailable.",
            Self::CapacityExceeded => "Reversible DLP capacity limit exceeded.",
            Self::TokenCollision => "Reversible DLP token digest collision detected.",
            Self::Blocked => "Request blocked by local security policy.",
        }
    }

    pub(crate) fn client_status_code(self) -> StatusCode {
        match self {
            Self::UpstreamStatus(status) => status,
            // A caused rejection is caller-actionable and nothing was sent upstream.
            Self::Blocked | Self::RestoreUnsupportedFor(_) => StatusCode::BAD_REQUEST,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    pub(crate) fn client_error_type(self) -> &'static str {
        match self {
            Self::UpstreamStatus(_) => "upstream_error",
            Self::Blocked | Self::RestoreUnsupportedFor(_) => "invalid_request_error",
            _ => "server_error",
        }
    }

    pub(crate) fn client_code(self) -> &'static str {
        match self {
            Self::UpstreamStatus(_) => "upstream_error",
            Self::Blocked => "dlp_blocked",
            Self::RestoreUnsupportedFor(_) => CLIENT_UNSUPPORTED_ENDPOINT_CODE,
            _ => CLIENT_PROCESSING_ERROR_CODE,
        }
    }

    pub(crate) fn client_message(self) -> &'static str {
        match self {
            Self::UpstreamStatus(_) | Self::Blocked | Self::RestoreUnsupportedFor(_) => {
                self.message()
            }
            // The generic family keeps its historical projection so legacy paths are
            // unaffected; only an explicit cause produces actionable text.
            Self::RestoreUnsupported => CLIENT_PROCESSING_ERROR_MESSAGE,
            _ => CLIENT_PROCESSING_ERROR_MESSAGE,
        }
    }

    pub(crate) fn failure_class(self) -> &'static str {
        match self {
            Self::UpstreamStatus(_) => "Upstream request failed",
            _ => "DLP forwarding error",
        }
    }

    pub(crate) fn into_codex_response(self) -> Response {
        self.into_app_response(&AppType::Codex)
    }

    pub(crate) fn into_app_response(self, app_type: &AppType) -> Response {
        if let Self::RestoreFailedAt(stage) = self {
            super::restore_diagnostics::log_failure(self.code(), stage);
        } else if self == Self::RestoreFailed {
            super::restore_diagnostics::log_failure(
                self.code(),
                super::restore_diagnostics::RestoreStage::Unknown,
            );
        }
        let status = self.client_status_code();
        let code = self.client_code();
        let message = self.client_message();
        let error_type = self.client_error_type();
        let native_message = if matches!(self, Self::UpstreamStatus(_)) {
            code
        } else {
            message
        };
        let payload = match app_type {
            AppType::Claude | AppType::ClaudeDesktop => json!({
                "type": "error",
                "error": {
                    "type": if status == StatusCode::BAD_REQUEST { "invalid_request_error" } else { "api_error" },
                    "message": native_message,
                    "code": code,
                }
            }),
            AppType::Gemini => json!({
                "error": {
                    "code": status.as_u16(),
                    "message": native_message,
                    "status": if status == StatusCode::BAD_REQUEST { "INVALID_ARGUMENT" } else { "UNAVAILABLE" }
                }
            }),
            _ => json!({
                "error": {
                    "message": message,
                    "type": error_type,
                    "code": code,
                    "param": Value::Null,
                }
            }),
        };
        let bytes = serde_json::to_vec(&payload).unwrap_or_else(|_| {
            b"{\"error\":{\"message\":\"The local proxy could not process the request. Please retry.\",\"type\":\"server_error\",\"code\":\"proxy_processing_failed\",\"param\":null}}".to_vec()
        });
        Response::builder()
            .status(status)
            .header(
                header::CONTENT_TYPE,
                HeaderValue::from_static("application/json"),
            )
            .body(Body::from(bytes))
            .unwrap_or_else(|_| Response::new(Body::empty()))
    }
}

pub(crate) fn build_codex_json_error(
    status: StatusCode,
    message: &str,
    error_type: &'static str,
    code: &'static str,
) -> Response {
    let payload = json!({
        "error": {
            "message": message,
            "type": error_type,
            "code": code,
            "param": Value::Null,
        }
    });
    let bytes = serde_json::to_vec(&payload).unwrap_or_else(|_| {
        b"{\"error\":{\"message\":\"The local proxy could not process the request. Please retry.\",\"type\":\"server_error\",\"code\":\"proxy_processing_failed\",\"param\":null}}".to_vec()
    });
    Response::builder()
        .status(status)
        .header(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/json"),
        )
        .body(Body::from(bytes))
        .unwrap_or_else(|_| Response::new(Body::empty()))
}

pub(crate) fn build_codex_restore_error_response(err: RestoreError) -> Response {
    build_codex_restore_error_response_at(err, super::restore_diagnostics::RestoreStage::Unknown)
}

pub(crate) fn build_codex_restore_error_response_at(
    err: RestoreError,
    stage: super::restore_diagnostics::RestoreStage,
) -> Response {
    super::restore_diagnostics::log_failure_with_reason(err.code(), stage, err.reason_at(stage));
    build_codex_json_error(
        StatusCode::INTERNAL_SERVER_ERROR,
        CLIENT_PROCESSING_ERROR_MESSAGE,
        "server_error",
        CLIENT_PROCESSING_ERROR_CODE,
    )
}

fn value_contains_any_token_reference(body: &Value) -> Result<bool, TokenError> {
    match body {
        Value::String(s) => {
            let mut found = false;
            visit_token_references(s, |_| {
                found = true;
                Ok(())
            })?;
            Ok(found)
        }
        Value::Array(items) => {
            for item in items {
                if value_contains_any_token_reference(item)? {
                    return Ok(true);
                }
            }
            Ok(false)
        }
        Value::Object(map) => {
            for (k, v) in map {
                let mut found = false;
                visit_token_references(k, |_| {
                    found = true;
                    Ok(())
                })?;
                if found || value_contains_any_token_reference(v)? {
                    return Ok(true);
                }
            }
            Ok(false)
        }
        _ => Ok(false),
    }
}

// Only actual encoded tool arguments gain a decoding pass. Ordinary strings and
// metadata keep literal semantics, and decoded trees never authorize references.
fn body_contains_any_token_reference(
    body: &Value,
    budget: &PayloadBudget,
) -> Result<bool, TokenError> {
    if value_contains_any_token_reference(body)? {
        return Ok(true);
    }
    encoded_arguments_contain_any_token_reference(body, budget)
}

fn encoded_arguments_contain_any_token_reference(
    body: &Value,
    budget: &PayloadBudget,
) -> Result<bool, TokenError> {
    if let Some(items) = body.get("input").and_then(Value::as_array) {
        for item in items {
            if item.get("type").and_then(Value::as_str) != Some("function_call") {
                continue;
            }
            let Some(encoded) = item.get("arguments").and_then(Value::as_str) else {
                continue;
            };
            if !encoded.as_bytes().contains(&b'\\') {
                continue;
            }
            // PayloadJson preflights 1 MiB, depth and nodes, then reserves both
            // retained and parser scratch allocations before decoding keys/values.
            let decoded = PayloadJson::try_from_str(encoded, budget.clone(), budget.limit())?;
            if value_contains_any_token_reference(decoded.as_value())? {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

/// Allocation ownership from cached history cannot authorize token dependencies.
/// Without a captured restoration lease, reference-shaped text remains untrusted literal text.
pub(crate) fn validate_unleased_enriched_body(body: &Value) -> Result<(), DlpForwardError> {
    // Validate decoded carriers and bounds without treating literal syntax as authority.
    body_contains_any_token_reference(body, &PayloadBudget::default()).map_err(|error| {
        DlpForwardError::from_token_error(error)
            .at_context_checkpoint(ContextCheckpoint::ReferenceScan)
    })?;
    Ok(())
}

fn body_matches_any_restore_rule(
    body: &Value,
    runtime: &SecurityRuntimeConfig,
    restore_rule_ids: &[&str],
    budget: &PayloadBudget,
) -> Result<bool, TokenError> {
    if restore_rule_ids.is_empty() && !runtime.restoration_enabled() {
        return Ok(false);
    }
    let fields = extract_payload_texts(body);
    for field in &fields {
        let spans =
            detect_spans_with_budget(field.text, field.is_password_field, runtime, budget.clone())?;
        for found in spans.as_slice() {
            if !found.range.is_empty()
                && (found.action == EffectiveAction::Restore
                    || (found.action == EffectiveAction::Redact
                        && restore_rule_ids.contains(&found.rule_id)))
            {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

struct ExtractedRequestHistory {
    previous_response_id: Option<PayloadText>,
    calls: PayloadVec<OwnedCallReq>,
    opaques: PayloadVec<PayloadText>,
    orphan_output_call_ids: PayloadVec<PayloadText>,
    referenced_tokens: PayloadVec<PayloadText>,
}

struct OwnedCallReq {
    call_id: PayloadText,
    item_type: PayloadText,
    name: PayloadText,
    payload: PayloadText,
    reasoning: Option<PayloadText>,
    encrypted_reasoning: Option<PayloadText>,
}

fn push_unique_payload_text(
    list: &mut PayloadVec<PayloadText>,
    text: &str,
    limit: usize,
    budget: &PayloadBudget,
) -> Result<(), TokenError> {
    if list.as_slice().iter().any(|t| t.as_str() == text) {
        return Ok(());
    }
    if list.len() >= 4096 {
        return Err(TokenError::CapacityExceeded);
    }
    let owned = PayloadText::try_from_str(text, budget.clone(), limit)?;
    list.push(owned)
}

fn collect_tokens_from_value(
    value: &Value,
    tokens: &mut PayloadVec<PayloadText>,
    budget: &PayloadBudget,
) -> Result<(), TokenError> {
    match value {
        Value::String(s) => visit_token_references(s, |range| {
            push_unique_payload_text(
                tokens,
                &s[range],
                super::token_format::MAX_REFERENCE_BYTES,
                budget,
            )
        }),
        Value::Array(items) => {
            for item in items {
                collect_tokens_from_value(item, tokens, budget)?;
            }
            Ok(())
        }
        Value::Object(map) => {
            for (k, v) in map {
                visit_token_references(k, |range| {
                    push_unique_payload_text(
                        tokens,
                        &k[range],
                        super::token_format::MAX_REFERENCE_BYTES,
                        budget,
                    )
                })?;
                collect_tokens_from_value(v, tokens, budget)?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

fn extract_request_history(
    body: &Value,
    budget: &PayloadBudget,
    projection: Option<&DlpHistoryProjection<'_>>,
    classification: RequestHistoryClassification,
) -> Result<ExtractedRequestHistory, TokenError> {
    let previous_response_id = match body.get("previous_response_id") {
        Some(Value::String(s)) => {
            if !valid_external_id(s) {
                return Err(TokenError::ContextUnavailable);
            }
            Some(PayloadText::try_from_str(s, budget.clone(), 256)?)
        }
        Some(Value::Null) | None => None,
        Some(_) => return Err(TokenError::ContextUnavailable),
    };
    let mut calls = PayloadVec::new(budget.clone())?;
    let mut opaques = PayloadVec::new(budget.clone())?;
    let mut output_call_ids = PayloadVec::new(budget.clone())?;
    let mut referenced_tokens = PayloadVec::new(budget.clone())?;

    visit_request_references(body, projection, |reference| {
        push_unique_payload_text(
            &mut referenced_tokens,
            reference,
            super::token_format::MAX_REFERENCE_BYTES,
            budget,
        )
    })?;

    // Unknown items remain opaque to recovery while genuine token references stay strict.
    if matches!(
        classification,
        RequestHistoryClassification::UnknownPassthrough
    ) {
        return Ok(ExtractedRequestHistory {
            previous_response_id: None,
            calls,
            opaques,
            orphan_output_call_ids: PayloadVec::new(budget.clone())?,
            referenced_tokens,
        });
    }

    if let Some(input) = body.get("input") {
        let items_slice: Vec<&Value> = match input {
            Value::Array(arr) => arr.iter().collect(),
            Value::Object(_) => vec![input],
            _ => Vec::new(),
        };
        for item in items_slice {
            let Some(obj) = item.as_object() else {
                continue;
            };
            match obj.get("type").and_then(Value::as_str) {
                Some("function_call") => {
                    let call_id = obj
                        .get("call_id")
                        .and_then(Value::as_str)
                        .filter(|s| valid_external_id(s))
                        .ok_or(TokenError::ContextUnavailable)?;
                    let name = obj
                        .get("name")
                        .and_then(Value::as_str)
                        .filter(|s| valid_external_id(s))
                        .ok_or(TokenError::ContextUnavailable)?;
                    let args = obj
                        .get("arguments")
                        .and_then(Value::as_str)
                        .ok_or(TokenError::ContextUnavailable)?;
                    let reasoning = match obj.get("reasoning_content") {
                        Some(Value::String(s)) => Some(s.as_str()),
                        Some(Value::Null) | None => None,
                        Some(_) => return Err(TokenError::ContextUnavailable),
                    };
                    let enc_reasoning = match obj.get("encrypted_reasoning") {
                        Some(Value::String(s)) => {
                            if s.is_empty() {
                                return Err(TokenError::ContextUnavailable);
                            }
                            push_unique_payload_text(&mut opaques, s, 1024 * 1024, budget)?;
                            Some(s.as_str())
                        }
                        Some(Value::Null) | None => None,
                        Some(_) => return Err(TokenError::ContextUnavailable),
                    };
                    calls.push(OwnedCallReq {
                        call_id: PayloadText::try_from_str(call_id, budget.clone(), 256)?,
                        item_type: PayloadText::try_from_str("function_call", budget.clone(), 32)?,
                        name: PayloadText::try_from_str(name, budget.clone(), 256)?,
                        payload: PayloadText::try_from_str(args, budget.clone(), 1024 * 1024)?,
                        reasoning: reasoning
                            .map(|r| PayloadText::try_from_str(r, budget.clone(), 1024 * 1024))
                            .transpose()?,
                        encrypted_reasoning: enc_reasoning
                            .map(|e| PayloadText::try_from_str(e, budget.clone(), 1024 * 1024))
                            .transpose()?,
                    })?;
                }
                Some("custom_tool_call") => {
                    let call_id = obj
                        .get("call_id")
                        .and_then(Value::as_str)
                        .filter(|s| valid_external_id(s))
                        .ok_or(TokenError::ContextUnavailable)?;
                    let name = obj
                        .get("name")
                        .and_then(Value::as_str)
                        .filter(|s| valid_external_id(s))
                        .ok_or(TokenError::ContextUnavailable)?;
                    let input_str = obj
                        .get("input")
                        .and_then(Value::as_str)
                        .ok_or(TokenError::ContextUnavailable)?;
                    let reasoning = match obj.get("reasoning_content") {
                        Some(Value::String(s)) => Some(s.as_str()),
                        Some(Value::Null) | None => None,
                        Some(_) => return Err(TokenError::ContextUnavailable),
                    };
                    let enc_reasoning = match obj.get("encrypted_reasoning") {
                        Some(Value::String(s)) => {
                            if s.is_empty() {
                                return Err(TokenError::ContextUnavailable);
                            }
                            push_unique_payload_text(&mut opaques, s, 1024 * 1024, budget)?;
                            Some(s.as_str())
                        }
                        Some(Value::Null) | None => None,
                        Some(_) => return Err(TokenError::ContextUnavailable),
                    };
                    calls.push(OwnedCallReq {
                        call_id: PayloadText::try_from_str(call_id, budget.clone(), 256)?,
                        item_type: PayloadText::try_from_str(
                            "custom_tool_call",
                            budget.clone(),
                            32,
                        )?,
                        name: PayloadText::try_from_str(name, budget.clone(), 256)?,
                        payload: PayloadText::try_from_str(input_str, budget.clone(), 1024 * 1024)?,
                        reasoning: reasoning
                            .map(|r| PayloadText::try_from_str(r, budget.clone(), 1024 * 1024))
                            .transpose()?,
                        encrypted_reasoning: enc_reasoning
                            .map(|e| PayloadText::try_from_str(e, budget.clone(), 1024 * 1024))
                            .transpose()?,
                    })?;
                }
                Some("function_call_output" | "custom_tool_call_output") => {
                    if crate::proxy::providers::codex_chat_common::is_unsolicited_page_context(item)
                    {
                        continue;
                    }
                    let call_id = obj
                        .get("call_id")
                        .and_then(Value::as_str)
                        .filter(|s| valid_external_id(s))
                        .ok_or(TokenError::ContextUnavailable)?;
                    push_unique_payload_text(&mut output_call_ids, call_id, 256, budget)?;
                }
                Some("reasoning") => {
                    if let Some(enc) = obj.get("encrypted_content") {
                        match enc {
                            Value::String(s) => {
                                if s.is_empty() {
                                    return Err(TokenError::ContextUnavailable);
                                }
                                push_unique_payload_text(&mut opaques, s, 1024 * 1024, budget)?;
                            }
                            Value::Null => {}
                            _ => return Err(TokenError::ContextUnavailable),
                        }
                    }
                }
                Some("compaction") => {
                    let enc = obj
                        .get("encrypted_content")
                        .and_then(Value::as_str)
                        .filter(|s| !s.is_empty())
                        .ok_or(TokenError::ContextUnavailable)?;
                    push_unique_payload_text(&mut opaques, enc, 1024 * 1024, budget)?;
                }
                _ => {}
            }
        }
    }

    let mut orphan_output_call_ids = PayloadVec::new(budget.clone())?;
    for cid in output_call_ids.as_slice() {
        if !calls
            .as_slice()
            .iter()
            .any(|c| c.call_id.as_str() == cid.as_str())
        {
            orphan_output_call_ids.push(PayloadText::try_from_str(
                cid.as_str(),
                budget.clone(),
                256,
            )?)?;
        }
    }

    Ok(ExtractedRequestHistory {
        previous_response_id,
        calls,
        opaques,
        orphan_output_call_ids,
        referenced_tokens,
    })
}

#[cfg(test)]
pub(super) mod route_scope_test_support {
    use std::{cell::Cell, marker::PhantomData, rc::Rc};

    use crate::proxy::{
        handler_context::{
            RequestIngress, RestorationFraming, RestorationOperation, RestorationRouteScope,
        },
        protocol_bridge::Protocol,
    };

    type Snapshot = (
        Protocol,
        RequestIngress,
        RestorationOperation,
        RestorationFraming,
    );

    thread_local! {
        static LAST_SCOPE: Cell<Option<Snapshot>> = const { Cell::new(None) };
    }

    // Read-only call-site observation; no request values or admission authority.
    pub(in crate::proxy::security) struct Observation {
        previous: Option<Snapshot>,
        _thread_bound: PhantomData<Rc<()>>,
    }

    pub(in crate::proxy::security) fn observe() -> Observation {
        Observation {
            previous: LAST_SCOPE.with(|slot| slot.replace(None)),
            _thread_bound: PhantomData,
        }
    }

    impl Observation {
        pub(in crate::proxy::security) fn recorded(&self) -> Option<Snapshot> {
            LAST_SCOPE.with(Cell::get)
        }
    }

    impl Drop for Observation {
        fn drop(&mut self) {
            LAST_SCOPE.with(|slot| slot.set(self.previous));
        }
    }

    pub(super) fn record(scope: RestorationRouteScope) {
        LAST_SCOPE.with(|slot| {
            slot.set(Some((
                scope.client,
                scope.ingress,
                scope.operation,
                scope.framing,
            )));
        });
    }
}

/// Admit only the existing Responses cells from trusted handler-owned scope.
/// Native and unsupported cells retain ordinary inspection, never namespace authority.
pub(crate) fn admit_http_request_with_scope(
    gateway: &RestorationGateway,
    app_type: &AppType,
    scope: crate::proxy::handler_context::RestorationRouteScope,
    body: &mut Value,
    headers: &HeaderMap,
) -> Result<RestorationAdmission, crate::proxy::error::ProxyError> {
    use crate::proxy::{
        handler_context::{RequestIngress, RestorationFraming, RestorationOperation},
        protocol_bridge::Protocol,
    };
    #[cfg(test)]
    route_scope_test_support::record(scope);
    if matches!(app_type, AppType::Codex)
        && matches!(
            (scope.ingress, scope.client, scope.operation, scope.framing),
            (
                RequestIngress::Direct,
                Protocol::Chat,
                RestorationOperation::ChatCompletions,
                RestorationFraming::Json | RestorationFraming::Sse
            )
        )
    {
        return native_chat::admit(gateway, app_type, body, headers, Protocol::Chat);
    }
    if matches!(app_type, AppType::Claude | AppType::ClaudeDesktop)
        && matches!(
            (scope.ingress, scope.client, scope.operation, scope.framing),
            (
                RequestIngress::Direct,
                Protocol::Anthropic,
                RestorationOperation::Messages,
                RestorationFraming::Json | RestorationFraming::Sse
            )
        )
    {
        return native_chat::admit(gateway, app_type, body, headers, Protocol::Anthropic);
    }
    // Only the two canonical generation pairs may acquire native Gemini authority;
    // a mismatched operation/framing combination stays on the legacy path.
    if matches!(app_type, AppType::Gemini)
        && matches!(
            (scope.ingress, scope.client, scope.operation, scope.framing),
            (
                RequestIngress::Direct,
                Protocol::Gemini,
                RestorationOperation::GenerateContent,
                RestorationFraming::Json
            ) | (
                RequestIngress::Direct,
                Protocol::Gemini,
                RestorationOperation::StreamGenerateContent,
                RestorationFraming::Sse
            )
        )
    {
        return native_chat::admit(gateway, app_type, body, headers, Protocol::Gemini);
    }
    // Registration is descriptive, not admission authority; no actual upstream is
    // known here. Gateway keeps only its legacy Responses behavior. Neither ingress
    // may acquire native authority from application attribution or request headers.
    let responses_supported = matches!(
        (scope.ingress, scope.client, scope.operation, scope.framing),
        (
            RequestIngress::Direct | RequestIngress::Gateway,
            Protocol::Responses,
            RestorationOperation::Responses,
            RestorationFraming::Json | RestorationFraming::Sse
        ) | (
            RequestIngress::Direct | RequestIngress::Gateway,
            Protocol::Responses,
            RestorationOperation::ResponsesCompact,
            RestorationFraming::Json
        )
    );
    let native_direct_scope = scope.ingress == RequestIngress::Direct
        && matches!(
            (scope.client, app_type),
            (Protocol::Chat, AppType::Codex)
                | (
                    Protocol::Anthropic,
                    AppType::Claude | AppType::ClaudeDesktop
                )
                | (Protocol::Gemini, AppType::Gemini)
        );
    admit_http_request(
        gateway,
        app_type,
        responses_supported,
        scope.operation == RestorationOperation::ResponsesCompact,
        native_direct_scope,
        body,
        headers,
    )
}

pub(crate) fn admit_http_request(
    gateway: &RestorationGateway,
    app_type: &AppType,
    endpoint_supported_codex_responses: bool,
    endpoint_is_compact: bool,
    native_direct_scope: bool,
    body: &mut Value,
    headers: &HeaderMap,
) -> Result<RestorationAdmission, crate::proxy::error::ProxyError> {
    let is_supported_codex =
        matches!(app_type, AppType::Codex) && endpoint_supported_codex_responses;
    let gateway_state = gateway.state();
    let reference_budget = match &gateway_state {
        Ok(Some(state)) => state.budget.clone(),
        _ => PayloadBudget::default(),
    };
    let has_tokens = if matches!(gateway_state, Ok(Some(_))) && is_supported_codex {
        false // Recomputed with private candidate proof after namespace admission.
    } else {
        match body_contains_any_token_reference(body, &reference_budget) {
            Ok(_) => false,
            Err(error) => {
                return Ok(RestorationAdmission::Intercepted(
                    DlpForwardError::from_token_error(error).into_app_response(app_type),
                ))
            }
        }
    };

    let gw_state = match gateway_state {
        Ok(Some(state)) => state,
        Ok(None) => {
            if has_tokens {
                let fwd_err = if is_supported_codex {
                    DlpForwardError::ContextUnavailable
                        .at_context_checkpoint(ContextCheckpoint::GatewayMissing)
                } else {
                    DlpForwardError::RestoreUnsupported
                };
                return Ok(RestorationAdmission::Intercepted(
                    fwd_err.into_app_response(app_type),
                ));
            }
            return match inspect_and_intercept_mut_with_headers(app_type, body, headers)? {
                Some(resp) => Ok(RestorationAdmission::Intercepted(resp)),
                None => Ok(RestorationAdmission::Passthrough),
            };
        }
        Err(err) => {
            return Ok(RestorationAdmission::Intercepted(
                DlpForwardError::from_token_error(err)
                    .at_context_checkpoint(ContextCheckpoint::GatewayState)
                    .into_app_response(app_type),
            ));
        }
    };

    let snapshot = match admit_security_runtime() {
        Ok(snap) => snap,
        Err(_) => {
            return Ok(RestorationAdmission::Intercepted(
                DlpForwardError::Unavailable.into_app_response(app_type),
            ));
        }
    };

    let dlp_active = snapshot.runtime.restoration_enabled()
        || (snapshot.runtime.enabled && !gw_state.rules.is_empty());
    let mut admitted_authority = if is_supported_codex && dlp_active {
        Some(
            RequestAuthority::admit_responses(
                &gw_state.store,
                "codex",
                extract_codex_restoration_session_id(headers, body),
                snapshot.restoration_policy_revision,
                gw_state.budget.clone(),
            )
            .map_err(|error| {
                crate::proxy::error::ProxyError::DlpForward(
                    DlpForwardError::from_token_error(error)
                        .at_context_checkpoint(ContextCheckpoint::NamespaceAdmission),
                )
            })?,
        )
    } else {
        None
    };
    let mut refusal_audit = None;
    let mut refusal_state = None;
    let result = (|| {
        // Optional discovery borrows the immutable request and never reserves mandatory payload bytes.
        let candidate = admitted_authority.as_ref().and_then(|authority| {
            authority
                .lease()
                .history_admission_candidate_projection(body)
        });
        let has_tokens = if is_supported_codex {
            // Syntax is bounded and validated, but never creates restoration authority.
            if let Err(error) = visit_request_references(body, candidate.as_ref(), |_| Ok(()))
                .and_then(|_| {
                    encoded_arguments_contain_any_token_reference(body, &reference_budget)
                        .map(|_| ())
                })
            {
                return Ok(RestorationAdmission::Intercepted(
                    DlpForwardError::from_token_error(error)
                        .at_context_checkpoint(ContextCheckpoint::ReferenceScan)
                        .into_app_response(app_type),
                ));
            }
            false
        } else {
            has_tokens
        };
        let budget = gw_state.budget.clone();

        let ledger = match gw_state.store.allocation_ledger() {
            Ok(l) => l,
            Err(err) => {
                return Ok(RestorationAdmission::Intercepted(
                    DlpForwardError::from_token_error(err).into_app_response(app_type),
                ));
            }
        };

        let mut rule_refs = match PayloadVec::new(budget.clone()) {
            Ok(v) => v,
            Err(err) => {
                return Ok(RestorationAdmission::Intercepted(
                    DlpForwardError::from_token_error(err).into_app_response(app_type),
                ));
            }
        };
        for r in gw_state.rules.as_slice() {
            if let Err(err) = rule_refs.push(r.as_str()) {
                return Ok(RestorationAdmission::Intercepted(
                    DlpForwardError::from_token_error(err).into_app_response(app_type),
                ));
            }
        }

        if !dlp_active && !has_tokens {
            return match inspect_and_intercept_mut_with_headers(app_type, body, headers)? {
                Some(resp) => Ok(RestorationAdmission::Intercepted(resp)),
                None => Ok(RestorationAdmission::Passthrough),
            };
        }

        let audit_state = match RequestAuditState::new(
            app_type,
            extract_codex_restoration_session_id(headers, body),
            budget.clone(),
        ) {
            Ok(state) => std::sync::Arc::new(state),
            Err(err) => {
                return Ok(RestorationAdmission::Intercepted(
                    DlpForwardError::from_token_error(err).into_app_response(app_type),
                ))
            }
        };
        refusal_audit = PendingRequestAudit::capture_refusal_with_history(
            body,
            &snapshot.runtime,
            budget.clone(),
            candidate.as_ref(),
        )
        .ok();
        refusal_state = Some(std::sync::Arc::clone(&audit_state));
        if !snapshot.runtime.enabled && has_tokens {
            let fwd_err = if is_supported_codex {
                DlpForwardError::ContextUnavailable
                    .at_context_checkpoint(ContextCheckpoint::RuntimeDisabled)
            } else {
                DlpForwardError::RestoreUnsupported
            };
            return Ok(RestorationAdmission::Intercepted(
                fwd_err.into_app_response(app_type),
            ));
        }

        if !is_supported_codex {
            let matched_restore = match body_matches_any_restore_rule(
                body,
                &snapshot.runtime,
                rule_refs.as_slice(),
                &budget,
            ) {
                Ok(m) => m,
                Err(err) => {
                    return Ok(RestorationAdmission::Intercepted(
                        DlpForwardError::from_token_error(err).into_app_response(app_type),
                    ));
                }
            };
            if matched_restore || has_tokens {
                // Only a direct native scope is caller-actionable; gateway and other
                // legacy shapes keep their existing fixed projection.
                let error = if native_direct_scope {
                    DlpForwardError::RestoreUnsupportedFor(
                        RestoreUnsupportedCause::UnsupportedEndpoint,
                    )
                } else {
                    DlpForwardError::RestoreUnsupported
                };
                return Ok(RestorationAdmission::Intercepted(
                    error.into_app_response(app_type),
                ));
            }
            return match inspect_and_intercept_mut_with_headers(app_type, body, headers)? {
                Some(resp) => Ok(RestorationAdmission::Intercepted(resp)),
                None => Ok(RestorationAdmission::Passthrough),
            };
        }

        let mut authority =
            admitted_authority
                .take()
                .ok_or(crate::proxy::error::ProxyError::DlpForward(
                    DlpForwardError::ContextUnavailable,
                ))?;

        let history_classification =
            responses_adapter::classify_request_history(body, &budget, candidate.as_ref())
                .map_err(|error| {
                    crate::proxy::error::ProxyError::DlpForward(
                        DlpForwardError::from_token_error(error)
                            .at_context_checkpoint(ContextCheckpoint::HistoryClassify),
                    )
                })?;
        let history = match extract_request_history(
            body,
            &budget,
            candidate.as_ref(),
            history_classification,
        ) {
            Ok(h) => h,
            Err(err) => {
                return Ok(RestorationAdmission::Intercepted(
                    DlpForwardError::from_token_error(err)
                        .at_context_checkpoint(ContextCheckpoint::HistoryExtract)
                        .into_app_response(app_type),
                ));
            }
        };

        let fresh_readable_history = matches!(
            history_classification,
            RequestHistoryClassification::FreshReadable
        );
        let needs_bound_history_inspection = candidate.is_some();
        let needs_provider_resume = matches!(
            history_classification,
            RequestHistoryClassification::ExplicitDependency
        );

        let mut rules_copy = if gw_state.rules.is_empty() {
            StoreVec::new(ledger.clone(), 4096)
        } else {
            match StoreVec::with_capacity(gw_state.rules.len(), ledger.clone(), 4096) {
                Ok(v) => v,
                Err(err) => {
                    return Ok(RestorationAdmission::Intercepted(
                        DlpForwardError::from_token_error(err).into_app_response(app_type),
                    ));
                }
            }
        };
        for r in gw_state.rules.as_slice() {
            let copied = match StoreText::try_from_str(r.as_str(), ledger.clone(), 256) {
                Ok(t) => t,
                Err(err) => {
                    return Ok(RestorationAdmission::Intercepted(
                        DlpForwardError::from_token_error(err).into_app_response(app_type),
                    ));
                }
            };
            if let Err(err) = rules_copy.push_without_growth(copied) {
                return Ok(RestorationAdmission::Intercepted(
                    DlpForwardError::from_token_error(err).into_app_response(app_type),
                ));
            }
        }

        let source_body = match PayloadJson::try_from_value(body, budget.clone(), budget.limit()) {
            Ok(j) => j,
            Err(err) => {
                return Ok(RestorationAdmission::Intercepted(
                    DlpForwardError::from_token_error(err).into_app_response(app_type),
                ));
            }
        };

        let initial_audit = std::sync::Arc::new(
            match PendingRequestAudit::capture_with_history(
                source_body.as_value(),
                &snapshot.runtime,
                rule_refs.as_slice(),
                authority.lease(),
                budget.clone(),
                candidate.as_ref(),
            ) {
                Ok(summary) => summary,
                Err(err) => {
                    return Ok(RestorationAdmission::Intercepted(
                        DlpForwardError::from_plan_error(err)
                            .at_context_checkpoint(ContextCheckpoint::AdmissionAudit)
                            .into_app_response(app_type),
                    ))
                }
            },
        );

        drop(candidate);
        if !needs_bound_history_inspection
            && !needs_provider_resume
            && !fresh_readable_history
            && history.referenced_tokens.as_slice().is_empty()
        {
            match authority.prepare_responses_request(
                source_body.as_value(),
                &snapshot.runtime,
                rule_refs.as_slice(),
                |_, _, _| Ok(()),
            ) {
                Ok(prepared) => {
                    *body = prepared.body.as_value().clone();
                    let pretokenized_body =
                        match PayloadJson::try_from_value(body, budget.clone(), budget.limit()) {
                            Ok(j) => j,
                            Err(err) => {
                                return Ok(RestorationAdmission::Intercepted(
                                    DlpForwardError::from_token_error(err)
                                        .into_app_response(app_type),
                                ));
                            }
                        };
                    return Ok(RestorationAdmission::Admitted(Box::new(DlpHttpRequest {
                        authority,
                        runtime: (*snapshot.runtime).clone(),
                        restore_rules: rules_copy,
                        source_body: pretokenized_body,
                        pretokenized: true,
                        native_protocol: None,
                        endpoint_is_compact,
                        audit_state: std::sync::Arc::clone(&audit_state),
                        initial_audit,
                    })));
                }
                Err(plan_err) => {
                    return Ok(RestorationAdmission::Intercepted(
                        DlpForwardError::from_plan_error(plan_err)
                            .at_context_checkpoint(ContextCheckpoint::AdmissionPrepare)
                            .into_app_response(app_type),
                    ));
                }
            }
        }

        Ok(RestorationAdmission::Admitted(Box::new(DlpHttpRequest {
            authority,
            runtime: (*snapshot.runtime).clone(),
            restore_rules: rules_copy,
            source_body,
            pretokenized: false,
            native_protocol: None,
            endpoint_is_compact,
            audit_state: std::sync::Arc::clone(&audit_state),
            initial_audit,
        })))
    })();
    if matches!(&result, Ok(RestorationAdmission::Intercepted(_))) {
        if let (Some(pending), Some(state)) = (&refusal_audit, &refusal_state) {
            state.record_refusal(pending, "", None);
        }
    }
    result
}

/// Independently owned response-name table. Allocation ownership is not restoration authority.
pub(crate) struct DlpResponseNamespaceTable {
    entries: PayloadVec<ResponseNamespaceEntry>,
    #[cfg(test)]
    _probe: Option<response_namespace_test_support::OwnerProbe>,
}
struct ResponseNamespaceEntry {
    flat: crate::proxy::providers::transform_codex_responses_namespace::StackToolName,
    namespace: PayloadText,
    name: PayloadText,
}
impl crate::proxy::providers::transform_codex_responses_namespace::NamespaceLookup
    for DlpResponseNamespaceTable
{
    fn lookup(&self, flat: &str) -> Option<(&str, &str)> {
        self.entries
            .as_slice()
            .iter()
            .find(|entry| entry.flat.as_str() == flat)
            .map(|entry| (entry.namespace.as_str(), entry.name.as_str()))
    }
    fn is_empty(&self) -> bool {
        self.entries.as_slice().is_empty()
    }
}
impl DlpResponseNamespaceTable {
    fn build(body: &Value, budget: PayloadBudget) -> Result<Self, TokenError> {
        use crate::proxy::providers::transform_codex_responses_namespace::StackToolName;
        let mut table = Self {
            entries: PayloadVec::new(budget.clone())?,
            #[cfg(test)]
            _probe: None,
        };
        let Some(tools) = body.get("tools").and_then(Value::as_array) else {
            return Ok(table);
        };
        for tool in tools {
            if tool.get("type").and_then(Value::as_str) != Some("namespace") {
                continue;
            }
            let Some(namespace) = tool
                .get("name")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|name| !name.is_empty())
            else {
                continue;
            };
            let Some(children) = tool
                .get("tools")
                .or_else(|| tool.get("children"))
                .and_then(Value::as_array)
            else {
                continue;
            };
            for child in children {
                if child.get("type").and_then(Value::as_str) != Some("function") {
                    continue;
                }
                let Some(name) = child
                    .get("name")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|name| !name.is_empty())
                else {
                    continue;
                };
                let flat =
                    StackToolName::new(namespace, name).ok_or(TokenError::CapacityExceeded)?;
                if table
                    .entries
                    .as_slice()
                    .iter()
                    .any(|entry| entry.flat.as_str() == flat.as_str())
                {
                    continue;
                }
                let namespace =
                    PayloadText::try_from_str(namespace, budget.clone(), budget.limit())?;
                let name = PayloadText::try_from_str(name, budget.clone(), budget.limit())?;
                table.entries.push(ResponseNamespaceEntry {
                    flat,
                    namespace,
                    name,
                })?;
            }
        }
        #[cfg(test)]
        {
            let charge = table.entries.capacity() * std::mem::size_of::<ResponseNamespaceEntry>()
                + table
                    .entries
                    .as_slice()
                    .iter()
                    .map(|entry| entry.namespace.capacity() + entry.name.capacity())
                    .sum::<usize>();
            table._probe = Some(response_namespace_test_support::OwnerProbe::new(charge));
        }
        Ok(table)
    }
}

impl DlpHttpRequest {
    pub(crate) fn is_native_chat(&self) -> bool {
        self.native_protocol == Some(crate::proxy::protocol_bridge::Protocol::Chat)
    }

    pub(crate) fn is_native_anthropic(&self) -> bool {
        self.native_protocol == Some(crate::proxy::protocol_bridge::Protocol::Anthropic)
    }

    pub(crate) fn is_native_gemini(&self) -> bool {
        self.native_protocol == Some(crate::proxy::protocol_bridge::Protocol::Gemini)
    }

    pub(crate) fn build_response_namespace_table(
        &self,
        body: &Value,
    ) -> Result<Option<DlpResponseNamespaceTable>, DlpForwardError> {
        if !(self.runtime.restoration_enabled()
            || (self.runtime.enabled && !self.restore_rules.as_slice().is_empty())
            || !self.authority.lease().is_empty())
        {
            return Ok(None);
        }
        #[cfg(test)]
        response_namespace_test_support::before_build(&self.authority.budget())?;
        let result = DlpResponseNamespaceTable::build(body, self.authority.budget())
            .map(Some)
            .map_err(DlpForwardError::from_token_error);
        #[cfg(test)]
        response_namespace_test_support::after_build(result.as_ref().err().copied());
        result
    }

    pub(crate) fn chat_history_provenance(
        &self,
    ) -> Result<super::chat_enrichment::ChatHistoryProvenance, DlpForwardError> {
        if self.native_protocol.is_some() {
            return Ok(super::chat_enrichment::ChatHistoryProvenance::UnknownPassthrough);
        }
        let candidate = self
            .authority
            .lease()
            .history_admission_candidate_projection(self.source_body.as_value());
        let classification = responses_adapter::classify_request_history(
            self.source_body.as_value(),
            &self.authority.budget(),
            candidate.as_ref(),
        )
        .map_err(|error| {
            DlpForwardError::from_token_error(error)
                .at_context_checkpoint(ContextCheckpoint::HistoryClassify)
        })?;
        Ok(match classification {
            RequestHistoryClassification::FreshReadable => {
                super::chat_enrichment::ChatHistoryProvenance::IndependentReadable
            }
            RequestHistoryClassification::ExplicitDependency => {
                super::chat_enrichment::ChatHistoryProvenance::StrictResume
            }
            RequestHistoryClassification::UnknownPassthrough => {
                super::chat_enrichment::ChatHistoryProvenance::UnknownPassthrough
            }
        })
    }

    pub(crate) fn chat_enrichment_budget(
        &self,
    ) -> Option<super::chat_enrichment::ChatEnrichmentBudget> {
        (self.runtime.restoration_enabled()
            || (self.runtime.enabled && !self.restore_rules.as_slice().is_empty())
            || !self.authority.lease().is_empty())
        .then(|| super::chat_enrichment::ChatEnrichmentBudget::new(self.authority.budget()))
    }

    #[cfg(test)]
    pub(super) fn audit_observer_for_test(
        &self,
    ) -> impl Fn() -> Vec<super::aggregation::PendingAuditEvent> {
        let state = self.audit_state.clone();
        move || state.recorded_for_test()
    }

    pub(crate) fn clone_source_body(&self) -> Result<(Value, DlpSourceBodyOwner), DlpForwardError> {
        #[cfg(test)]
        source_copy_test_support::attempt();
        let copy = PayloadJson::try_from_value(
            self.source_body.as_value(),
            self.authority.budget(),
            self.authority.budget().limit(),
        )
        .map_err(DlpForwardError::from_token_error)?;
        let (value, reservation) = copy.into_parts();
        Ok((
            value,
            DlpSourceBodyOwner {
                #[cfg(test)]
                _probe: source_copy_test_support::OwnerProbe::new(reservation.bytes()),
                _reservation: reservation,
            },
        ))
    }

    pub(crate) fn prepare_native_error_safety(
        &self,
        attempt: &mut DlpPreparedAttempt,
        provider: &Value,
        credentials: &[String],
        sent_credentials: Option<&super::upstream_error::ErrorCredentials>,
    ) -> Result<(), DlpForwardError> {
        if attempt.native_protocol.is_none() {
            return Ok(());
        }
        if sent_credentials.is_none() {
            return Err(DlpForwardError::RedactionFailed);
        }
        attempt.native_error_safety = Some(
            super::upstream_error::NativeErrorSafety::prepare(
                super::upstream_error::ErrorSafetyContext {
                    runtime: &self.runtime,
                    request: self.source_body.as_value(),
                    provider,
                    credentials,
                    sent_credentials,
                    budget: self.authority.budget(),
                },
            )
            .map_err(DlpForwardError::from_token_error)?,
        );
        Ok(())
    }

    pub(crate) fn capture_error_credentials(
        &self,
        headers: &http::HeaderMap,
    ) -> Option<super::upstream_error::ErrorCredentials> {
        super::upstream_error::ErrorCredentials::capture(headers, self.authority.budget()).ok()
    }

    pub(crate) async fn read_upstream_error(
        &self,
        response: crate::proxy::hyper_client::ProxyResponse,
        provider: &Value,
        credentials: &[String],
        sent_credentials: Option<&super::upstream_error::ErrorCredentials>,
        timeout: std::time::Duration,
    ) -> super::upstream_error::SafeUpstreamError {
        if sent_credentials.is_none() {
            return super::upstream_error::SafeUpstreamError::unavailable(response.status());
        }
        super::upstream_error::read_error(
            response,
            super::upstream_error::ErrorSafetyContext {
                runtime: &self.runtime,
                request: self.source_body.as_value(),
                provider,
                credentials,
                sent_credentials,
                budget: self.authority.budget(),
            },
            timeout,
        )
        .await
    }

    pub(crate) fn build_mapped_body(
        &self,
        body: &Value,
        provider: &crate::provider::Provider,
        strip_one_m: bool,
    ) -> Result<(Value, DlpMappingBodyOwner), DlpForwardError> {
        #[cfg(test)]
        mapping_copy_test_support::attempt();
        let replacement = body.get("model").and_then(|target| {
            let original = target.as_str()?;
            let mapped = crate::proxy::model_mapper::borrowed_mapped_model(original, provider);
            let text = if strip_one_m {
                crate::proxy::model_mapper::strip_one_m_suffix_for_upstream(mapped)
            } else {
                mapped
            };
            (text != original).then_some(JsonStringReplacement { target, text })
        });
        let copy = match replacement {
            Some(edit) => PayloadJson::try_with_replacements(
                body,
                &[edit],
                self.authority.budget(),
                self.authority.budget().limit(),
            ),
            None => PayloadJson::try_from_value(
                body,
                self.authority.budget(),
                self.authority.budget().limit(),
            ),
        }
        .map_err(DlpForwardError::from_token_error)?;
        let (value, reservation) = copy.into_parts();
        Ok((
            value,
            DlpMappingBodyOwner {
                #[cfg(test)]
                _probe: mapping_copy_test_support::OwnerProbe::new(reservation.bytes()),
                _reservation: reservation,
            },
        ))
    }

    /// Reserve resolver scratch before its first copy; release only after the
    /// selected model is copied into the independently owned mapping tree.
    pub(crate) fn build_desktop_provider_mapped_body(
        &self,
        body: &Value,
        provider: &crate::provider::Provider,
    ) -> Result<(Value, DlpMappingBodyOwner), crate::proxy::error::ProxyError> {
        use crate::proxy::error::ProxyError;

        let _resolver_scratch = if self.is_native_anthropic() {
            let bytes =
                crate::claude_desktop_config::proxy_request_model_scratch_bytes(body, provider)
                    .ok_or(ProxyError::DlpForward(DlpForwardError::CapacityExceeded))?;
            Some(
                self.authority
                    .budget()
                    .reserve(bytes)
                    .map_err(DlpForwardError::from_token_error)
                    .map_err(ProxyError::DlpForward)?,
            )
        } else {
            None
        };
        let model = crate::claude_desktop_config::proxy_request_upstream_model(body, provider)
            .map_err(|error| {
                // Do not move model-derived error text beyond the scratch owner
                // or expose it to client responses and forwarding diagnostics.
                let message = if self.is_native_anthropic() {
                    "Claude Desktop model routing failed.".to_string()
                } else {
                    error.to_string()
                };
                ProxyError::InvalidRequest(message)
            })?;
        self.build_desktop_mapped_body(
            body,
            &model,
            crate::claude_desktop_config::should_normalize_mimo_anthropic_thinking_history(
                provider, &model,
            ),
        )
        .map_err(ProxyError::DlpForward)
    }

    pub(crate) fn build_desktop_mapped_body(
        &self,
        body: &Value,
        model: &str,
        normalize_mimo: bool,
    ) -> Result<(Value, DlpMappingBodyOwner), DlpForwardError> {
        #[cfg(test)]
        mapping_copy_test_support::attempt();
        if normalize_mimo && self.is_native_anthropic() {
            if let Some(messages) = body.get("messages").and_then(Value::as_array) {
                for message in messages {
                    if message.get("role").and_then(Value::as_str) != Some("assistant") {
                        continue;
                    }
                    if let Some(blocks) = message.get("content").and_then(Value::as_array) {
                        let tool_bearing = blocks.iter().any(|block| {
                            block.get("type").and_then(Value::as_str) == Some("tool_use")
                        });
                        let immutable = blocks.iter().any(|block| {
                            matches!(
                                block.get("type").and_then(Value::as_str),
                                Some("thinking" | "redacted_thinking")
                            )
                        });
                        // Native carriers are immutable before normalization or
                        // inspection. Never remove signatures to make them editable.
                        if tool_bearing && immutable {
                            return Err(DlpForwardError::RestoreUnsupported);
                        }
                    }
                }
            }
        }
        let target = body
            .get("model")
            .filter(|value| value.is_string())
            .ok_or(DlpForwardError::RedactionFailed)?;
        let model = crate::proxy::model_mapper::strip_one_m_suffix_for_upstream(model);
        let copy = PayloadJson::try_with_replacements(
            body,
            &[JsonStringReplacement {
                target,
                text: model,
            }],
            self.authority.budget(),
            self.authority.budget().limit(),
        )
        .map_err(DlpForwardError::from_token_error)?;
        let (mut value, mut reservation) = copy.into_parts();
        let result = (|| {
            if normalize_mimo {
                // MiMo can grow each tool-bearing assistant array by one block,
                // replace redacted blocks, or add a missing thinking field. The
                // second full-tree charge covers container reallocation peaks;
                // each block/message gets a conservative new-object allowance.
                let mut units = 0usize;
                if let Some(messages) = value.get("messages").and_then(Value::as_array) {
                    units = messages.len();
                    for message in messages {
                        if let Some(blocks) = message.get("content").and_then(Value::as_array) {
                            units = units
                                .checked_add(blocks.len())
                                .ok_or(DlpForwardError::CapacityExceeded)?;
                        }
                    }
                }
                let growth = units
                    .checked_mul(4096)
                    .and_then(|bytes| bytes.checked_add(reservation.bytes()))
                    .ok_or(DlpForwardError::CapacityExceeded)?;
                reservation
                    .grow(growth)
                    .map_err(DlpForwardError::from_token_error)?;
                crate::claude_desktop_config::normalize_mimo_anthropic_thinking_history(&mut value);
            }
            Ok(())
        })();
        if let Err(error) = result {
            drop(value);
            return Err(error);
        }
        let owner = DlpMappingBodyOwner {
            #[cfg(test)]
            _probe: mapping_copy_test_support::OwnerProbe::new(reservation.bytes()),
            _reservation: reservation,
        };
        Ok((value, owner))
    }

    pub(crate) fn build_namespace_body(
        &self,
        body: &Value,
    ) -> Result<Option<(Value, DlpNamespaceOwner)>, DlpForwardError> {
        let Some(output) = PayloadJson::try_namespace_body(
            body,
            self.authority.budget(),
            self.authority.budget().limit(),
        )
        .map_err(DlpForwardError::from_token_error)?
        else {
            return Ok(None);
        };
        let (body, reservation) = output.into_parts();
        Ok(Some((
            body,
            DlpNamespaceOwner {
                _reservation: reservation,
            },
        )))
    }

    pub(crate) fn build_prompt_cache_body(
        &self,
        body: &Value,
        key: &str,
    ) -> Result<(Value, DlpPromptCacheOwner), DlpForwardError> {
        #[cfg(test)]
        transform_input_test_support::prompt_candidate();
        let output = PayloadJson::try_with_prompt_cache_key(
            body,
            key,
            self.authority.budget(),
            self.authority.budget().limit(),
        )
        .map_err(DlpForwardError::from_token_error)?;
        let (body, reservation) = output.into_parts();
        Ok((
            body,
            DlpPromptCacheOwner {
                _reservation: reservation,
            },
        ))
    }

    pub(crate) fn build_outbound_tail(
        &self,
        body: &Value,
        patch: Option<&Value>,
    ) -> Result<(Value, DlpOutboundTailOwner), DlpForwardError> {
        let budget = self.authority.budget();
        let limit = budget.limit();
        let output = if self.is_native_anthropic() {
            PayloadJson::try_anthropic_outbound_tail(body, patch, budget, limit)
        } else {
            PayloadJson::try_outbound_tail(body, patch, budget, limit)
        }
        .map_err(DlpForwardError::from_token_error)?;
        let (body, reservation) = output.into_parts();
        Ok((
            body,
            DlpOutboundTailOwner {
                _reservation: reservation,
            },
        ))
    }

    pub(crate) fn is_pretokenized(&self) -> bool {
        self.pretokenized
    }

    #[allow(clippy::too_many_arguments)]
    fn prepare_owned_attempt<F>(
        &self,
        provider_id: &str,
        protocol: &str,
        target_url: &str,
        auth_principal: Option<&str>,
        operation: Option<&crate::proxy::forwarder::DlpOperationDescriptor<'_>>,
        enriched_body: &mut Value,
        transform_and_override: F,
    ) -> Result<(DlpPreparedAttempt, DlpTransformInputOwner), DlpForwardError>
    where
        F: FnOnce(
            Value,
            &DlpHistoryTransform<'_>,
        ) -> Result<(Value, Option<DlpHistoryMapping>), DlpForwardError>,
    {
        if self.native_protocol.is_some() {
            return self.prepare_native_chat_attempt(
                provider_id,
                protocol,
                target_url,
                auth_principal,
                operation,
                enriched_body,
                transform_and_override,
            );
        }
        let mut refusal_audit = None;
        let result = (|| {
            let (mut attempt_lease, binding) = self
                .authority
                .fork_bound_attempt(provider_id, protocol, target_url, auth_principal, operation)
                .map_err(|error| {
                    DlpForwardError::from_token_error(error.error)
                        .at_context_checkpoint(error.checkpoint)
                })?;

            let provenance =
                super::restoration_core::AttemptProvenance::new(&self.authority.budget())
                    .map_err(DlpForwardError::from_token_error)?;
            let mut projection = attempt_lease.history_projection(&binding, enriched_body);
            if let Some(projection) = &mut projection {
                projection.restrict_to_origin(self.source_body.as_value());
            }
            refusal_audit = PendingRequestAudit::capture_refusal_with_history(
                enriched_body,
                &self.runtime,
                self.authority.budget(),
                projection.as_ref(),
            )
            .ok();
            let attempt_history_classification = responses_adapter::classify_request_history(
                enriched_body,
                &self.authority.budget(),
                projection.as_ref(),
            )
            .map_err(|error| {
                DlpForwardError::from_token_error(error)
                    .at_context_checkpoint(ContextCheckpoint::HistoryClassify)
            })?;
            let history = extract_request_history(
                enriched_body,
                &self.authority.budget(),
                projection.as_ref(),
                attempt_history_classification,
            )
            .map_err(|error| {
                DlpForwardError::from_token_error(error)
                    .at_context_checkpoint(ContextCheckpoint::AttemptHistory)
            })?;

            let mut opaque_refs = PayloadVec::new(self.authority.budget())
                .map_err(DlpForwardError::from_token_error)?;
            for o in history.opaques.as_slice() {
                opaque_refs
                    .push(o.as_str())
                    .map_err(DlpForwardError::from_token_error)?;
            }
            let mut orphan_refs = PayloadVec::new(self.authority.budget())
                .map_err(DlpForwardError::from_token_error)?;
            for cid in history.orphan_output_call_ids.as_slice() {
                orphan_refs
                    .push(cid.as_str())
                    .map_err(DlpForwardError::from_token_error)?;
            }

            let mut token_candidates = PayloadVec::new(self.authority.budget())
                .map_err(DlpForwardError::from_token_error)?;
            for t in history.referenced_tokens.as_slice() {
                token_candidates
                    .push(t.as_str())
                    .map_err(DlpForwardError::from_token_error)?;
            }
            attempt_lease
                .resume_matching_completed_tokens(&binding, token_candidates.as_slice())
                .map_err(|error| {
                    DlpForwardError::from_token_error(error)
                        .at_context_checkpoint(ContextCheckpoint::TokenResume)
                })?;
            let mut token_refs = PayloadVec::new(self.authority.budget())
                .map_err(DlpForwardError::from_token_error)?;
            for &reference in token_candidates.as_slice() {
                if attempt_lease
                    .resolve_if_authorized(reference)
                    .map_err(DlpForwardError::from_token_error)?
                    .is_some()
                {
                    token_refs
                        .push(reference)
                        .map_err(DlpForwardError::from_token_error)?;
                }
            }

            let dependency_projection = self
                .authority
                .lease()
                .history_admission_candidate_projection(self.source_body.as_value());
            // Freeze dependency obligations with the same candidate used for call selection.
            let source_history_classification = responses_adapter::classify_request_history(
                self.source_body.as_value(),
                &self.authority.budget(),
                dependency_projection.as_ref(),
            )
            .map_err(|error| {
                DlpForwardError::from_token_error(error)
                    .at_context_checkpoint(ContextCheckpoint::HistoryClassify)
            })?;
            let requires_completed_context = matches!(
                source_history_classification,
                RequestHistoryClassification::ExplicitDependency
            ) || matches!(
                attempt_history_classification,
                RequestHistoryClassification::ExplicitDependency
            ) || !token_refs.as_slice().is_empty();
            // Only unchanged, fully readable client history can separate unrelated calls.
            // Real references, explicit/opaque dependencies and cache contributions keep all witnesses.
            let per_call_readable_history = dependency_projection.is_some()
                && token_refs.as_slice().is_empty()
                && opaque_refs.as_slice().is_empty()
                && self.source_body.as_value().get("input") == enriched_body.get("input")
                && responses_adapter::is_fresh_readable_history_for_call_selection(
                    self.source_body.as_value(),
                    &self.authority.budget(),
                )
                .map_err(|error| {
                    DlpForwardError::from_token_error(error)
                        .at_context_checkpoint(ContextCheckpoint::HistoryClassify)
                })?;
            let mut call_specs = PayloadVec::new(self.authority.budget())
                .map_err(DlpForwardError::from_token_error)?;
            for c in history.calls.as_slice() {
                if per_call_readable_history
                    && !self
                        .source_body
                        .as_value()
                        .get("input")
                        .and_then(Value::as_array)
                        .is_some_and(|items| {
                            items.iter().any(|item| {
                                item.get("call_id").and_then(Value::as_str)
                                    == Some(c.call_id.as_str())
                                    && dependency_projection
                                        .as_ref()
                                        .is_some_and(|p| p.has_recorded_call_identity(item))
                            })
                        })
                {
                    continue;
                }
                call_specs
                    .push(CompletedCallSpec {
                        call_id: c.call_id.as_str(),
                        item_type: c.item_type.as_str(),
                        name: c.name.as_str(),
                        payload: c.payload.as_str(),
                        reasoning: c.reasoning.as_ref().map(PayloadText::as_str),
                        encrypted_reasoning: c
                            .encrypted_reasoning
                            .as_ref()
                            .map(PayloadText::as_str),
                    })
                    .map_err(DlpForwardError::from_token_error)?;
            }
            #[cfg(test)]
            completion_selection_test_support::after_selection();
            drop(dependency_projection);

            if requires_completed_context
                || (per_call_readable_history && !call_specs.as_slice().is_empty())
            {
                let mut resume_reason = None;
                if let Err(error) = attempt_lease.resume_completed_context_diagnosed(
                    &binding,
                    history
                        .previous_response_id
                        .as_ref()
                        .map(PayloadText::as_str),
                    call_specs.as_slice(),
                    opaque_refs.as_slice(),
                    &mut resume_reason,
                ) {
                    let error = DlpForwardError::from_token_error(error);
                    if optional_history_miss(error).is_ok() {
                        super::restore_diagnostics::log_context_resume_failure(
                            resume_reason.unwrap_or(
                                super::restore_diagnostics::ContextResumeReason::Unknown,
                            ),
                        );
                        // Restoration authority is optional; missing memory-only history
                        // must not block the otherwise independently inspected request.
                    } else {
                        return Err(error);
                    }
                }

                if let Err(error) = attempt_lease.verify_output_call_ids(
                    &binding,
                    history
                        .previous_response_id
                        .as_ref()
                        .map(PayloadText::as_str),
                    orphan_refs.as_slice(),
                ) {
                    let error = DlpForwardError::from_token_error(error)
                        .at_context_checkpoint(ContextCheckpoint::OutputCallIds);
                    optional_history_miss(error)?;
                }
            }

            let mut rule_refs = PayloadVec::new(self.authority.budget())
                .map_err(DlpForwardError::from_token_error)?;
            for r in self.restore_rules.as_slice() {
                rule_refs
                    .push(r.as_str())
                    .map_err(DlpForwardError::from_token_error)?;
            }

            let pending_audit = PendingRequestAudit::capture_with_history(
                enriched_body,
                &self.runtime,
                rule_refs.as_slice(),
                &attempt_lease,
                self.authority.budget(),
                projection.as_ref(),
            )
            .map_err(|error| {
                DlpForwardError::from_plan_error(error)
                    .at_context_checkpoint(ContextCheckpoint::AttemptAudit)
            })?;
            let prepared = responses_adapter::prepare_request_with_history(
                &mut attempt_lease,
                enriched_body,
                &self.runtime,
                rule_refs.as_slice(),
                token_refs.as_slice(),
                self.authority.budget(),
                projection.as_ref(),
                |candidate_val, _encoded, _proof| {
                    visit_request_references(candidate_val, projection.as_ref(), |_| Ok(()))
                },
            )
            .map_err(|error| {
                DlpForwardError::from_plan_error(error)
                    .at_context_checkpoint(ContextCheckpoint::RequestPrepare)
            })?;

            #[cfg(test)]
            transform_input_test_support::prepared(prepared.body.as_value());
            // Declare the reservation owner before every moved form of its tree.
            // Retaining it does not authorize allocations made by the callback.
            let history_mapping = projection
                .as_ref()
                .and_then(|p| p.responses_mapping(prepared.body.as_value()));
            drop(projection);
            let input_owner;
            let prepared_body = {
                let (body, reservation) = prepared.body.into_parts();
                input_owner = DlpTransformInputOwner {
                    _reservation: reservation,
                };
                body
            };
            let context = DlpHistoryTransform {
                lease: &attempt_lease,
                binding: &binding,
                origin: self.source_body.as_value(),
            };
            let (final_body, converted_mapping) = transform_and_override(prepared_body, &context)
                .map_err(|error| {
                error.at_context_checkpoint(ContextCheckpoint::RequestTransform)
            })?;
            let history_mapping = if protocol == "openai_responses" {
                history_mapping
            } else {
                converted_mapping
            };
            validate_final_outbound_body(
                &final_body,
                protocol,
                &attempt_lease,
                FinalOutboundContext {
                    runtime: &self.runtime,
                    budget: &self.authority.budget(),
                    audit_state: &self.audit_state,
                    provider_id,
                    binding: Some(&binding),
                    history: history_mapping.as_ref(),
                },
            )
            .map_err(|error| error.at_context_checkpoint(ContextCheckpoint::FinalValidation))?;
            *enriched_body = final_body;

            Ok((
                DlpPreparedAttempt {
                    lease: attempt_lease,
                    binding,
                    budget: self.authority.budget(),
                    endpoint_is_compact: self.endpoint_is_compact,
                    upstream_json_protocol: match protocol {
                        "openai_responses" if self.endpoint_is_compact => {
                            Some(JsonCompletionProtocol::ResponsesCompact)
                        }
                        "openai_responses" => Some(JsonCompletionProtocol::NativeResponses),
                        "openai_chat" => Some(JsonCompletionProtocol::ChatCompletions),
                        "anthropic" => Some(JsonCompletionProtocol::AnthropicMessages),
                        _ => None,
                    },
                    provenance,
                    json_completion: completion_verifier::JsonCompletionVerifier::new(),
                    audit_state: std::sync::Arc::clone(&self.audit_state),
                    initial_audit: std::sync::Arc::clone(&self.initial_audit),
                    pending_audit,
                    upstream_payload_owner: None,
                    history_mapping,
                    native_history_digest: None,
                    native_protocol: None,
                    native_original: None,
                    native_error_safety: None,
                    sse_evidence: std::sync::Mutex::new(None),
                },
                input_owner,
            ))
        })();
        if result.is_err() {
            self.audit_state
                .record_refusal(&self.initial_audit, provider_id, None);
            if let Some(pending) = &refusal_audit {
                self.audit_state.record_refusal(pending, provider_id, None);
            }
        }
        result
    }

    #[cfg(test)]
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn prepare_attempt<F>(
        &self,
        provider_id: &str,
        protocol: &str,
        target_url: &str,
        auth_principal: Option<&str>,
        enriched_body: &mut Value,
        mut transform_and_override: F,
    ) -> Result<DlpPreparedAttempt, DlpForwardError>
    where
        F: FnMut(&Value) -> Result<Value, DlpForwardError>,
    {
        // Test-only borrowed callbacks may allocate distinct output trees. The
        // input reservation never follows or accounts for those copies.
        self.prepare_owned_attempt(
            provider_id,
            protocol,
            target_url,
            auth_principal,
            None,
            enriched_body,
            |body, _context| transform_and_override(&body).map(|body| (body, None)),
        )
        .map(|(attempt, _input_owner)| attempt)
    }

    #[cfg(test)]
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn prepare_owned_attempt_for_proxy<F>(
        &self,
        provider_id: &str,
        protocol: &str,
        target_url: &str,
        auth_principal: Option<&str>,
        enriched_body: &mut Value,
        transform_and_override: F,
    ) -> Result<(DlpPreparedAttempt, DlpTransformInputOwner), crate::proxy::error::ProxyError>
    where
        F: FnOnce(Value) -> Result<Value, crate::proxy::error::ProxyError>,
    {
        self.prepare_owned_attempt_with_operation_for_proxy(
            None,
            provider_id,
            protocol,
            target_url,
            auth_principal,
            enriched_body,
            transform_and_override,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn prepare_owned_attempt_with_operation_for_proxy<F>(
        &self,
        operation: Option<&crate::proxy::forwarder::DlpOperationDescriptor<'_>>,
        provider_id: &str,
        protocol: &str,
        target_url: &str,
        auth_principal: Option<&str>,
        enriched_body: &mut Value,
        transform_and_override: F,
    ) -> Result<(DlpPreparedAttempt, DlpTransformInputOwner), crate::proxy::error::ProxyError>
    where
        F: FnOnce(Value) -> Result<Value, crate::proxy::error::ProxyError>,
    {
        self.prepare_owned_attempt_with_history_for_proxy(
            operation,
            provider_id,
            protocol,
            target_url,
            auth_principal,
            enriched_body,
            |body, _context| transform_and_override(body).map(|body| (body, None)),
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn prepare_owned_attempt_with_history_for_proxy<F>(
        &self,
        operation: Option<&crate::proxy::forwarder::DlpOperationDescriptor<'_>>,
        provider_id: &str,
        protocol: &str,
        target_url: &str,
        auth_principal: Option<&str>,
        enriched_body: &mut Value,
        transform_and_override: F,
    ) -> Result<(DlpPreparedAttempt, DlpTransformInputOwner), crate::proxy::error::ProxyError>
    where
        F: FnOnce(
            Value,
            &DlpHistoryTransform<'_>,
        )
            -> Result<(Value, Option<DlpHistoryMapping>), crate::proxy::error::ProxyError>,
    {
        let mut proxy_err: Option<crate::proxy::error::ProxyError> = None;
        let res = self.prepare_owned_attempt(
            provider_id,
            protocol,
            target_url,
            auth_principal,
            operation,
            enriched_body,
            |prepared_body, context| match transform_and_override(prepared_body, context) {
                Ok(val) => Ok(val),
                Err(crate::proxy::error::ProxyError::DlpForward(dlp)) => Err(dlp),
                Err(other) => {
                    proxy_err = Some(other);
                    Err(DlpForwardError::RedactionFailed)
                }
            },
        );
        match res {
            Ok(attempt) => Ok(attempt),
            Err(dlp) => {
                if let Some(other) = proxy_err {
                    Err(other)
                } else {
                    Err(crate::proxy::error::ProxyError::DlpForward(dlp))
                }
            }
        }
    }

    #[cfg(test)]
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn prepare_attempt_for_proxy<F>(
        &self,
        provider_id: &str,
        protocol: &str,
        target_url: &str,
        auth_principal: Option<&str>,
        enriched_body: &mut Value,
        mut transform_and_override: F,
    ) -> Result<DlpPreparedAttempt, crate::proxy::error::ProxyError>
    where
        F: FnMut(&Value) -> Result<Value, crate::proxy::error::ProxyError>,
    {
        let mut proxy_err: Option<crate::proxy::error::ProxyError> = None;
        let res = self.prepare_attempt(
            provider_id,
            protocol,
            target_url,
            auth_principal,
            enriched_body,
            |prepared_body| match transform_and_override(prepared_body) {
                Ok(val) => Ok(val),
                Err(crate::proxy::error::ProxyError::DlpForward(dlp)) => Err(dlp),
                Err(other) => {
                    proxy_err = Some(other);
                    Err(DlpForwardError::RedactionFailed)
                }
            },
        );
        match res {
            Ok(attempt) => Ok(attempt),
            Err(dlp) => {
                if let Some(other) = proxy_err {
                    Err(other)
                } else {
                    Err(crate::proxy::error::ProxyError::DlpForward(dlp))
                }
            }
        }
    }
}

fn validate_non_carrier_tokens(value: &Value, proof: &[&str]) -> Result<(), TokenError> {
    let mut all_tokens = Vec::new();
    visit_value_tokens(value, &mut |tok| {
        if !proof.contains(&tok) {
            return Err(TokenError::ContextUnavailable);
        }
        all_tokens.push(tok.to_owned());
        Ok(())
    })?;
    Ok(())
}

fn visit_value_tokens<'a>(
    value: &'a Value,
    visitor: &mut impl FnMut(&'a str) -> Result<(), TokenError>,
) -> Result<(), TokenError> {
    match value {
        Value::String(s) => visit_token_references(s, |range| visitor(&s[range])),
        Value::Array(items) => {
            for item in items {
                visit_value_tokens(item, visitor)?;
            }
            Ok(())
        }
        Value::Object(map) => {
            for (k, v) in map {
                visit_token_references(k, |range| visitor(&k[range]))?;
                visit_value_tokens(v, visitor)?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

// These borrowed projections are inspection-only; never replace the outbound body.
#[derive(serde::Serialize)]
#[serde(untagged)]
enum FinalInspectionItem<'a> {
    Text(&'a str),
    Arguments {
        #[serde(rename = "type")]
        kind: &'static str,
        arguments: &'a Value,
    },
    // The wrapper keeps scalar metadata read-only in the canonical carrier plan.
    Metadata {
        metadata: &'a Value,
    },
    ReadOnly(&'a Value),
}

#[derive(serde::Serialize)]
struct FinalInspectionBody<'a, T: ?Sized> {
    input: &'a T,
}

fn collect_final_metadata<'a>(
    value: &'a Value,
    keys: &[&str],
    items: &mut PayloadVec<FinalInspectionItem<'a>>,
) -> Result<(), TokenError> {
    for key in keys {
        if let Some(metadata) = value.get(*key) {
            items.push(FinalInspectionItem::Metadata { metadata })?;
        }
    }
    Ok(())
}

fn collect_final_content<'a>(
    content: &'a Value,
    anthropic: bool,
    items: &mut PayloadVec<FinalInspectionItem<'a>>,
) -> Result<(), TokenError> {
    match content {
        Value::String(text) => items.push(FinalInspectionItem::Text(text))?,
        Value::Array(blocks) => {
            for block in blocks {
                collect_final_content(block, anthropic, items)?;
            }
        }
        Value::Object(_) => match content.get("type").and_then(Value::as_str) {
            Some("text" | "input_text" | "output_text") => {
                if let Some(text) = content.get("text").and_then(Value::as_str) {
                    items.push(FinalInspectionItem::Text(text))?;
                }
            }
            Some("tool_use") if anthropic => {
                collect_final_metadata(content, &["id", "name"], items)?;
                if let Some(arguments) = content.get("input") {
                    items.push(FinalInspectionItem::Arguments {
                        kind: "function_call",
                        arguments,
                    })?;
                }
            }
            Some("tool_result") if anthropic => {
                collect_final_metadata(content, &["tool_use_id"], items)?;
                if let Some(result) = content.get("content") {
                    collect_final_content(result, anthropic, items)?;
                }
            }
            Some("thinking") if anthropic => {
                if let Some(text) = content.get("thinking").and_then(Value::as_str) {
                    items.push(FinalInspectionItem::Text(text))?;
                }
            }
            Some(
                "image" | "image_url" | "input_image" | "audio" | "input_audio" | "output_audio"
                | "file" | "input_file" | "document" | "video" | "input_video"
                | "redacted_thinking",
            ) => {}
            // Unknown content remains observable but has no rewrite authority.
            _ => items.push(FinalInspectionItem::ReadOnly(content))?,
        },
        _ => {}
    }
    Ok(())
}

struct FinalInspectionCarriers {
    carriers: CodexRequestCarriers,
    // Flattened ordinal -> actual protocol coordinate, never a text lookup.
    targets: PayloadVec<(usize, super::history_provenance::DlpHistoryTarget)>,
    native: bool,
}
impl FinalInspectionCarriers {
    fn policy_for_path(
        &self,
        path: &[super::history_provenance::HistoryPathStep<'_>],
        policy: &impl Fn(super::history_provenance::DlpHistoryTarget) -> HistoryLeafPolicy,
    ) -> HistoryLeafPolicy {
        use super::history_provenance::HistoryPathStep::{Field, Index};
        if self.native {
            return history_target_for_path(path).map_or(HistoryLeafPolicy::Strict, policy);
        }
        if let [Field("input"), Index(index)] = path {
            if let Some((_, target)) = self
                .targets
                .as_slice()
                .iter()
                .find(|(ordinal, _)| ordinal == index)
            {
                return policy(*target);
            }
        }
        HistoryLeafPolicy::Strict
    }
}

fn final_inspection_carriers(
    body: &Value,
    protocol: &str,
    budget: &PayloadBudget,
    for_refusal_audit: bool,
) -> Result<FinalInspectionCarriers, DlpForwardError> {
    let collect = |value: &Value| {
        if for_refusal_audit {
            CodexRequestCarriers::from_body_for_refusal_audit(value, budget.clone())
        } else {
            CodexRequestCarriers::from_body_with_budget(value, budget.clone())
        }
        .map_err(DlpForwardError::from_token_error)
    };
    let mut targets = PayloadVec::new(budget.clone()).map_err(DlpForwardError::from_token_error)?;
    if protocol == "openai_responses" {
        return Ok(FinalInspectionCarriers {
            carriers: collect(body)?,
            targets,
            native: true,
        });
    }
    if protocol == "gemini_native" {
        let mut items =
            PayloadVec::new(budget.clone()).map_err(DlpForwardError::from_token_error)?;
        collect_final_gemini(body, &mut items).map_err(DlpForwardError::from_token_error)?;
        let projection = final_inspection_json(
            &FinalInspectionBody {
                input: items.as_slice(),
            },
            budget,
        )
        .map_err(DlpForwardError::from_token_error)?;
        return Ok(FinalInspectionCarriers {
            carriers: collect(projection.as_value())?,
            targets,
            native: false,
        });
    }
    let anthropic = match protocol {
        "openai_chat" => false,
        "anthropic" => true,
        _ => return Err(DlpForwardError::RestoreUnsupported),
    };
    let mut items = PayloadVec::new(budget.clone()).map_err(DlpForwardError::from_token_error)?;
    if anthropic {
        if let Some(system) = body.get("system") {
            collect_final_content(system, anthropic, &mut items)
                .map_err(DlpForwardError::from_token_error)?;
        }
    }
    if let Some(messages) = body.get("messages").and_then(Value::as_array) {
        for (message_index, message) in messages.iter().enumerate() {
            if let Some(content) = message.get("content") {
                if !anthropic && content.is_string() {
                    targets
                        .push((
                            items.len(),
                            super::history_provenance::DlpHistoryTarget::ChatContent {
                                message_index,
                            },
                        ))
                        .map_err(DlpForwardError::from_token_error)?;
                }
                collect_final_content(content, anthropic, &mut items)
                    .map_err(DlpForwardError::from_token_error)?;
            }
            if !anthropic {
                collect_final_metadata(message, &["name", "tool_call_id"], &mut items)
                    .map_err(DlpForwardError::from_token_error)?;
                for key in ["reasoning_content", "reasoning", "refusal"] {
                    if let Some(text) = message.get(key).and_then(Value::as_str) {
                        if key == "reasoning_content" {
                            targets
                                .push((
                                    items.len(),
                                    super::history_provenance::DlpHistoryTarget::ChatReasoning {
                                        message_index,
                                    },
                                ))
                                .map_err(DlpForwardError::from_token_error)?;
                        }
                        items
                            .push(FinalInspectionItem::Text(text))
                            .map_err(DlpForwardError::from_token_error)?;
                    }
                }
                if let Some(calls) = message.get("tool_calls").and_then(Value::as_array) {
                    for call in calls {
                        collect_final_metadata(call, &["id"], &mut items)
                            .map_err(DlpForwardError::from_token_error)?;
                        if let Some(function) = call.get("function") {
                            collect_final_metadata(function, &["name"], &mut items)
                                .map_err(DlpForwardError::from_token_error)?;
                        }
                        if let Some(arguments) = call.pointer("/function/arguments") {
                            items
                                .push(FinalInspectionItem::Arguments {
                                    kind: "function_call",
                                    arguments,
                                })
                                .map_err(DlpForwardError::from_token_error)?;
                        }
                    }
                }
                if let Some(function) = message.get("function_call") {
                    collect_final_metadata(function, &["name"], &mut items)
                        .map_err(DlpForwardError::from_token_error)?;
                }
                if let Some(arguments) = message.pointer("/function_call/arguments") {
                    items
                        .push(FinalInspectionItem::Arguments {
                            kind: "function_call",
                            arguments,
                        })
                        .map_err(DlpForwardError::from_token_error)?;
                }
            }
        }
    }
    let projection = final_inspection_json(
        &FinalInspectionBody {
            input: items.as_slice(),
        },
        budget,
    )
    .map_err(DlpForwardError::from_token_error)?;
    Ok(FinalInspectionCarriers {
        carriers: collect(projection.as_value())?,
        targets,
        native: false,
    })
}

/// Native Gemini text carriers only: generated Parts, ProtoJSON system spellings and
/// Struct-valued function arguments/results. Media, signatures and unknown members stay opaque.
fn collect_final_gemini<'a>(
    body: &'a Value,
    items: &mut PayloadVec<FinalInspectionItem<'a>>,
) -> Result<(), TokenError> {
    for field in ["systemInstruction", "system_instruction"] {
        if let Some(instruction) = body.get(field) {
            collect_final_gemini_content(instruction, items)?;
        }
    }
    if let Some(contents) = body.get("contents").and_then(Value::as_array) {
        for content in contents {
            collect_final_gemini_content(content, items)?;
        }
    }
    Ok(())
}

fn collect_final_gemini_content<'a>(
    content: &'a Value,
    items: &mut PayloadVec<FinalInspectionItem<'a>>,
) -> Result<(), TokenError> {
    let Some(parts) = content.get("parts").and_then(Value::as_array) else {
        return Ok(());
    };
    for part in parts {
        if let Some(text) = part.get("text").and_then(Value::as_str) {
            items.push(FinalInspectionItem::Text(text))?;
        }
        for (carrier, field) in [("functionCall", "args"), ("functionResponse", "response")] {
            if let Some(value) = part
                .get(carrier)
                .and_then(|function| function.get(field))
                .filter(|value| value.is_object())
            {
                items.push(FinalInspectionItem::Arguments {
                    kind: "function_call",
                    arguments: value,
                })?;
            }
        }
    }
    Ok(())
}

fn final_inspection_json(
    value: &impl serde::Serialize,
    budget: &PayloadBudget,
) -> Result<PayloadJson, TokenError> {
    struct Writer {
        buffer: PayloadBuffer,
        error: Option<TokenError>,
    }
    impl std::io::Write for Writer {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            if let Err(error) = self.buffer.append(bytes) {
                self.error = Some(error);
                return Err(std::io::Error::other(error.code()));
            }
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut writer = Writer {
        buffer: PayloadBuffer::new(budget.clone(), budget.limit())?,
        error: None,
    };
    if serde_json::to_writer(&mut writer, value).is_err() {
        return Err(writer.error.unwrap_or(TokenError::RedactionFailed));
    }
    let encoded =
        std::str::from_utf8(writer.buffer.as_slice()).map_err(|_| TokenError::RedactionFailed)?;
    PayloadJson::try_from_str(encoded, budget.clone(), budget.limit())
}

fn audit_final_refusal(
    carriers: &FinalInspectionCarriers,
    policy: &impl Fn(super::history_provenance::DlpHistoryTarget) -> HistoryLeafPolicy,
    runtime: &SecurityRuntimeConfig,
    budget: &PayloadBudget,
    audit_state: &RequestAuditState,
    provider_id: &str,
) -> Result<(), TokenError> {
    carriers.carriers.visit_with_path_policy(
        |path| carriers.policy_for_path(path, policy),
        |leaf| {
            if leaf.policy == HistoryLeafPolicy::DeliveredReasoning {
                return Ok(());
            }
            // Flatten only decoded inspection text; schemas, keys and opaque bytes are absent.
            #[derive(serde::Serialize)]
            struct PasswordArguments<'a> {
                password: &'a str,
            }
            #[derive(serde::Serialize)]
            struct PasswordItem<'a> {
                #[serde(rename = "type")]
                kind: &'static str,
                arguments: PasswordArguments<'a>,
            }
            let body = if leaf.is_password_field {
                final_inspection_json(
                    &FinalInspectionBody {
                        input: &[PasswordItem {
                            kind: "function_call",
                            arguments: PasswordArguments {
                                password: leaf.text,
                            },
                        }],
                    },
                    budget,
                )?
            } else {
                final_inspection_json(&FinalInspectionBody { input: &leaf.text }, budget)?
            };
            let pending =
                PendingRequestAudit::capture_refusal(body.as_value(), runtime, budget.clone())?;
            audit_state.record_refusal(&pending, provider_id, None);
            Ok(())
        },
    )
}

struct FinalOutboundContext<'a> {
    runtime: &'a SecurityRuntimeConfig,
    budget: &'a PayloadBudget,
    audit_state: &'a RequestAuditState,
    provider_id: &'a str,
    binding: Option<&'a ActualAttemptBinding>,
    history: Option<&'a DlpHistoryMapping>,
}

fn validate_final_outbound_body(
    body: &Value,
    protocol: &str,
    lease: &DlpRequestLease,
    context: FinalOutboundContext<'_>,
) -> Result<(), DlpForwardError> {
    let FinalOutboundContext {
        runtime,
        budget,
        audit_state,
        provider_id,
        binding,
        history,
    } = context;
    // Bound the actual tree before recursive token checks or borrowed projection traversal.
    let bounded_body = PayloadJson::try_from_value(body, budget.clone(), budget.limit())
        .map_err(DlpForwardError::from_token_error)?;
    let policy = |target| {
        history
            .zip(binding)
            .map_or(HistoryLeafPolicy::Strict, |(mapping, binding)| {
                mapping.policy_for_target(lease, binding, body, target)
            })
    };
    let carriers = match final_inspection_carriers(bounded_body.as_value(), protocol, budget, false)
    {
        Ok(carriers) => carriers,
        Err(error) => {
            // Collect valid siblings for refusal evidence only; this path never authorizes a send.
            let audit = final_inspection_carriers(bounded_body.as_value(), protocol, budget, true)
                .and_then(|carriers| {
                    audit_final_refusal(
                        &carriers,
                        &policy,
                        runtime,
                        budget,
                        audit_state,
                        provider_id,
                    )
                    .map_err(DlpForwardError::from_token_error)
                });
            if audit.is_err() {
                log::warn!("[SecurityAudit] stage=final_refusal category=unavailable");
            }
            return Err(error);
        }
    };
    let result = (|| {
        super::request_carriers::visit_bounded_key_references(
            bounded_body.as_value(),
            &mut |reference| {
                if lease.resolve_if_authorized(reference)?.is_some() {
                    return Err(TokenError::ContextUnavailable);
                }
                Ok(())
            },
        )
        .map_err(DlpForwardError::from_token_error)?;
        carriers
            .carriers
            .visit_key_references(|reference| {
                if lease.resolve_if_authorized(reference)?.is_some() {
                    return Err(TokenError::ContextUnavailable);
                }
                Ok(())
            })
            .map_err(DlpForwardError::from_token_error)?;
        visit_references_with_policy(
            body,
            |path| history_target_for_path(path).map_or(HistoryLeafPolicy::Strict, policy),
            |tok| lease.resolve_if_authorized(tok).map(|_| ()),
        )
        .map_err(DlpForwardError::from_token_error)?;
        let mut refusal = None;
        carriers
            .carriers
            .visit_with_path_policy(
                |path| carriers.policy_for_path(path, &policy),
                |leaf| {
                    if leaf.policy == HistoryLeafPolicy::DeliveredReasoning {
                        return Ok(());
                    }
                    let mut refs = PayloadVec::new(budget.clone())?;
                    leaf.policy.visit_references(leaf.text, |range| {
                        if lease
                            .resolve_if_authorized(&leaf.text[range.clone()])?
                            .is_some()
                        {
                            refs.push(range)?;
                        }
                        Ok(())
                    })?;
                    for found in detect_spans_with_budget(
                        leaf.text,
                        leaf.is_password_field,
                        runtime,
                        budget.clone(),
                    )?
                    .as_slice()
                    {
                        if !matches!(
                            found.action,
                            EffectiveAction::Redact
                                | EffectiveAction::Restore
                                | EffectiveAction::Block
                        ) {
                            continue;
                        }
                        if found.range.is_empty()
                            || !refs.as_slice().iter().any(|range| {
                                range.start <= found.range.start && found.range.end <= range.end
                            })
                        {
                            refusal = Some(if found.action == EffectiveAction::Block {
                                DlpForwardError::Blocked
                            } else {
                                DlpForwardError::RedactionFailed
                            });
                        }
                    }
                    Ok(())
                },
            )
            .map_err(DlpForwardError::from_token_error)?;
        refusal.map_or(Ok(()), Err)
    })();
    if result.is_err() {
        // Auditing is auxiliary: an audit failure cannot turn a refusal into forwarding.
        if audit_final_refusal(
            &carriers,
            &policy,
            runtime,
            budget,
            audit_state,
            provider_id,
        )
        .is_err()
        {
            log::warn!("[SecurityAudit] stage=final_refusal category=unavailable");
        }
    }
    result
}

impl DlpPreparedAttempt {
    pub(crate) fn has_active_tokens(&self) -> bool {
        !self.lease.is_empty()
    }

    pub(super) fn budget(&self) -> PayloadBudget {
        self.budget.clone()
    }

    fn response_protocol_pair(
        &self,
    ) -> Result<super::restoration_contract::ProtocolPair, RestoreError> {
        use super::restoration_contract::{ClientProtocol, ProtocolPair, UpstreamProtocol};
        use crate::proxy::protocol_bridge::Protocol;
        let upstream = match self.upstream_json_protocol {
            Some(
                JsonCompletionProtocol::NativeResponses | JsonCompletionProtocol::ResponsesCompact,
            ) => Protocol::Responses,
            Some(JsonCompletionProtocol::ChatCompletions) => Protocol::Chat,
            Some(JsonCompletionProtocol::AnthropicMessages) => Protocol::Anthropic,
            None => return Err(RestoreError::RestoreFailed),
        };
        Ok(ProtocolPair::new(
            ClientProtocol::new(Protocol::Responses),
            UpstreamProtocol::new(upstream),
        ))
    }

    pub(crate) fn verify_upstream_json_completion(
        &self,
        protocol: JsonCompletionProtocol,
        raw_json: &str,
    ) -> Result<(), RestoreError> {
        self.json_completion.observe_json(
            self.upstream_json_protocol,
            protocol,
            raw_json,
            &self.budget,
            &self.lease,
            &self.binding,
            self.response_protocol_pair()?,
            &self.provenance,
        )
    }

    #[cfg(test)]
    fn restore_json_response(
        self,
        status: u16,
        body: &Value,
        _norm_mode: ArgumentNormalizationMode,
        observation: Option<&CompletionObservation<'_>>,
    ) -> Result<responses_adapter::RestoredJsonResponse, Box<Response>> {
        // Boxed so this helper's error stays cheap to return; tests only inspect the outcome.
        let witness = self
            .json_completion_witness(status, observation)
            .map_err(|error| Box::new(build_codex_restore_error_response(error)))?;
        stage_json_response(body, self.endpoint_is_compact, &self.lease, &self.budget)
            .and_then(|staged| staged.publish(&self.lease, &self.binding, &witness, &self.budget))
            .map_err(|error| Box::new(build_codex_restore_error_response(error)))
    }

    #[cfg(test)]
    fn json_completion_witness(
        &self,
        status: u16,
        observation: Option<&CompletionObservation<'_>>,
    ) -> Result<CompletionWitness, RestoreError> {
        if status != 200 {
            return Err(RestoreError::RestoreFailed);
        }
        // Converted Responses status is not original-protocol completion evidence.
        // Raw observations may establish evidence only once; failures are sticky.
        if let Some(fact) = observation {
            match fact {
                CompletionObservation::RawJson(raw) => {
                    let protocol = self
                        .upstream_json_protocol
                        .ok_or(RestoreError::RestoreFailed)?;
                    self.verify_upstream_json_completion(protocol, raw)?;
                }
                CompletionObservation::AnthropicJson(_)
                    if matches!(
                        self.upstream_json_protocol,
                        Some(JsonCompletionProtocol::AnthropicMessages)
                    ) => {}
                _ => return Err(RestoreError::RestoreFailed),
            }
        }
        CompletionWitness::from_verified_completion(self.json_completion.permits_completion())
            .map_err(RestoreError::scan_error)
    }

    /// Only the actual xAI SSE branch requests original-byte evidence.
    pub(crate) fn xai_original_sse_evidence_tracker(
        &self,
    ) -> Option<std::sync::Arc<std::sync::Mutex<UpstreamSseEvidenceTracker>>> {
        let mut tracker = UpstreamSseEvidenceTracker::xai_original(self.budget.clone());
        tracker.bind_attempt(self.provenance.clone());
        Some(std::sync::Arc::new(std::sync::Mutex::new(tracker)))
    }

    pub(crate) fn upstream_sse_evidence_tracker(
        &self,
    ) -> Option<std::sync::Arc<std::sync::Mutex<UpstreamSseEvidenceTracker>>> {
        let mut owner = self.sse_evidence.lock().ok()?;
        if let Some(owner) = &*owner {
            return Some(owner.tracker.clone());
        }
        let mut tracker = match self.upstream_json_protocol {
            Some(JsonCompletionProtocol::ChatCompletions) => {
                UpstreamSseEvidenceTracker::chat(self.budget.clone())
            }
            Some(JsonCompletionProtocol::AnthropicMessages) => {
                UpstreamSseEvidenceTracker::anthropic(self.budget.clone())
            }
            _ => return None,
        };
        tracker.bind_attempt(self.provenance.clone());
        match self.response_protocol_pair().and_then(|pair| {
            responses_adapter::ResponsesSseInspection::begin(
                &self.lease,
                &self.binding,
                &self.budget,
                pair,
                self.provenance.clone(),
            )
        }) {
            Ok(inspection) => tracker.inspection = Some(inspection),
            Err(_) => tracker.abort(),
        }
        let tracker = std::sync::Arc::new(std::sync::Mutex::new(tracker));
        *owner = Some(PreparedSseEvidence {
            tracker: tracker.clone(),
        });
        Some(tracker)
    }

    pub(crate) fn verify_upstream_sse_completion(
        &self,
        protocol: JsonCompletionProtocol,
        raw_sse: &str,
    ) -> Result<(), RestoreError> {
        let mut tracker = match (self.upstream_json_protocol, protocol) {
            (
                Some(JsonCompletionProtocol::ChatCompletions),
                JsonCompletionProtocol::ChatCompletions,
            ) => UpstreamSseEvidenceTracker::chat(self.budget.clone()),
            (
                Some(JsonCompletionProtocol::AnthropicMessages),
                JsonCompletionProtocol::AnthropicMessages,
            ) => UpstreamSseEvidenceTracker::anthropic(self.budget.clone()),
            _ => {
                return self
                    .json_completion
                    .observe_sse(Err(RestoreError::RestoreFailed));
            }
        };
        tracker.bind_attempt(self.provenance.clone());
        let checked = (|| -> Result<responses_adapter::ResponsesJsonInspection, RestoreError> {
            let pair = self.response_protocol_pair()?;
            let mut inspection = responses_adapter::ResponsesJsonInspection::begin(
                &self.lease,
                &self.binding,
                &self.budget,
                pair,
                self.provenance.clone(),
            )?;
            tracker.observe_buffered_sse(protocol, raw_sse, |block| {
                let original = super::restoration_contract::OriginalResponseObservation::frame(
                    block.as_bytes(),
                    pair.upstream(),
                    self.provenance.clone(),
                    self.budget.clone(),
                )?;
                inspection.observe_original(&original)
            })?;
            Ok(inspection)
        })();
        self.json_completion
            .observe_sse(checked.and_then(|inspection| Ok((tracker.take()?, inspection))))
    }

    pub(crate) async fn restore_client_http_response(
        self,
        response: Response,
        mode: ArgumentNormalizationMode,
        evidence: Option<std::sync::Arc<std::sync::Mutex<UpstreamSseEvidenceTracker>>>,
    ) -> Response {
        // Move cleanup into the delivered stream only after a successful handoff.
        // Before the first poll, the attempt itself still owns this guard.
        let early_inspection = match self.sse_evidence.lock() {
            Ok(mut owner) => owner.take(),
            Err(_) => {
                return build_codex_restore_error_response_at(
                    RestoreError::RestoreFailed,
                    RestoreStage::SseInit,
                )
            }
        };
        let upstream_owner = self.upstream_payload_owner();
        if !response.status().is_success() {
            let (parts, body) = response.into_parts();
            let stream = body
                .into_data_stream()
                .map(|chunk| chunk.map_err(|_| std::io::Error::other("Response stream failed.")));
            return Response::from_parts(
                parts,
                Body::from_stream(retain_stream_owner(stream, upstream_owner)),
            );
        }
        let (mut parts, body) = response.into_parts();
        let is_sse = parts
            .headers
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| v.contains("text/event-stream"));
        let encoded = parts
            .headers
            .get(header::CONTENT_ENCODING)
            .is_some_and(|value| value != "identity");
        if encoded {
            return build_codex_restore_error_response_at(
                RestoreError::RestoreFailed,
                RestoreStage::ResponseEncoding,
            );
        }
        if is_sse {
            clear_entity_headers(&mut parts.headers);
            let inspection = match evidence
                .as_ref()
                .map(|tracker| {
                    tracker
                        .lock()
                        .map_err(|_| RestoreError::RestoreFailed)
                        .map(|mut tracker| tracker.inspection.take())
                })
                .transpose()
            {
                Ok(inspection) => inspection.flatten(),
                Err(error) => {
                    return build_codex_restore_error_response_at(error, RestoreStage::SseInit)
                }
            };
            let restorer = match self.into_sse_restorer_with_inspection(
                parts.status.as_u16(),
                mode,
                inspection,
            ) {
                Ok(restorer) => restorer,
                Err(error) => {
                    return build_codex_restore_error_response_at(error, RestoreStage::SseInit)
                }
            };
            let stream = body
                .into_data_stream()
                .map(|chunk| chunk.map_err(|_| std::io::Error::other("Response stream failed.")));
            return Response::from_parts(
                parts,
                Body::from_stream(retain_stream_owner(
                    wrap_sse_stream_with_restoration_and_evidence(stream, restorer, evidence),
                    (upstream_owner, early_inspection),
                )),
            );
        }
        let budget = self.budget.clone();
        let mut stage = RestoreStage::JsonRestore;
        let result = async {
            let mut input = PayloadBuffer::new(
                budget.clone(),
                crate::proxy::hyper_client::MAX_RESPONSE_BODY_BYTES,
            )
            .map_err(RestoreError::scan_error)?;
            let mut stream = body.into_data_stream();
            while let Some(chunk) = stream.next().await {
                let chunk = chunk.map_err(|_| RestoreError::RestoreFailed)?;
                input.append(&chunk).map_err(RestoreError::scan_error)?;
            }
            let raw =
                std::str::from_utf8(input.as_slice()).map_err(|_| RestoreError::RestoreFailed)?;
            stage = RestoreStage::CompletionEvidence;
            if self.json_completion.is_unobserved()? {
                match self.upstream_json_protocol {
                    Some(
                        protocol @ (JsonCompletionProtocol::NativeResponses
                        | JsonCompletionProtocol::ResponsesCompact),
                    ) => {
                        self.verify_upstream_json_completion(protocol, raw)?;
                    }
                    _ => return Err(RestoreError::RestoreFailed),
                }
            }
            stage = RestoreStage::JsonRestore;
            let parsed = PayloadJson::try_from_str(raw, budget.clone(), budget.limit())
                .map_err(RestoreError::scan_error)?;
            stage = RestoreStage::CompletionEvidence;
            let pair = self.response_protocol_pair()?;
            let (proof, inspection) = self.json_completion.finish(
                parsed.as_value(),
                &self.binding,
                parts.status.as_u16(),
            )?;
            stage = RestoreStage::JsonRestore;
            let staged = stage_json_response_bound(
                parsed.as_value(),
                self.endpoint_is_compact,
                &self.lease,
                &self.binding,
                &budget,
                pair,
                &self.provenance,
                inspection,
            )?;
            let pending_history = self
                .lease
                .capture_delivered_history(&self.binding, staged.as_value());
            let changed = staged.as_value() != parsed.as_value();
            // Mandatory charged final encoding and activation share one sealed boundary.
            stage = RestoreStage::ContextPublish;
            let (_restored, encoded) = staged.encode_and_publish(
                &self.lease,
                &self.binding,
                proof,
                &budget,
                crate::proxy::hyper_client::MAX_RESPONSE_BODY_BYTES,
            )?;
            let output = if changed {
                encoded
            } else {
                drop(encoded);
                input
            };
            let bytes = bytes_with_owner(output, upstream_owner);
            if let Some(pending) = pending_history {
                let _ = self.lease.publish_delivered_history(&self.binding, pending);
            }
            Ok::<_, RestoreError>((bytes, changed))
        }
        .await;
        match result {
            Ok((bytes, changed)) => {
                if changed {
                    clear_entity_headers(&mut parts.headers);
                }
                if let Ok(length) = HeaderValue::from_str(&bytes.len().to_string()) {
                    parts.headers.insert(header::CONTENT_LENGTH, length);
                }
                Response::from_parts(parts, Body::from(bytes))
            }
            Err(error) => build_codex_restore_error_response_at(error, stage),
        }
    }

    #[cfg(test)]
    pub(super) fn into_sse_restorer(
        self,
        status: u16,
        norm_mode: ArgumentNormalizationMode,
    ) -> Result<SseRestorationState, RestoreError> {
        self.into_sse_restorer_with_inspection(status, norm_mode, None)
    }

    fn into_sse_restorer_with_inspection(
        self,
        status: u16,
        norm_mode: ArgumentNormalizationMode,
        inspection: Option<responses_adapter::ResponsesSseInspection>,
    ) -> Result<SseRestorationState, RestoreError> {
        if status != 200 || self.endpoint_is_compact {
            return Err(RestoreError::RestoreFailed);
        }
        use super::restoration_contract::{ClientProtocol, ProtocolPair, UpstreamProtocol};
        use crate::proxy::protocol_bridge::Protocol;
        let upstream = match self.upstream_json_protocol {
            Some(
                JsonCompletionProtocol::NativeResponses | JsonCompletionProtocol::ResponsesCompact,
            ) => Protocol::Responses,
            Some(JsonCompletionProtocol::ChatCompletions) => Protocol::Chat,
            Some(JsonCompletionProtocol::AnthropicMessages) => Protocol::Anthropic,
            None => return Err(RestoreError::RestoreFailed),
        };
        let pair = ProtocolPair::new(
            ClientProtocol::new(Protocol::Responses),
            UpstreamProtocol::new(upstream),
        );
        let requirement = super::restoration_core::SseEvidenceRequirement::new(
            self.provenance.clone(),
            self.upstream_json_protocol,
            norm_mode,
        );
        let mut restorer = match inspection {
            Some(inspection) => SseRestorationState::from_inspection(
                self.lease,
                self.binding,
                self.budget,
                norm_mode,
                pair,
                &self.provenance,
                inspection,
            )?,
            None => SseRestorationState::new_bound(
                self.lease,
                self.binding,
                self.budget,
                norm_mode,
                pair,
                self.provenance,
            )?,
        };
        restorer.require_original_evidence(requirement);
        Ok(restorer)
    }
}

struct ChargedHttpBytes(PayloadBuffer);
impl AsRef<[u8]> for ChargedHttpBytes {
    fn as_ref(&self) -> &[u8] {
        self.0.as_slice()
    }
}

fn clear_entity_headers(headers: &mut HeaderMap) {
    for name in [
        "content-length",
        "content-encoding",
        "transfer-encoding",
        "etag",
        "digest",
        "content-digest",
        "repr-digest",
        "content-md5",
    ] {
        headers.remove(name);
    }
}

/// Non-authoritative observer shell; completion transfers its single owned state.
pub(crate) struct UpstreamSseEvidenceTracker {
    state: Option<completion_verifier::OriginalEvidence>,
    inspection: Option<responses_adapter::ResponsesSseInspection>,
}

impl UpstreamSseEvidenceTracker {
    pub(super) fn chat(budget: PayloadBudget) -> Self {
        Self {
            state: Some(completion_verifier::OriginalEvidence::chat(budget)),
            inspection: None,
        }
    }

    pub(super) fn anthropic(budget: PayloadBudget) -> Self {
        Self {
            state: Some(completion_verifier::OriginalEvidence::anthropic(budget)),
            inspection: None,
        }
    }

    fn xai_original(budget: PayloadBudget) -> Self {
        Self {
            state: Some(completion_verifier::OriginalEvidence::xai_original(budget)),
            inspection: None,
        }
    }

    fn bind_attempt(&mut self, attempt: super::restoration_core::AttemptProvenance) {
        if let Some(state) = &mut self.state {
            state.bind_attempt(attempt);
        }
    }

    fn abort(&mut self) {
        if let Some(state) = &mut self.state {
            state.abort();
        }
    }

    fn observe_xai_original_frame(&mut self, raw: &str) -> Result<(), RestoreError> {
        self.observe_inspection_frame(raw)?;
        self.state
            .as_mut()
            .ok_or(RestoreError::RestoreFailed)?
            .observe_xai_original_frame(raw)
    }

    fn observe_xai_original_end(&mut self, unfinished_frame: bool, unfinished_utf8: bool) {
        if let Some(state) = &mut self.state {
            state.observe_xai_original_end(unfinished_frame, unfinished_utf8);
        }
    }

    /// Inspect the original buffered entity with the sole final-SSE mechanics owner.
    pub(crate) fn observe_fully_read_anthropic_body(
        &mut self,
        raw: &str,
        is_sse: bool,
    ) -> Result<(), RestoreError> {
        let result = if is_sse {
            self.observe_buffered_sse(JsonCompletionProtocol::AnthropicMessages, raw, |_| Ok(()))
        } else {
            self.observe_fully_read_anthropic_json(raw);
            self.completion_result()
        };
        if result.is_err() {
            self.abort();
        }
        result
    }

    fn observe_buffered_sse(
        &mut self,
        protocol: JsonCompletionProtocol,
        raw_sse: &str,
        mut observe_original: impl FnMut(&str) -> Result<(), RestoreError>,
    ) -> Result<(), RestoreError> {
        let budget = self
            .state
            .as_ref()
            .ok_or(RestoreError::RestoreFailed)?
            .budget();
        let (mut frame_start, mut offset) = (0usize, 0usize);
        for line in raw_sse.split_inclusive('\n') {
            offset += line.len();
            if offset - frame_start > 1024 * 1024 {
                return Err(RestoreError::RestoreFailed);
            }
            if line != "\n" && line != "\r\n" {
                continue;
            }
            let block = &raw_sse[frame_start..offset];
            frame_start = offset;
            observe_original(block)?;
            self.observe(&CompletionObservation::RawFrame(block));
            let mut data = PayloadBuffer::new(budget.clone(), 1024 * 1024)
                .map_err(RestoreError::scan_error)?;
            let mut event = None;
            let mut count = 0usize;
            for line in block.lines() {
                if let Some(value) = line.strip_prefix("event:") {
                    event = Some(value.trim());
                } else if let Some(value) = line
                    .strip_prefix("data:")
                    .or_else(|| (line == "data").then_some(""))
                {
                    if count != 0 {
                        data.append(b"\n").map_err(RestoreError::scan_error)?;
                    }
                    count += 1;
                    data.append(value.strip_prefix(' ').unwrap_or(value).as_bytes())
                        .map_err(RestoreError::scan_error)?;
                }
            }
            if count == 0 {
                continue;
            }
            let raw =
                std::str::from_utf8(data.as_slice()).map_err(|_| RestoreError::RestoreFailed)?;
            if raw.trim() == "[DONE]" {
                self.observe(&CompletionObservation::ChatDone);
            } else {
                let parsed = PayloadJson::try_from_str(raw, budget.clone(), budget.limit())
                    .map_err(RestoreError::scan_error)?;
                match protocol {
                    JsonCompletionProtocol::ChatCompletions => {
                        self.observe(&CompletionObservation::ChatChunk(parsed.as_value()))
                    }
                    JsonCompletionProtocol::AnthropicMessages => {
                        self.observe(&CompletionObservation::AnthropicEvent {
                            event_name: event,
                            data: parsed.as_value(),
                        })
                    }
                    _ => return Err(RestoreError::RestoreFailed),
                }
            }
        }
        self.observe(&CompletionObservation::EntityEnd {
            reached_eof: true,
            unfinished_frame: frame_start != raw_sse.len(),
            unfinished_utf8: false,
        });
        self.completion_result()?;
        Ok(())
    }

    /// Records a fully read original JSON entity, never a synthetic SSE EOF.
    pub(crate) fn observe_fully_read_anthropic_json(&mut self, raw: &str) {
        let parsed = self
            .state
            .as_mut()
            .and_then(|state| state.observe_fully_read_anthropic_json(raw));
        if let Some(inspection) = &mut self.inspection {
            let observed = parsed
                .as_ref()
                .ok_or(RestoreError::RestoreFailed)
                .and_then(|parsed| {
                    let budget = self
                        .state
                        .as_ref()
                        .ok_or(RestoreError::RestoreFailed)?
                        .budget();
                    inspection.observe_json(parsed.as_value(), &budget)
                });
            if observed.is_err() {
                self.abort();
            }
        }
    }

    pub(crate) fn observe(&mut self, fact: &CompletionObservation<'_>) {
        if let CompletionObservation::RawFrame(raw) = fact {
            if self.observe_inspection_frame(raw).is_err() {
                self.abort();
                return;
            }
        }
        if let Some(state) = &mut self.state {
            state.observe(fact);
        }
    }

    fn observe_inspection_frame(&mut self, raw: &str) -> Result<(), RestoreError> {
        if let Some(inspection) = &mut self.inspection {
            let budget = self
                .state
                .as_ref()
                .ok_or(RestoreError::RestoreFailed)?
                .budget();
            inspection.observe_frame(raw, &budget)?;
        }
        Ok(())
    }

    fn has_unsupported_carrier(&self) -> bool {
        self.state
            .as_ref()
            .is_some_and(completion_verifier::OriginalEvidence::has_unsupported_carrier)
    }

    fn completion_result(&self) -> Result<(), RestoreError> {
        self.state
            .as_ref()
            .ok_or(RestoreError::RestoreFailed)?
            .completion_result()
    }

    fn take(&mut self) -> Result<completion_verifier::OriginalEvidence, RestoreError> {
        self.state.take().ok_or(RestoreError::RestoreFailed)
    }
}

/// Owns only cancellation cleanup, from preparation through delivered-stream drop.
/// Keeping an observer Arc alive never extends the mechanics owner's lifetime.
struct PreparedSseEvidence {
    tracker: std::sync::Arc<std::sync::Mutex<UpstreamSseEvidenceTracker>>,
}

impl Drop for PreparedSseEvidence {
    fn drop(&mut self) {
        let mut tracker = match self.tracker.lock() {
            Ok(tracker) => tracker,
            // Poisoned state is accessible only to destroy it, never as evidence.
            Err(poisoned) => poisoned.into_inner(),
        };
        tracker.inspection = None;
        tracker.state = None;
    }
}

#[cfg(test)]
pub(in crate::proxy::security) mod xai_lending_test_support {
    use super::UpstreamSseEvidenceTracker;
    use std::{
        cell::RefCell,
        rc::Rc,
        sync::{Arc, Mutex},
    };

    #[derive(Default)]
    struct State {
        tracker: Option<Arc<Mutex<UpstreamSseEvidenceTracker>>>,
        lends: usize,
        observed: usize,
    }
    thread_local! {
        static PROBE: RefCell<Option<Arc<Mutex<State>>>> = const { RefCell::new(None) };
    }
    pub(in crate::proxy::security) struct Guard(
        Option<Arc<Mutex<State>>>,
        std::marker::PhantomData<Rc<()>>,
    );
    pub(in crate::proxy::security) struct Probe(Arc<Mutex<State>>);
    pub(in crate::proxy::security) fn install() -> (Guard, Probe) {
        let state = Arc::new(Mutex::new(State::default()));
        let previous = PROBE.with(|slot| slot.replace(Some(state.clone())));
        (Guard(previous, std::marker::PhantomData), Probe(state))
    }
    impl Drop for Guard {
        fn drop(&mut self) {
            PROBE.with(|slot| {
                slot.replace(self.0.take());
            });
        }
    }
    impl Probe {
        // Retain the observer shell to detect stranded owners after cancellation.
        // Only counts and occupancy escape; this grants no mechanics or authority.
        pub(in crate::proxy::security) fn snapshot(
            &self,
        ) -> Result<(usize, usize, bool), &'static str> {
            let state = self.0.lock().map_err(|_| "c3_xai_probe_poisoned")?;
            let (lends, observed, tracker) = (state.lends, state.observed, state.tracker.clone());
            drop(state);
            let occupied = match tracker {
                Some(tracker) => tracker
                    .lock()
                    .map_err(|_| "c3_xai_tracker_poisoned")?
                    .inspection
                    .is_some(),
                None => false,
            };
            Ok((lends, observed, occupied))
        }
    }
    pub(in crate::proxy::security) fn lent(tracker: &Arc<Mutex<UpstreamSseEvidenceTracker>>) {
        PROBE.with(|slot| {
            if let Some(probe) = slot.borrow().as_ref() {
                if let Ok(mut state) = probe.lock() {
                    state.tracker = Some(tracker.clone());
                    state.lends += 1;
                }
            }
        });
    }
    pub(in crate::proxy::security) fn observed() {
        PROBE.with(|slot| {
            if let Some(probe) = slot.borrow().as_ref() {
                if let Ok(mut state) = probe.lock() {
                    state.observed += 1;
                }
            }
        });
    }
}

/// A cancellation guard for the sole mechanics owner while a converter is polled.
/// The tracker is plumbing only: it cannot clone, extract or publish the lifecycle.
struct LentSseInspection {
    tracker: std::sync::Arc<std::sync::Mutex<UpstreamSseEvidenceTracker>>,
    active: bool,
}

impl LentSseInspection {
    fn lend(
        state: &mut SseRestorationState,
        tracker: &std::sync::Arc<std::sync::Mutex<UpstreamSseEvidenceTracker>>,
    ) -> Result<Self, RestoreError> {
        let mut slot = tracker.lock().map_err(|_| RestoreError::RestoreFailed)?;
        if slot.inspection.is_some() {
            return Err(RestoreError::RestoreFailed);
        }
        slot.inspection = Some(state.lend_original_inspection()?);
        #[cfg(test)]
        xai_lending_test_support::lent(tracker);
        Ok(Self {
            tracker: tracker.clone(),
            active: true,
        })
    }

    fn reclaim(mut self, state: &mut SseRestorationState) -> Result<(), RestoreError> {
        let inspection = self
            .tracker
            .lock()
            .map_err(|_| RestoreError::RestoreFailed)?
            .inspection
            .take()
            .ok_or(RestoreError::RestoreFailed)?;
        self.active = false;
        state.reclaim_original_inspection(inspection)
    }
}

impl Drop for LentSseInspection {
    fn drop(&mut self) {
        if self.active {
            // Poisoning cannot strand the owner after cancellation.
            let mut tracker = match self.tracker.lock() {
                Ok(tracker) => tracker,
                Err(poisoned) => poisoned.into_inner(),
            };
            tracker.inspection = None;
        }
    }
}

/// Observe original bytes before xAI's legacy framing and JSON repairs. The
/// observer forwards the same chunks and owns at most one charged SSE frame.
pub(crate) fn observe_xai_original_sse<S, E>(
    stream: S,
    evidence: Option<std::sync::Arc<std::sync::Mutex<UpstreamSseEvidenceTracker>>>,
) -> std::pin::Pin<Box<dyn Stream<Item = Result<Bytes, std::io::Error>> + Send>>
where
    S: Stream<Item = Result<Bytes, E>> + Send + 'static,
    E: std::error::Error + Send + 'static,
{
    Box::pin(async_stream::stream! {
        tokio::pin!(stream);
        let Some(evidence) = evidence else {
            while let Some(chunk) = stream.next().await {
                yield chunk.map_err(|_| std::io::Error::other("Response stream failed."));
            }
            return;
        };
        let budget = match evidence.lock() {
            Ok(tracker) => tracker.state.as_ref().map(completion_verifier::OriginalEvidence::budget),
            Err(_) => None,
        };
        let Some(budget) = budget else {
            yield Err(std::io::Error::other("Response stream failed."));
            return;
        };
        let mut frame = match PayloadBuffer::new(budget.clone(), 1024 * 1024) {
            Ok(frame) => frame,
            Err(_) => {
                if let Ok(mut tracker) = evidence.lock() { tracker.abort(); }
                yield Err(std::io::Error::other("Response stream failed."));
                return;
            }
        };
        let mut line_start = 0;
        while let Some(chunk) = stream.next().await {
            let chunk = match chunk {
                Ok(chunk) => chunk,
                Err(_) => {
                    if let Ok(mut tracker) = evidence.lock() { tracker.abort(); }
                    yield Err(std::io::Error::other("Response stream failed."));
                    return;
                }
            };
            let mut offset = 0;
            let checked = (|| -> Result<(), RestoreError> {
                while offset < chunk.len() {
                    let end = chunk[offset..].iter().position(|byte| *byte == b'\n')
                        .map_or(chunk.len(), |pos| offset + pos + 1);
                    frame.append(&chunk[offset..end]).map_err(RestoreError::scan_error)?;
                    offset = end;
                    if chunk[end - 1] != b'\n' { continue; }
                    let line = &frame.as_slice()[line_start..];
                    let blank = line == b"\n" || line == b"\r\n";
                    line_start = frame.len();
                    if !blank { continue; }
                    let raw = std::str::from_utf8(frame.as_slice()).map_err(|_| RestoreError::RestoreFailed)?;
                    evidence.lock().map_err(|_| RestoreError::RestoreFailed)?
                        .observe_xai_original_frame(raw)?;
                    #[cfg(test)]
                    xai_lending_test_support::observed();
                    frame.clear();
                    line_start = 0;
                }
                Ok(())
            })();
            if checked.is_err() {
                if let Ok(mut tracker) = evidence.lock() { tracker.abort(); }
                yield Err(std::io::Error::other("Response stream failed."));
                return;
            }
            yield Ok(chunk);
        }
        if let Ok(mut tracker) = evidence.lock() {
            tracker.observe_xai_original_end(
                frame.len() != 0,
                std::str::from_utf8(frame.as_slice()).is_err(),
            );
        };
        // Dropping/cancelling this stream never synthesizes an EOF or authority.
    })
}

pub(crate) fn rebuild_restored_json_http_response(
    status: StatusCode,
    headers: &HeaderMap,
    body_value: &Value,
) -> Response {
    let bytes = match serde_json::to_vec(body_value) {
        Ok(b) => b,
        Err(_) => {
            return build_codex_restore_error_response(RestoreError::RestoreFailed);
        }
    };
    let mut response_builder = Response::builder().status(status);
    for (key, value) in headers.iter() {
        if key != header::TRANSFER_ENCODING
            && key != header::CONTENT_LENGTH
            && key != header::CONTENT_ENCODING
        {
            response_builder = response_builder.header(key, value);
        }
    }
    response_builder = response_builder.header(header::CONTENT_LENGTH, bytes.len());
    match response_builder.body(Body::from(bytes)) {
        Ok(resp) => resp,
        Err(_) => build_codex_restore_error_response(RestoreError::RestoreFailed),
    }
}

pub(crate) fn wrap_sse_stream_with_restoration<S>(
    upstream_stream: S,
    restorer: SseRestorationState,
) -> std::pin::Pin<Box<dyn Stream<Item = Result<Bytes, std::io::Error>> + Send>>
where
    S: Stream<Item = Result<Bytes, std::io::Error>> + Send + 'static,
{
    wrap_sse_stream_with_restoration_and_evidence(upstream_stream, restorer, None)
}

pub(crate) fn wrap_sse_stream_with_restoration_and_evidence<S>(
    upstream_stream: S,
    mut restorer: SseRestorationState,
    evidence: Option<std::sync::Arc<std::sync::Mutex<UpstreamSseEvidenceTracker>>>,
) -> std::pin::Pin<Box<dyn Stream<Item = Result<Bytes, std::io::Error>> + Send>>
where
    S: Stream<Item = Result<Bytes, std::io::Error>> + Send + 'static,
{
    Box::pin(async_stream::stream! {
        tokio::pin!(upstream_stream);
        let mut frame = match PayloadBuffer::new(restorer.payload_budget(), 1024 * 1024) {
            Ok(buffer) => buffer,
            Err(_) => {
                yield Ok(responses_adapter::failed_sse_frame_at(RestoreError::RestoreFailed, RestoreStage::SseInit));
                return;
            }
        };
        let mut verifier = completion_verifier::SseCompletionVerifier::new(&restorer);
        let mut line_start = 0usize;
        loop {
            let lent = if restorer.needs_converted_inspection() {
                match evidence.as_ref().ok_or(RestoreError::RestoreFailed)
                    .and_then(|tracker| LentSseInspection::lend(&mut restorer, tracker)) {
                    Ok(lent) => Some(lent),
                    Err(error) => {
                        yield Ok(responses_adapter::failed_sse_frame_at(error, RestoreStage::CompletionEvidence));
                        return;
                    }
                }
            } else { None };
            let next = upstream_stream.next().await;
            if let Some(lent) = lent {
                if let Err(error) = lent.reclaim(&mut restorer) {
                    yield Ok(responses_adapter::failed_sse_frame_at(error, RestoreStage::CompletionEvidence));
                    return;
                }
            }
            let Some(chunk_res) = next else { break };
            let chunk = match chunk_res {
                Ok(chunk) => chunk,
                Err(_) => {
                    yield Ok(responses_adapter::failed_sse_frame_at(RestoreError::RestoreFailed, RestoreStage::SseRead));
                    return;
                }
            };
            let mut offset = 0usize;
            while offset < chunk.len() {
                let end = chunk[offset..].iter().position(|&byte| byte == b'\n')
                    .map_or(chunk.len(), |pos| offset + pos + 1);
                if frame.append(&chunk[offset..end]).is_err() {
                    yield Ok(responses_adapter::failed_sse_frame_at(RestoreError::RestoreFailed, RestoreStage::SseFrame));
                    return;
                }
                offset = end;
                if frame.as_slice().last() != Some(&b'\n') {
                    continue;
                }
                let line = &frame.as_slice()[line_start..];
                let blank = line == b"\n" || line == b"\r\n";
                line_start = frame.len();
                if !blank {
                    continue;
                }
                let block = match std::str::from_utf8(frame.as_slice()) {
                    Ok(block) => block,
                    Err(_) => {
                        yield Ok(responses_adapter::failed_sse_frame_at(RestoreError::RestoreFailed, RestoreStage::SseFrame));
                        return;
                    }
                };
                if let Err(error) = restorer.observe_native_input(block) {
                    yield Ok(responses_adapter::failed_sse_frame_at(error, RestoreStage::CompletionEvidence));
                    return;
                }
                if let Err(error) = verifier.observe_block(block) {
                    yield Ok(responses_adapter::failed_sse_frame_at(error, RestoreStage::CompletionEvidence));
                    return;
                }
                match restorer.process_sse_block(block) {
                    Ok(frames) => {
                        frame.clear();
                        line_start = 0;
                        for frame in frames {
                            yield Ok(frame);
                        }
                    }
                    Err(err) => {
                        let error = evidence.as_ref().and_then(|tracker| tracker.lock().ok()
                            .and_then(|guard| guard.has_unsupported_carrier().then_some(RestoreError::RestoreUnsupported)))
                            .unwrap_or(err);
                        yield Ok(responses_adapter::failed_sse_frame_at(error, restorer.failure_stage()));
                        return;
                    }
                }
            }
        }
        // EOF is not an SSE delimiter and cannot authorize an unfinished event.
        if frame.len() != 0 {
            yield Ok(responses_adapter::failed_sse_frame_at(RestoreError::RestoreFailed, RestoreStage::SseEof));
            return;
        }
        let checked_original = match evidence {
            Some(evidence) => evidence.lock().map_err(|_| RestoreError::RestoreFailed)
                .and_then(|mut tracker| tracker.take()).map(Some),
            None => Ok(None),
        };
        let original = match checked_original {
            Ok(original) => original,
            Err(error) => {
                yield Ok(responses_adapter::failed_sse_frame_at(error, RestoreStage::CompletionEvidence));
                return;
            }
        };
        let proof = match verifier.finish(&restorer, original) {
            Ok(proof) => proof,
            Err((error, stage)) => {
                yield Ok(responses_adapter::failed_sse_frame_at(error, stage));
                return;
            }
        };
        match restorer.finalize(proof) {
            Ok(batch) => {
                let mut delivery = batch.into_delivery_stream();
                while let Some(item) = delivery.next().await {
                    yield item;
                }
            }
            Err((error, stage)) => yield Ok(responses_adapter::failed_sse_frame_at(error, stage)),
        }
    })
}

#[cfg(test)]
#[path = "tests/json_restoration_tests.rs"]
mod json_restoration_tests;

#[cfg(test)]
#[path = "tests/final_validation_tests.rs"]
mod final_validation_tests;

#[cfg(test)]
pub(crate) mod transform_input_test_support {
    use std::{
        cell::RefCell,
        sync::{
            atomic::{AtomicBool, Ordering},
            Arc, Mutex,
        },
    };

    #[derive(Default)]
    pub(crate) struct Observation {
        pub prepared_pointer: usize,
        pub input_pointer: usize,
        pub prepared_key_pointer: usize,
        pub moved_key_pointer: usize,
        pub prompt_candidates: usize,
    }
    pub(crate) struct Probe {
        pub observation: Mutex<Observation>,
        pub reached: tokio::sync::Notify,
        pub resume: tokio::sync::Notify,
        paused: AtomicBool,
    }
    thread_local! {
        static CURRENT: RefCell<Option<Arc<Probe>>> = const { RefCell::new(None) };
    }
    pub(crate) struct Guard;
    impl Drop for Guard {
        fn drop(&mut self) {
            CURRENT.with(|slot| slot.replace(None));
        }
    }
    pub(crate) fn install() -> (Guard, Arc<Probe>) {
        let probe = Arc::new(Probe {
            observation: Mutex::new(Observation::default()),
            reached: tokio::sync::Notify::new(),
            resume: tokio::sync::Notify::new(),
            paused: AtomicBool::new(false),
        });
        CURRENT.with(|slot| slot.replace(Some(probe.clone())));
        (Guard, probe)
    }
    pub(crate) fn moved_prompt_key(key: Option<&str>) {
        CURRENT.with(|slot| {
            if let Some(probe) = slot.borrow().as_ref() {
                probe
                    .observation
                    .lock()
                    .expect("synthetic probe")
                    .moved_key_pointer = key.map_or(0, |key| key.as_ptr() as usize);
            }
        });
    }
    pub(super) fn prompt_candidate() {
        CURRENT.with(|slot| {
            if let Some(probe) = slot.borrow().as_ref() {
                probe
                    .observation
                    .lock()
                    .expect("synthetic probe")
                    .prompt_candidates += 1;
            }
        });
    }
    pub(super) fn prepared(value: &serde_json::Value) {
        CURRENT.with(|slot| {
            if let Some(probe) = slot.borrow().as_ref() {
                probe
                    .observation
                    .lock()
                    .expect("synthetic probe")
                    .prepared_key_pointer = value
                    .get("prompt_cache_key")
                    .and_then(serde_json::Value::as_str)
                    .map_or(0, |key| key.as_ptr() as usize);
            }
        });
        CURRENT.with(|slot| {
            if let Some(probe) = slot.borrow().as_ref() {
                probe
                    .observation
                    .lock()
                    .expect("synthetic transform probe")
                    .prepared_pointer = value
                    .get("padding")
                    .or_else(|| value.pointer("/metadata/padding"))
                    .and_then(serde_json::Value::as_str)
                    .map_or(0, |text| text.as_ptr() as usize);
            }
        });
    }
    pub(crate) fn input(value: &serde_json::Value) {
        CURRENT.with(|slot| {
            if let Some(probe) = slot.borrow().as_ref() {
                probe
                    .observation
                    .lock()
                    .expect("synthetic transform probe")
                    .input_pointer = value
                    .get("padding")
                    .or_else(|| value.pointer("/metadata/padding"))
                    .and_then(serde_json::Value::as_str)
                    .map_or(0, |text| text.as_ptr() as usize);
            }
        });
    }
    pub(crate) async fn pause() {
        let probe = CURRENT.with(|slot| slot.borrow().clone());
        if let Some(probe) = probe {
            if !probe.paused.swap(true, Ordering::SeqCst) {
                probe.reached.notify_one();
                probe.resume.notified().await;
            }
        }
    }
}

#[cfg(test)]
pub(super) mod source_copy_test_support {
    use std::{
        cell::RefCell,
        marker::PhantomData,
        rc::Rc,
        sync::{Arc, Mutex},
    };

    #[derive(Clone, Copy, Default, Debug)]
    pub(in super::super) struct Snapshot {
        pub attempts: usize,
        pub created: usize,
        pub released: usize,
        pub live_charge: usize,
    }

    type State = Arc<Mutex<Snapshot>>;
    thread_local! {
        static OBSERVATION: RefCell<Option<State>> = const { RefCell::new(None) };
    }

    pub(in super::super) struct Probe {
        state: State,
        previous: Option<State>,
        _thread_bound: PhantomData<Rc<()>>,
    }

    impl Probe {
        pub(in super::super) fn snapshot(&self) -> Snapshot {
            *self.state.lock().expect("synthetic source probe")
        }
    }

    impl Drop for Probe {
        fn drop(&mut self) {
            OBSERVATION.with(|state| state.replace(self.previous.take()));
        }
    }

    pub(in super::super) fn install() -> Probe {
        let state = Arc::new(Mutex::new(Snapshot::default()));
        Probe {
            previous: OBSERVATION.with(|slot| slot.replace(Some(state.clone()))),
            state,
            _thread_bound: PhantomData,
        }
    }

    pub(super) fn attempt() {
        OBSERVATION.with(|slot| {
            if let Some(state) = slot.borrow().as_ref() {
                state.lock().expect("synthetic source probe").attempts += 1;
            }
        });
    }

    // Observe the source owner itself, never total payload-budget consumption.
    // The captured state follows a Send owner even if a future changes threads.
    pub(super) struct OwnerProbe(Option<(State, usize)>);
    impl OwnerProbe {
        pub(super) fn new(bytes: usize) -> Self {
            Self(OBSERVATION.with(|slot| {
                slot.borrow().as_ref().map(|state| {
                    let mut snapshot = state.lock().expect("synthetic source probe");
                    snapshot.created += 1;
                    snapshot.live_charge += bytes;
                    (state.clone(), bytes)
                })
            }))
        }
    }
    impl Drop for OwnerProbe {
        fn drop(&mut self) {
            if let Some((state, bytes)) = self.0.take() {
                let mut snapshot = state.lock().expect("synthetic source probe");
                snapshot.released += 1;
                snapshot.live_charge -= bytes;
            }
        }
    }
}

#[cfg(test)]
pub(super) mod mapping_copy_test_support {
    use std::{
        cell::RefCell,
        marker::PhantomData,
        rc::Rc,
        sync::{Arc, Mutex},
    };

    #[derive(Clone, Copy, Default, Debug)]
    pub(in super::super) struct Snapshot {
        pub attempts: usize,
        pub created: usize,
        pub released: usize,
        pub live_charge: usize,
    }

    type State = Arc<Mutex<Snapshot>>;
    thread_local! {
        static OBSERVATION: RefCell<Option<State>> = const { RefCell::new(None) };
    }

    pub(in super::super) struct Probe {
        state: State,
        previous: Option<State>,
        _thread_bound: PhantomData<Rc<()>>,
    }

    impl Probe {
        pub(in super::super) fn snapshot(&self) -> Snapshot {
            *self.state.lock().expect("synthetic mapping probe")
        }
    }

    impl Drop for Probe {
        fn drop(&mut self) {
            OBSERVATION.with(|state| state.replace(self.previous.take()));
        }
    }

    pub(in super::super) fn install() -> Probe {
        let state = Arc::new(Mutex::new(Snapshot::default()));
        Probe {
            previous: OBSERVATION.with(|slot| slot.replace(Some(state.clone()))),
            state,
            _thread_bound: PhantomData,
        }
    }

    pub(super) fn attempt() {
        OBSERVATION.with(|slot| {
            if let Some(state) = slot.borrow().as_ref() {
                state.lock().expect("synthetic mapping probe").attempts += 1;
            }
        });
    }

    // Observe the mapping owner itself, never total payload-budget consumption.
    // The captured state follows a Send owner even if a future changes threads.
    pub(super) struct OwnerProbe(Option<(State, usize)>);
    impl OwnerProbe {
        pub(super) fn new(bytes: usize) -> Self {
            Self(OBSERVATION.with(|slot| {
                slot.borrow().as_ref().map(|state| {
                    let mut snapshot = state.lock().expect("synthetic mapping probe");
                    snapshot.created += 1;
                    snapshot.live_charge += bytes;
                    (state.clone(), bytes)
                })
            }))
        }
    }
    impl Drop for OwnerProbe {
        fn drop(&mut self) {
            if let Some((state, bytes)) = self.0.take() {
                let mut snapshot = state.lock().expect("synthetic mapping probe");
                snapshot.released += 1;
                snapshot.live_charge -= bytes;
            }
        }
    }
}

#[cfg(test)]
mod private_correctness_tests {
    use super::*;

    fn source_copy_request(budget: PayloadBudget) -> DlpHttpRequest {
        let store = DlpTokenStore::new();
        let request_body = json!({"input":"synthetic source copy", "nested":["quoted\nvalue"]});
        DlpHttpRequest {
            authority: RequestAuthority::from_test_lease(
                store.admit("codex", None, 1).expect("synthetic lease"),
                budget.clone(),
            ),
            runtime: SecurityRuntimeConfig::default(),
            restore_rules: StoreVec::new(store.allocation_ledger().expect("ledger"), 4096),
            source_body: PayloadJson::try_from_value(&request_body, budget.clone(), budget.limit())
                .expect("synthetic source"),
            pretokenized: false,
            native_protocol: None,
            endpoint_is_compact: false,
            audit_state: std::sync::Arc::new(
                RequestAuditState::new(&AppType::Codex, None, budget.clone())
                    .expect("synthetic audit"),
            ),
            initial_audit: std::sync::Arc::new(
                PendingRequestAudit::empty(budget.clone()).expect("synthetic summary"),
            ),
        }
    }

    #[test]
    fn guarded_response_namespace_captured_participation_truth_table() {
        use super::super::tokenization::{TokenCategory, TokenFragment};
        for enabled in [false, true] {
            for policy in [false, true] {
                for rules in [false, true] {
                    for leased in [false, true] {
                        let budget = PayloadBudget::with_limit(4 * 1024 * 1024);
                        let mut request = source_copy_request(budget.clone());
                        request.runtime.enabled = enabled;
                        if policy {
                            request.runtime.builtin_action_overrides.insert(
                                "chinese_phone_number".into(),
                                super::super::AuditAction::RedactRestore,
                            );
                        }
                        if rules {
                            let store = DlpTokenStore::new();
                            request
                                .restore_rules
                                .push(
                                    StoreText::try_from_str(
                                        "chinese_phone_number",
                                        store.allocation_ledger().expect("synthetic ledger"),
                                        256,
                                    )
                                    .expect("synthetic rule"),
                                )
                                .expect("synthetic rules");
                        }
                        if leased {
                            request
                                .authority
                                .lease_mut_for_responses()
                                .prepare(
                                    &[TokenFragment {
                                        raw: "synthetic-secret",
                                        whole_span_categories: &[TokenCategory::Custom],
                                    }],
                                    &[],
                                    |_| Ok(()),
                                )
                                .expect("synthetic token");
                        }
                        let body = json!({"tools":[{"type":"namespace","name":"ns","tools":[{"type":"function","name":"f"}]}]});
                        let expected = (enabled && (policy || rules)) || leased;
                        let before = budget.used();
                        let table = request
                            .build_response_namespace_table(&body)
                            .expect("captured table");
                        assert_eq!(
                            table.is_some(),
                            expected,
                            "enabled={enabled} policy={policy} rules={rules} lease={leased}"
                        );
                        assert_eq!(budget.used() > before, expected);
                        drop(table);
                        assert_eq!(budget.used(), before);
                        drop(request);
                        assert_eq!(budget.used(), 0);
                    }
                }
            }
        }
    }

    #[test]
    fn guarded_namespace_private_owner_refusal_and_callback_failures_release() {
        use crate::proxy::error::ProxyError;
        let budget = PayloadBudget::with_limit(4 * 1024 * 1024);
        let request = source_copy_request(budget.clone());
        let original = json!({"input":"synthetic safe input","tools":[{"type":"namespace","name":"n","tools":[{"type":"function","name":"f","description":"p".repeat(4096)}]}]});
        let baseline = budget.used();
        let first_owner;
        let (first, owner) = request
            .build_namespace_body(&original)
            .expect("first")
            .expect("candidate");
        first_owner = owner;
        let charge = budget.used() - baseline;
        let second_owner;
        let (second, owner) = request
            .build_namespace_body(&original)
            .expect("second")
            .expect("candidate");
        second_owner = owner;
        assert_eq!(budget.used(), baseline + 2 * charge);
        assert_ne!(
            first["tools"][0]["description"]
                .as_str()
                .expect("text")
                .as_ptr(),
            second["tools"][0]["description"]
                .as_str()
                .expect("text")
                .as_ptr()
        );
        drop(first);
        drop(first_owner);
        assert_eq!(budget.used(), baseline + charge);
        drop(second);
        drop(second_owner);
        assert_eq!(budget.used(), baseline);
        let filler = budget
            .reserve(budget.limit() - baseline - charge + 1)
            .expect("fill");
        assert!(matches!(
            request.build_namespace_body(&original),
            Err(DlpForwardError::CapacityExceeded)
        ));
        drop(filler);
        assert_eq!(budget.used(), baseline);
        for final_validation in [false, true] {
            let mut body = original.clone();
            let mut retained = None;
            let result = request.prepare_owned_attempt_for_proxy(
                "synthetic-provider",
                "openai_responses",
                "http://127.0.0.1:1/v1/responses",
                Some("synthetic-account"),
                &mut body,
                |input| {
                    let (mut output, owner) = request
                        .build_namespace_body(&input)
                        .map_err(ProxyError::DlpForward)?
                        .ok_or(ProxyError::DlpForward(DlpForwardError::Unavailable))?;
                    retained = Some(owner);
                    if final_validation {
                        output["input"] = json!("13800138000");
                        Ok(output)
                    } else {
                        Err(ProxyError::DlpForward(DlpForwardError::Unavailable))
                    }
                },
            );
            assert!(if final_validation {
                matches!(
                    result,
                    Err(ProxyError::DlpForward(DlpForwardError::RedactionFailed))
                )
            } else {
                matches!(
                    result,
                    Err(ProxyError::DlpForward(DlpForwardError::Unavailable))
                )
            });
            assert!(retained.is_some());
            assert!(budget.used() >= baseline + charge);
            // Refusal auditing can retain its dedup backing until request drop.
            // Measure this owner independently rather than attributing audit bytes to it.
            let before_release = budget.used();
            drop(retained);
            assert_eq!(budget.used(), before_release - charge);
        }
        drop(request);
        assert_eq!(budget.used(), 0);
    }

    #[test]
    fn guarded_prompt_cache_independent_owner_and_failures_release() {
        use crate::proxy::error::ProxyError;
        let budget = PayloadBudget::with_limit(4 * 1024 * 1024);
        let request = source_copy_request(budget.clone());
        let original = json!({"messages":[{"role":"user","content":"synthetic safe input"}]});
        let baseline = budget.used();
        let first_owner;
        let (first, owner) = request
            .build_prompt_cache_body(&original, "synthetic-key")
            .expect("first");
        first_owner = owner;
        let charge = budget.used() - baseline;
        let second_owner;
        let (second, owner) = request
            .build_prompt_cache_body(&original, "synthetic-key")
            .expect("second");
        second_owner = owner;
        assert_eq!(budget.used(), baseline + 2 * charge);
        assert_ne!(
            first["prompt_cache_key"].as_str().expect("first").as_ptr(),
            second["prompt_cache_key"]
                .as_str()
                .expect("second")
                .as_ptr()
        );
        drop(first);
        drop(first_owner);
        assert_eq!(budget.used(), baseline + charge);
        drop(second);
        drop(second_owner);
        assert_eq!(budget.used(), baseline);
        let filler = budget
            .reserve(budget.limit() - baseline - charge + 1)
            .expect("fill");
        assert!(matches!(
            request.build_prompt_cache_body(&original, "synthetic-key"),
            Err(DlpForwardError::CapacityExceeded)
        ));
        drop(filler);
        assert_eq!(budget.used(), baseline);
        for final_validation in [false, true] {
            let candidate_owner;
            let mut body = json!({"input":"synthetic safe input"});
            let mut retained = None;
            let result = request.prepare_owned_attempt_for_proxy(
                "synthetic-provider",
                "openai_chat",
                "http://127.0.0.1:1/v1/chat/completions",
                Some("synthetic-account"),
                &mut body,
                |input| {
                    let input = json!({"messages":[{"role":"user","content":input["input"]}]});
                    let (mut output, owner) = request
                        .build_prompt_cache_body(&input, "synthetic-key")
                        .map_err(ProxyError::DlpForward)?;
                    retained = Some(owner);
                    if final_validation {
                        output["messages"][0]["content"] = json!("13800138000");
                        Ok(output)
                    } else {
                        Err(ProxyError::DlpForward(DlpForwardError::Unavailable))
                    }
                },
            );
            candidate_owner = retained;
            assert!(if final_validation {
                matches!(
                    result,
                    Err(ProxyError::DlpForward(DlpForwardError::RedactionFailed))
                )
            } else {
                matches!(
                    result,
                    Err(ProxyError::DlpForward(DlpForwardError::Unavailable))
                )
            });
            drop(result);
            assert!(
                budget.used() > baseline,
                "retained candidate owner must remain charged through callback failure"
            );
            drop(candidate_owner);
            // Refusal audit deduplication can remain until request drop.
            assert!(budget.used() <= baseline + 256);
        }
        drop(request);
        assert_eq!(budget.used(), 0);
    }

    #[test]
    fn guarded_transform_input_owned_attempts_keep_independent_reservations() {
        let provenance_charge = std::mem::size_of::<super::super::payload_budget::Reservation>()
            + 2 * std::mem::size_of::<usize>();
        let budget = PayloadBudget::with_limit(4 * 1024 * 1024);
        let request = source_copy_request(budget.clone());
        let original = json!({"input":"synthetic safe input", "padding":"p".repeat(4096)});
        let baseline = budget.used();
        let expected = PayloadJson::try_from_value(&original, budget.clone(), budget.limit())
            .expect("synthetic measure");
        let charge = budget.used() - baseline;
        drop(expected);
        let (_probe_guard, probe) = transform_input_test_support::install();
        let first_owner;
        let mut first_body = original.clone();
        let (first_attempt, owner) = request
            .prepare_owned_attempt_for_proxy(
                "first-provider",
                "openai_responses",
                "http://127.0.0.1:1/v1/responses",
                Some("synthetic-account"),
                &mut first_body,
                |body| {
                    transform_input_test_support::input(&body);
                    let observed = probe.observation.lock().expect("synthetic probe");
                    assert_eq!(observed.prepared_pointer, observed.input_pointer);
                    assert_ne!(observed.prepared_pointer, 0);
                    Ok(body)
                },
            )
            .expect("first owned attempt");
        first_owner = owner;
        assert_eq!(first_body, original);
        assert_eq!(budget.used(), baseline + charge + provenance_charge);
        let second_owner;
        let mut second_body = original.clone();
        let (second_attempt, owner) = request
            .prepare_owned_attempt_for_proxy(
                "second-provider",
                "openai_responses",
                "http://127.0.0.1:2/v1/responses",
                Some("synthetic-account"),
                &mut second_body,
                Ok,
            )
            .expect("second owned attempt");
        second_owner = owner;
        assert_eq!(budget.used(), baseline + 2 * (charge + provenance_charge));
        assert_ne!(
            first_body["padding"].as_str().expect("padding").as_ptr(),
            second_body["padding"].as_str().expect("padding").as_ptr()
        );
        drop(first_attempt);
        drop(second_attempt);
        assert_eq!(
            budget.used(),
            baseline + 2 * charge,
            "attempt authority does not own input allocation"
        );
        drop(first_body);
        drop(first_owner);
        assert_eq!(budget.used(), baseline + charge);
        drop(second_body);
        drop(second_owner);
        assert_eq!(budget.used(), baseline);
        drop(request);
        assert_eq!(budget.used(), 0);
    }

    #[test]
    fn guarded_transform_input_callback_and_final_validation_failures_release_charge() {
        use crate::proxy::error::ProxyError;
        let budget = PayloadBudget::with_limit(4 * 1024 * 1024);
        let request = source_copy_request(budget.clone());
        let original = json!({"input":"synthetic safe input", "padding":"p".repeat(4096)});
        let baseline = budget.used();
        for dlp_failure in [false, true] {
            let mut body = original.clone();
            let result = request.prepare_owned_attempt_for_proxy(
                "synthetic-provider",
                "openai_responses",
                "http://127.0.0.1:1/v1/responses",
                Some("synthetic-account"),
                &mut body,
                |input| {
                    assert!(budget.used() > baseline);
                    drop(input);
                    if dlp_failure {
                        Err(ProxyError::DlpForward(DlpForwardError::CapacityExceeded))
                    } else {
                        Err(ProxyError::InvalidRequest(
                            "synthetic converter sentinel".into(),
                        ))
                    }
                },
            );
            assert!(if dlp_failure {
                matches!(
                    result,
                    Err(ProxyError::DlpForward(DlpForwardError::CapacityExceeded))
                )
            } else {
                matches!(result, Err(ProxyError::InvalidRequest(ref message)) if message == "synthetic converter sentinel")
            });
            assert_eq!(body, original);
            assert_eq!(budget.used(), baseline);
        }
        let mut body = original.clone();
        let result = request.prepare_owned_attempt_for_proxy(
            "synthetic-provider",
            "openai_responses",
            "http://127.0.0.1:1/v1/responses",
            Some("synthetic-account"),
            &mut body,
            |mut input| {
                input["input"] = json!("13800138000");
                Ok(input)
            },
        );
        assert!(matches!(
            result,
            Err(ProxyError::DlpForward(DlpForwardError::RedactionFailed))
        ));
        assert_eq!(body, original);
        let events = request.audit_state.recorded_for_test();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].log.action_taken, "block");
        // The live request retains the four-slot refusal-audit dedup table,
        // not a prepared input allocation. Request destruction releases it.
        let audit_charge =
            4 * 2 * std::mem::size_of::<super::super::identity::ContentFingerprint>();
        assert_eq!(budget.used(), baseline + audit_charge);
        drop(body);
        drop(request);
        assert_eq!(budget.used(), 0);
    }

    #[test]
    fn guarded_outbound_tail_owners_are_independent_and_noop_still_rebuilds() {
        let budget = PayloadBudget::with_limit(1024 * 1024);
        let request = source_copy_request(budget.clone());
        let original = json!({"input":"synthetic safe input","padding":"p".repeat(4096)});
        let baseline = budget.used();
        let measure = PayloadJson::try_from_value(&original, budget.clone(), budget.limit())
            .expect("measure");
        let charge = budget.used() - baseline;
        drop(measure);
        let first_owner;
        let (first, owner) = request
            .build_outbound_tail(&original, None)
            .expect("first tail");
        first_owner = owner;
        let second_owner;
        let (second, owner) = request
            .build_outbound_tail(&original, Some(&json!({})))
            .expect("noop tail");
        second_owner = owner;
        assert_eq!(first, original);
        assert_eq!(second, original);
        assert_ne!(
            first["padding"].as_str().expect("padding").as_ptr(),
            original["padding"].as_str().expect("padding").as_ptr()
        );
        assert_ne!(
            first["padding"].as_str().expect("padding").as_ptr(),
            second["padding"].as_str().expect("padding").as_ptr()
        );
        assert_eq!(budget.used(), baseline + 2 * charge);
        drop(first);
        drop(first_owner);
        assert_eq!(budget.used(), baseline + charge);
        drop(second);
        drop(second_owner);
        assert_eq!(budget.used(), baseline);
        drop(request);
        assert_eq!(budget.used(), 0);
    }

    #[test]
    fn guarded_outbound_tail_callback_and_final_validation_failures_release() {
        use crate::proxy::error::ProxyError;
        let budget = PayloadBudget::with_limit(4 * 1024 * 1024);
        let request = source_copy_request(budget.clone());
        let original = json!({"input":"synthetic safe input","padding":"p".repeat(4096)});
        let baseline = budget.used();
        for capacity_failure in [false, true] {
            let mut tail_owner = None;
            let mut body = original.clone();
            let result = request.prepare_owned_attempt_for_proxy(
                "synthetic-provider",
                "openai_responses",
                "http://127.0.0.1:1/v1/responses",
                Some("synthetic-account"),
                &mut body,
                |input| {
                    let (output, owner) = request
                        .build_outbound_tail(&input, Some(&json!({"tail_extra":"q".repeat(4096)})))
                        .map_err(ProxyError::DlpForward)?;
                    tail_owner = Some(owner);
                    drop(output);
                    Err(if capacity_failure {
                        ProxyError::DlpForward(DlpForwardError::CapacityExceeded)
                    } else {
                        ProxyError::InvalidRequest("synthetic tail sentinel".into())
                    })
                },
            );
            assert!(if capacity_failure {
                matches!(
                    result,
                    Err(ProxyError::DlpForward(DlpForwardError::CapacityExceeded))
                )
            } else {
                matches!(result, Err(ProxyError::InvalidRequest(ref message)) if message == "synthetic tail sentinel")
            });
            assert_eq!(body, original);
            assert!(
                budget.used() > baseline,
                "caller owns tail through callback failure"
            );
            drop(body);
            drop(tail_owner);
            assert_eq!(budget.used(), baseline);
        }
        let mut tail_owner = None;
        let mut body = original.clone();
        let result = request.prepare_owned_attempt_for_proxy(
            "synthetic-provider",
            "openai_responses",
            "http://127.0.0.1:1/v1/responses",
            Some("synthetic-account"),
            &mut body,
            |input| {
                let (output, owner) = request
                    .build_outbound_tail(&input, Some(&json!({"input":"13800138000"})))
                    .map_err(ProxyError::DlpForward)?;
                tail_owner = Some(owner);
                Ok(output)
            },
        );
        assert!(matches!(
            result,
            Err(ProxyError::DlpForward(DlpForwardError::RedactionFailed))
        ));
        assert_eq!(body, original);
        assert!(budget.used() > baseline);
        drop(body);
        drop(tail_owner);
        let events = request.audit_state.recorded_for_test();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].log.action_taken, "block");
        let audit_charge =
            4 * 2 * std::mem::size_of::<super::super::identity::ContentFingerprint>();
        assert_eq!(budget.used(), baseline + audit_charge);
        drop(request);
        assert_eq!(budget.used(), 0);
    }

    fn mapping_provider() -> crate::provider::Provider {
        crate::provider::Provider::with_id(
            "synthetic-mapping".into(),
            "Synthetic mapping".into(),
            json!({"env":{"ANTHROPIC_MODEL":format!("{} [1M]", "m".repeat(4096))}}),
            None,
        )
    }

    #[test]
    fn m3_desktop_mapping_capacity_failure_and_drop_refund_every_charge(
    ) -> Result<(), Box<dyn std::error::Error>> {
        use super::super::payload_json::test_support::{self, Action, Phase};
        for normalize in [false, true] {
            let budget = PayloadBudget::with_limit(1024 * 1024);
            let request = source_copy_request(budget.clone());
            let body = json!({"model":"claude-sonnet-fixture","messages":[
                {"role":"assistant","content":[{"type":"tool_use","id":"synthetic-call","name":"synthetic_lookup","input":{}}]}]});
            let original = body.to_string();
            let before = budget.used();
            let owner;
            let mapped = {
                let (mapped, reservation) = request
                    .build_desktop_mapped_body(&body, "synthetic-mimo-model", normalize)
                    .map_err(|_| "m3_desktop_mapping_fixture_failed")?;
                owner = reservation;
                mapped
            };
            let charge = budget.used() - before;
            assert!(
                charge > 0 && mapped["model"] == "synthetic-mimo-model",
                "m3_desktop_mapping_not_charged"
            );
            drop(mapped);
            assert!(
                budget.used() == before + charge,
                "m3_desktop_mapping_charge_released_before_owner"
            );
            drop(owner);
            assert!(
                budget.used() == before,
                "m3_desktop_mapping_drop_not_refunded"
            );
            let fill = budget.reserve(budget.limit() - before - charge + 1)?;
            assert!(
                matches!(
                    request.build_desktop_mapped_body(&body, "synthetic-mimo-model", normalize),
                    Err(DlpForwardError::CapacityExceeded)
                ),
                "m3_desktop_mapping_short_capacity_not_rejected"
            );
            assert!(
                budget.used() == before + fill.bytes(),
                "m3_desktop_mapping_capacity_failure_leaked_charge"
            );
            drop(fill);
            let fill = budget.reserve(budget.limit() - before - charge)?;
            let owner;
            let mapped = {
                let (value, reservation) = request
                    .build_desktop_mapped_body(&body, "synthetic-mimo-model", normalize)
                    .map_err(|_| "m3_desktop_mapping_fixture_failed")?;
                owner = reservation;
                value
            };
            assert!(
                budget.used() == budget.limit(),
                "m3_desktop_mapping_exact_capacity_failed"
            );
            drop(mapped);
            drop(owner);
            drop(fill);
            let hook = test_support::install(|event| {
                if event.phase == Phase::AfterBuild {
                    Action::Fail
                } else {
                    Action::Continue
                }
            });
            assert!(
                matches!(
                    request.build_desktop_mapped_body(&body, "synthetic-mimo-model", normalize),
                    Err(DlpForwardError::Unavailable)
                ),
                "m3_desktop_mapping_build_failure_not_propagated"
            );
            drop(hook);
            // Compare the serialized source, not `Value` equality: map order is preserved
            // on the wire, so only the encoded form proves the source stayed untouched.
            let restored_source = body.to_string();
            assert!(
                budget.used() == before && restored_source == original,
                "m3_desktop_mapping_failure_mutated_source_or_leaked"
            );
            drop(request);
            assert!(budget.used() == 0, "m3_desktop_mapping_request_drop_leaked");
        }
        Ok(())
    }

    #[test]
    fn guarded_model_mapping_precharges_final_tree_and_preserves_source() {
        use super::super::payload_json::test_support::{self, Action, Phase};
        let budget = PayloadBudget::with_limit(1024 * 1024);
        let request = source_copy_request(budget.clone());
        let body = json!({"input":"synthetic input", "model":"original", "nested":[{"model":"untouched"}]});
        let original = body.to_string();
        let provider = mapping_provider();
        let before = budget.used();
        let mut expected = body.clone();
        expected["model"] = json!("m".repeat(4096));
        let expected_json = PayloadJson::try_from_value(&expected, budget.clone(), budget.limit())
            .expect("expected copy");
        let charge = budget.used() - before;
        drop(expected_json);
        let hook = test_support::install(move |event| {
            if event.phase == Phase::BeforeBuild {
                assert_eq!(event.charged, charge);
                assert_eq!(event.used, before + charge);
            }
            Action::Continue
        });
        let owner;
        let mapped = {
            let (value, reservation) = request
                .build_mapped_body(&body, &provider, true)
                .expect("mapped");
            owner = reservation;
            value
        };
        assert_eq!(mapped.to_string(), expected.to_string());
        assert_eq!(body.to_string(), original);
        assert_eq!(budget.used(), before + charge);
        drop(hook);
        drop(mapped);
        assert_eq!(budget.used(), before + charge);
        drop(owner);
        assert_eq!(budget.used(), before);
        drop(request);
        assert_eq!(budget.used(), 0);
    }

    #[test]
    fn guarded_model_mapping_exact_capacity_and_short_by_one() {
        use super::super::payload_json::test_support::{self, Action, Phase};
        let budget = PayloadBudget::with_limit(1024 * 1024);
        let request = source_copy_request(budget.clone());
        let body = json!({"input":"synthetic input", "model":"original"});
        let provider = mapping_provider();
        let before = budget.used();
        let (mapped, owner) = request
            .build_mapped_body(&body, &provider, false)
            .expect("measure");
        let charge = budget.used() - before;
        assert!(mapped["model"]
            .as_str()
            .is_some_and(|model| model.ends_with("[1M]")));
        drop(mapped);
        drop(owner);
        let fill = budget
            .reserve(budget.limit() - before - charge + 1)
            .expect("fill");
        let hook = test_support::install(|_| panic!("capacity refusal must precede construction"));
        assert!(matches!(
            request.build_mapped_body(&body, &provider, false),
            Err(DlpForwardError::CapacityExceeded)
        ));
        drop(hook);
        drop(fill);
        let fill = budget
            .reserve(budget.limit() - before - charge)
            .expect("exact fill");
        let (mapped, owner) = request
            .build_mapped_body(&body, &provider, false)
            .expect("exact capacity");
        assert_eq!(budget.used(), budget.limit());
        drop(mapped);
        drop(owner);
        assert_eq!(budget.used(), before + fill.bytes());
        drop(fill);
        let hook = test_support::install(|event| {
            if event.phase == Phase::AfterBuild {
                Action::Fail
            } else {
                Action::Continue
            }
        });
        assert!(matches!(
            request.build_mapped_body(&body, &provider, false),
            Err(DlpForwardError::Unavailable)
        ));
        assert_eq!(budget.used(), before);
        assert_eq!(body["model"], "original");
        drop(hook);
        drop(request);
        assert_eq!(budget.used(), 0);
    }

    #[test]
    fn guarded_model_mapping_missing_nonstring_and_no_mapping_preserve_wire() {
        let budget = PayloadBudget::with_limit(1024 * 1024);
        let request = source_copy_request(budget.clone());
        let mut provider = mapping_provider();
        for config in [json!({}), provider.settings_config.clone()] {
            provider.settings_config = config;
            for body in [
                json!({"input":"x"}),
                json!({"model":17,"input":"x"}),
                json!({"model":null,"input":"x"}),
            ] {
                let before = budget.used();
                let (mapped, owner) = request
                    .build_mapped_body(&body, &provider, true)
                    .expect("mapping copy");
                assert_eq!(mapped.to_string(), body.to_string());
                assert!(budget.used() > before);
                drop(mapped);
                drop(owner);
                assert_eq!(budget.used(), before);
            }
        }
        provider.settings_config = json!({});
        let body = json!({"model":"synthetic [1m]  ","input":"x"});
        let (mapped, owner) = request
            .build_mapped_body(&body, &provider, true)
            .expect("suffix");
        assert_eq!(mapped["model"], "synthetic");
        drop(mapped);
        drop(owner);
        drop(request);
        assert_eq!(budget.used(), 0);
    }

    #[test]
    fn private_source_copy_has_distinct_prebuild_charge() {
        use super::super::payload_json::test_support::{self, Action, Phase};
        use std::{cell::Cell, rc::Rc};
        let budget = PayloadBudget::with_limit(1024 * 1024);
        let request = source_copy_request(budget.clone());
        let before = budget.used();
        let builds = Rc::new(Cell::new(0));
        let observed = builds.clone();
        let hook = test_support::install(move |event| {
            if event.phase == Phase::BeforeBuild {
                assert!(event.charged > 0);
                assert_eq!(event.used, before + event.charged);
                observed.set(observed.get() + 1);
            }
            Action::Continue
        });
        let owner;
        let copy = {
            let (copy, charge) = request.clone_source_body().expect("source copy");
            owner = charge;
            copy
        };
        assert_eq!(copy, *request.source_body.as_value());
        assert_eq!(
            builds.get(),
            1,
            "each request-source copy must reserve independently before building"
        );
        assert!(
            budget.used() > before,
            "a live independent source copy must retain its charge"
        );
        drop(hook);
        drop(copy);
        assert!(
            budget.used() > before,
            "owner must hold charge until explicitly released"
        );
        drop(owner);
        assert_eq!(budget.used(), before);
        drop(request);
        assert_eq!(budget.used(), 0);
    }

    #[test]
    fn private_source_copy_capacity_denies_before_build() {
        use super::super::payload_json::test_support::{self, Action};
        use std::{cell::Cell, rc::Rc};
        let budget = PayloadBudget::with_limit(1024 * 1024);
        let request = source_copy_request(budget.clone());
        let before = budget.used();
        let fill = budget
            .reserve(budget.limit() - before)
            .expect("fill synthetic budget");
        let observations = Rc::new(Cell::new(0));
        let observed = observations.clone();
        let hook = test_support::install(move |_| {
            observed.set(observed.get() + 1);
            Action::Continue
        });
        assert!(matches!(
            request.clone_source_body(),
            Err(DlpForwardError::CapacityExceeded)
        ));
        assert_eq!(
            observations.get(),
            0,
            "capacity denial must precede JSON allocation"
        );
        assert_eq!(budget.used(), budget.limit());
        assert_eq!(
            request.source_body.as_value()["input"],
            "synthetic source copy"
        );
        drop(hook);
        drop(fill);
        assert_eq!(budget.used(), before);
        drop(request);
        assert_eq!(budget.used(), 0);
    }

    #[test]
    fn private_source_copy_build_failure_rolls_back_and_preserves_source() {
        use super::super::payload_json::test_support::{self, Action, Phase};
        let budget = PayloadBudget::with_limit(1024 * 1024);
        let request = source_copy_request(budget.clone());
        let before = budget.used();
        let hook = test_support::install(|event| {
            if event.phase == Phase::AfterBuild {
                Action::Fail
            } else {
                Action::Continue
            }
        });
        assert!(matches!(
            request.clone_source_body(),
            Err(DlpForwardError::Unavailable)
        ));
        assert_eq!(budget.used(), before);
        assert_eq!(
            request.source_body.as_value()["input"],
            "synthetic source copy"
        );
        drop(hook);
        drop(request);
        assert_eq!(budget.used(), 0);
    }

    fn prepared_attempt(protocol: &str, session: Option<&str>) -> DlpPreparedAttempt {
        let store = DlpTokenStore::new();
        let budget = PayloadBudget::with_limit(4 * 1024 * 1024);
        let request_body = json!({"input": "hello"});
        let request = DlpHttpRequest {
            authority: RequestAuthority::from_test_lease(
                store.admit("codex", session, 1).expect("admit fixture"),
                budget.clone(),
            ),
            runtime: SecurityRuntimeConfig::default(),
            restore_rules: StoreVec::new(store.allocation_ledger().expect("ledger"), 4096),
            source_body: PayloadJson::try_from_value(
                &request_body,
                budget.clone(),
                4 * 1024 * 1024,
            )
            .expect("source fixture"),
            pretokenized: false,
            native_protocol: None,
            endpoint_is_compact: false,
            audit_state: std::sync::Arc::new(
                RequestAuditState::new(&AppType::Codex, session, budget.clone())
                    .expect("audit state fixture"),
            ),
            initial_audit: std::sync::Arc::new(
                PendingRequestAudit::empty(budget).expect("audit summary fixture"),
            ),
        };
        request
            .prepare_attempt(
                "fixture-provider",
                protocol,
                "http://127.0.0.1:1/v1/responses",
                Some("fixture-account"),
                &mut request_body.clone(),
                |body| Ok(body.clone()),
            )
            .expect("prepare fixture attempt")
    }

    #[test]
    fn private_outbound_empty_lease_skips_owned_encoding() {
        use super::super::payload_buffer::test_support::{self, Action};
        use std::{cell::Cell, rc::Rc};
        let attempt = prepared_attempt("openai_responses", None);
        assert!(!attempt.has_restoration_lease());
        let before = attempt.budget.used();
        let observations = Rc::new(Cell::new(0));
        let observed = observations.clone();
        let _hook = test_support::install(move |_| {
            observed.set(observed.get() + 1);
            Action::Continue
        });
        assert!(attempt
            .encode_outbound_body(&json!({"input":"ordinary fixture"}))
            .expect("empty lease")
            .is_none());
        assert_eq!(
            observations.get(),
            0,
            "empty leases must not allocate or copy an owned wire body"
        );
        assert_eq!(attempt.budget.used(), before);
    }

    fn completed_response() -> Value {
        json!({"id":"resp_fixture", "status":"completed", "output":[]})
    }

    #[test]
    fn private_json_converted_completion_requires_original_evidence() {
        let attempt = prepared_attempt("openai_chat", Some("fixture-session"));
        assert!(attempt
            .restore_json_response(
                200,
                &completed_response(),
                ArgumentNormalizationMode::Exact,
                None
            )
            .is_err());
    }

    #[test]
    fn private_json_original_protocol_and_failures_are_bound_to_attempt() {
        let attempt = prepared_attempt("openai_chat", Some("fixture-session"));
        assert!(attempt
            .verify_upstream_json_completion(
                JsonCompletionProtocol::NativeResponses,
                &completed_response().to_string()
            )
            .is_err());
        assert!(attempt
            .restore_json_response(
                200,
                &completed_response(),
                ArgumentNormalizationMode::Exact,
                None
            )
            .is_err());
    }

    #[test]
    fn private_json_invalid_observation_cannot_be_ignored() {
        let attempt = prepared_attempt("openai_responses", Some("fixture-session"));
        assert!(attempt
            .restore_json_response(
                200,
                &completed_response(),
                ArgumentNormalizationMode::Exact,
                Some(&CompletionObservation::InvalidJson)
            )
            .is_err());
    }

    #[test]
    fn private_json_failed_original_cannot_be_replaced_by_completed_conversion() {
        let attempt = prepared_attempt("openai_chat", Some("fixture-session"));
        let invalid = r#"{"id":"chat_fixture","choices":[{"finish_reason":"length","message":{"content":"partial"}}]}"#;
        assert!(attempt
            .verify_upstream_json_completion(JsonCompletionProtocol::ChatCompletions, invalid)
            .is_err());
        assert!(attempt
            .restore_json_response(
                200,
                &completed_response(),
                ArgumentNormalizationMode::Exact,
                None
            )
            .is_err());
    }

    #[test]
    fn private_json_verified_original_allows_complete_conversion() {
        let attempt = prepared_attempt("openai_chat", Some("fixture-session"));
        let complete = r#"{"id":"chat_fixture","choices":[{"finish_reason":"stop","message":{"content":"hello"}}]}"#;
        assert!(attempt
            .verify_upstream_json_completion(JsonCompletionProtocol::ChatCompletions, complete)
            .is_ok());
        assert!(attempt
            .restore_json_response(
                200,
                &completed_response(),
                ArgumentNormalizationMode::Exact,
                None
            )
            .is_ok());
    }
    fn c3_json_attempt(
        fixture: &super::super::unified_test_support::ResponsesFixture,
        mapped: bool,
    ) -> super::super::unified_test_support::TestResult<(DlpPreparedAttempt, Value)> {
        use super::super::unified_test_support::{CONTACT, PRINCIPAL, PROVIDER, SESSION};
        let mut body = json!({"input": if mapped { CONTACT } else { "Synthetic ordinary input" }});
        let request = fixture.admit(&mut body, false, Some(SESSION))?;
        let attempt = request
            .prepare_attempt(
                PROVIDER,
                "openai_chat",
                "https://synthetic.invalid/v1/chat/completions",
                Some(PRINCIPAL),
                &mut body,
                |candidate| Ok(candidate.clone()),
            )
            .map_err(|_| "c3_json_preparation_failed")?;
        assert_eq!(attempt.has_active_tokens(), mapped);
        let arguments = json!({"value":body["input"]}).to_string();
        let original = json!({"id":"synthetic-c3-chat", "choices":[{
            "finish_reason":"tool_calls", "message":{"role":"assistant", "tool_calls":[{
                "id":"synthetic-c3-call", "type":"function", "function":{
                    "name":"synthetic_nonexecuting_tool", "arguments":arguments
                }
            }]}
        }]});
        attempt
            .verify_upstream_json_completion(
                JsonCompletionProtocol::ChatCompletions,
                &original.to_string(),
            )
            .map_err(|_| "c3_json_original_observation_failed")?;
        let response = json!({"id":"synthetic-c3-response", "status":"completed", "output":[{
            "id":"synthetic-c3-item", "type":"function_call", "status":"completed",
            "call_id":"synthetic-c3-call", "name":"synthetic_nonexecuting_tool", "arguments":arguments
        }]});
        Ok((attempt, response))
    }

    async fn c3_json_invalid_final_client(
        mapped: bool,
        field: &str,
    ) -> super::super::unified_test_support::TestResult {
        use super::super::{
            tokenization::publication_test_support,
            unified_test_support::{ResponsesFixture, CONTACT},
        };
        use http_body_util::BodyExt;
        let fixture = ResponsesFixture::new();
        let (attempt, mut response) = c3_json_attempt(&fixture, mapped)?;
        response[field] = match field {
            "status" => json!("incomplete"),
            "error" => json!({"code":"synthetic-final-error"}),
            _ => json!({"reason":"synthetic-final-incomplete"}),
        };
        let delivered = attempt
            .restore_client_http_response(
                Response::builder()
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&response)?))?,
                ArgumentNormalizationMode::Exact,
                None,
            )
            .await;
        let status = delivered.status();
        let bytes = delivered.into_body().collect().await?.to_bytes();
        let value: Value = serde_json::from_slice(&bytes)?;
        let (published, pending, pins, ..) = publication_test_support::counts(&fixture.store)?;
        assert!(
            published == 0 && pending == 0 && pins == 0,
            "c3_json_invalid_final_completion_published"
        );
        assert!(
            status == StatusCode::INTERNAL_SERVER_ERROR
                && value["error"]["message"] == CLIENT_PROCESSING_ERROR_MESSAGE
                && value.get("output").is_none(),
            "c3_json_invalid_final_completion_released_tool"
        );
        assert!(
            !std::str::from_utf8(&bytes)?.contains(CONTACT),
            "c3_json_invalid_final_completion_leaked_text"
        );
        Ok(())
    }

    macro_rules! c3_json_final_tests {
        ($($name:ident => ($mapped:expr, $field:expr)),+ $(,)?) => { $(
            #[tokio::test(flavor = "current_thread")]
            #[serial_test::serial]
            async fn $name() -> super::super::unified_test_support::TestResult {
                c3_json_invalid_final_client($mapped, $field).await
            }
        )+ };
    }
    c3_json_final_tests!(
        private_c3_json_final_status_mapped => (true, "status"),
        private_c3_json_final_error_mapped => (true, "error"),
        private_c3_json_final_incomplete_mapped => (true, "incomplete_details"),
        private_c3_json_final_status_zero => (false, "status"),
        private_c3_json_final_error_zero => (false, "error"),
        private_c3_json_final_incomplete_zero => (false, "incomplete_details"),
    );

    #[tokio::test(flavor = "current_thread")]
    #[serial_test::serial]
    async fn private_c3_json_valid_final_control_preserves_delivery_and_publication(
    ) -> super::super::unified_test_support::TestResult {
        use super::super::{
            tokenization::publication_test_support,
            unified_test_support::{ResponsesFixture, CONTACT},
        };
        use http_body_util::BodyExt;
        for mapped in [false, true] {
            let fixture = ResponsesFixture::new();
            let (attempt, response) = c3_json_attempt(&fixture, mapped)?;
            let delivered = attempt
                .restore_client_http_response(
                    Response::builder()
                        .header("content-type", "application/json")
                        .body(Body::from(serde_json::to_vec(&response)?))?,
                    ArgumentNormalizationMode::Exact,
                    None,
                )
                .await;
            assert!(
                delivered.status() == StatusCode::OK,
                "c3_json_valid_final_rejected"
            );
            let bytes = delivered.into_body().collect().await?.to_bytes();
            let value: Value = serde_json::from_slice(&bytes)?;
            let arguments: Value = serde_json::from_str(
                value["output"][0]["arguments"]
                    .as_str()
                    .ok_or("c3_json_control_arguments_missing")?,
            )?;
            assert!(
                arguments["value"]
                    == if mapped {
                        CONTACT
                    } else {
                        "Synthetic ordinary input"
                    },
                "c3_json_valid_final_delivery_changed"
            );
            let (published, pending, pins, ..) = publication_test_support::counts(&fixture.store)?;
            assert!(
                published > 0 && pending == 0 && pins == 0,
                "c3_json_valid_final_not_published"
            );
        }
        Ok(())
    }

    #[test]
    #[serial_test::serial]
    fn private_c3_json_donor_proof_rejects_same_binding_foreign_attempt_before_encoding(
    ) -> super::super::unified_test_support::TestResult {
        use super::super::{
            payload_buffer::test_support::{self, Action},
            tokenization::publication_test_support,
            unified_test_support::ResponsesFixture,
        };
        use std::{cell::Cell, rc::Rc};
        let fixture = ResponsesFixture::new();
        let (mut recipient, source) = c3_json_attempt(&fixture, true)?;
        let (donor, donor_source) = c3_json_attempt(&fixture, true)?;
        assert!(source == donor_source, "c3_json_donor_source_differs");
        assert!(!donor.provenance.matches(&recipient.provenance));
        assert!(
            donor.binding.store_id == recipient.binding.store_id
                && donor.binding.namespace == recipient.binding.namespace
                && donor.binding.generation == recipient.binding.generation
                && donor.binding.policy_revision == recipient.binding.policy_revision
                && donor.binding.provider_fingerprint == recipient.binding.provider_fingerprint
                && donor.binding.reusable_principal == recipient.binding.reusable_principal
                && donor.binding.operation_lineage == recipient.binding.operation_lineage,
            "c3_json_donor_binding_differs"
        );
        let pair = recipient
            .response_protocol_pair()
            .map_err(|_| "c3_json_pair_failed")?;
        let (proof, donor_inspection) = donor
            .json_completion
            .finish(&source, &donor.binding, 200)
            .map_err(|_| "c3_json_donor_finish_failed")?;
        drop(donor_inspection);
        let verifier = std::mem::replace(
            &mut recipient.json_completion,
            completion_verifier::JsonCompletionVerifier::new(),
        );
        let (recipient_proof, inspection) = verifier
            .finish(&source, &recipient.binding, 200)
            .map_err(|_| "c3_json_recipient_finish_failed")?;
        drop(recipient_proof);
        let budget = recipient.budget.clone();
        let baseline = budget.used();
        let staged = stage_json_response_bound(
            &source,
            false,
            &recipient.lease,
            &recipient.binding,
            &budget,
            pair,
            &recipient.provenance,
            inspection,
        )
        .map_err(|_| "c3_json_recipient_stage_failed")?;
        let events = Rc::new(Cell::new(0));
        let observed = events.clone();
        let guard = test_support::install(move |_| {
            observed.set(observed.get() + 1);
            Action::Continue
        });
        let result = staged.encode_and_publish(
            &recipient.lease,
            &recipient.binding,
            proof,
            &budget,
            1024 * 1024,
        );
        drop(guard);
        let rejected = result.is_err();
        drop(result);
        let (published, pending, pins, ..) = publication_test_support::counts(&fixture.store)?;
        assert!(rejected, "c3_json_foreign_attempt_proof_accepted");
        assert_eq!(events.get(), 0, "c3_json_foreign_attempt_reached_encoding");
        assert!(
            published == 0 && pending == 0 && pins == 0,
            "c3_json_foreign_attempt_published"
        );
        assert!(
            budget.used() <= baseline,
            "c3_json_foreign_attempt_retained_staging_charge"
        );
        Ok(())
    }

    #[tokio::test]
    async fn private_sse_wrapper_charges_pending_frame_and_releases_on_cancel() {
        use futures::{poll, stream};
        let attempt = prepared_attempt("openai_responses", Some("fixture-session"));
        let budget = attempt.budget();
        let restorer = attempt
            .into_sse_restorer(200, ArgumentNormalizationMode::Exact)
            .expect("restorer");
        let baseline = budget.used();
        let upstream = stream::once(async { Ok(Bytes::from_static(b": fixture pending\n")) })
            .chain(stream::pending());
        let mut wrapped = wrap_sse_stream_with_restoration(upstream, restorer);
        assert!(poll!(wrapped.next()).is_pending());
        assert!(
            budget.used() >= baseline + 18,
            "pending frame must retain its charge"
        );
        drop(wrapped);
        let identity_charge = std::mem::size_of::<super::super::payload_budget::Reservation>()
            + 2 * std::mem::size_of::<usize>();
        let provenance_charge = identity_charge;
        let lifecycle_charge = identity_charge;
        let deferred_owner_charge = identity_charge;
        assert_eq!(
            baseline,
            provenance_charge + lifecycle_charge + deferred_owner_charge,
            "framing retains attempt provenance, lifecycle identity and deferred owner"
        );
        assert_eq!(
            budget.used(),
            0,
            "cancel releases frame and provenance reservations"
        );
    }

    #[tokio::test]
    async fn private_sse_wrapper_rejects_multiline_frame_before_eof() {
        use futures::stream;
        let attempt = prepared_attempt("openai_responses", Some("fixture-session"));
        let restorer = attempt
            .into_sse_restorer(200, ArgumentNormalizationMode::Exact)
            .expect("restorer");
        let line = Bytes::from(format!(":{}\n", "x".repeat(1022)));
        let upstream =
            stream::iter((0..1025).map(move |_| Ok(line.clone()))).chain(stream::pending());
        let mut wrapped = wrap_sse_stream_with_restoration(upstream, restorer);
        let frame = tokio::time::timeout(std::time::Duration::from_secs(1), wrapped.next())
            .await
            .expect("oversized frame fails without waiting for EOF")
            .expect("failure frame")
            .expect("fixed failure");
        assert!(std::str::from_utf8(&frame)
            .expect("utf8")
            .contains("proxy_processing_failed"));
    }

    #[tokio::test]
    async fn private_sse_wrapper_rejects_unterminated_completed_frame() {
        use futures::stream;
        let attempt = prepared_attempt("openai_responses", Some("fixture-session"));
        let restorer = attempt
            .into_sse_restorer(200, ArgumentNormalizationMode::Exact)
            .expect("restorer");
        let raw = format!(
            "event: response.completed\ndata: {}",
            json!({"type":"response.completed","response":completed_response()})
        );
        let mut wrapped =
            wrap_sse_stream_with_restoration(stream::iter([Ok(Bytes::from(raw))]), restorer);
        let output = wrapped
            .next()
            .await
            .expect("failure frame")
            .expect("fixed failure");
        assert!(
            std::str::from_utf8(&output)
                .expect("utf8")
                .contains("proxy_processing_failed"),
            "EOF cannot complete unfinished frame"
        );
    }
}

#[cfg(test)]
mod private_http_exit_tests {
    use super::*;

    #[tokio::test]
    async fn private_http_exit_retains_output_charge_and_removes_entity_validators() {
        let store = DlpTokenStore::new();
        let budget = PayloadBudget::with_limit(4 * 1024 * 1024);
        let mut lease = store.admit("codex", None, 1).expect("request lease");
        lease
            .prepare_with_budget(
                &[super::super::tokenization::TokenFragment {
                    raw: "fixture-private-value",
                    whole_span_categories: &[super::super::tokenization::TokenCategory::Custom],
                }],
                &[],
                budget.clone(),
                |_| Ok(()),
            )
            .expect("mapping");
        let token = lease.authorized_tokens().expect("tokens").as_slice()[0].to_owned();
        let binding = lease
            .bind_actual_attempt(
                "fixture",
                "openai_responses",
                "http://127.0.0.1:1/responses",
                Some("fixture-account"),
            )
            .expect("binding");
        let attempt = DlpPreparedAttempt {
            lease,
            binding,
            budget: budget.clone(),
            endpoint_is_compact: false,
            upstream_json_protocol: Some(JsonCompletionProtocol::NativeResponses),
            provenance: super::super::restoration_core::AttemptProvenance::new(&budget)
                .expect("attempt provenance fixture"),
            json_completion: completion_verifier::JsonCompletionVerifier::new(),
            audit_state: std::sync::Arc::new(
                RequestAuditState::new(&AppType::Codex, None, budget.clone())
                    .expect("audit state fixture"),
            ),
            initial_audit: std::sync::Arc::new(
                PendingRequestAudit::empty(budget.clone()).expect("audit summary fixture"),
            ),
            upstream_payload_owner: None,
            history_mapping: None,
            native_history_digest: None,
            native_protocol: None,
            native_original: None,
            native_error_safety: None,
            sse_evidence: std::sync::Mutex::new(None),
            pending_audit: PendingRequestAudit::empty(budget.clone())
                .expect("audit summary fixture"),
        };
        let raw = json!({"id":"resp_exit","status":"completed","output":[{"id":"msg_exit","type":"message","role":"assistant","content":[{"type":"output_text","text":token}]}]}).to_string();
        let response = Response::builder()
            .header("content-type", "application/json")
            .header("etag", "fixture")
            .header("digest", "fixture")
            .header("content-md5", "fixture")
            .body(Body::from(raw))
            .expect("response");
        let response = attempt
            .restore_client_http_response(response, ArgumentNormalizationMode::Exact, None)
            .await;
        assert_eq!(response.status(), StatusCode::OK);
        for name in [
            "etag",
            "digest",
            "content-md5",
            "content-encoding",
            "transfer-encoding",
        ] {
            assert!(!response.headers().contains_key(name));
        }
        assert!(
            budget.used() > 0,
            "returned body must own output reservation"
        );
        let bytes = axum::body::to_bytes(response.into_body(), 1024 * 1024)
            .await
            .expect("body");
        assert!(std::str::from_utf8(&bytes)
            .expect("utf8")
            .contains("fixture-private-value"));
        drop(bytes);
        assert_eq!(budget.used(), 0, "consumed response releases charge");
    }
}

#[cfg(test)]
mod private_production_activation_tests {
    use super::*;

    #[test]
    #[serial_test::serial]
    fn private_default_gateway_uses_captured_restore_action_without_fixture_rules() {
        let previous = (*super::super::runtime::get_security_runtime()).clone();
        struct Reset(SecurityRuntimeConfig);
        impl Drop for Reset {
            fn drop(&mut self) {
                super::super::runtime::set_security_runtime_for_test(self.0.clone());
            }
        }
        let _reset = Reset(previous);
        for (id, text, prefix) in [
            ("chinese_phone_number", "13800138000", "rredact-cn_phone-"),
            (
                "catalog:generic-sk-key-notice",
                "sk-syntheticfixture",
                "rredact-api_token-",
            ),
        ] {
            let mut runtime = SecurityRuntimeConfig::default();
            runtime
                .builtin_action_overrides
                .insert(id.into(), super::super::types::AuditAction::RedactRestore);
            super::super::runtime::set_security_runtime_for_test(runtime.clone());
            let gateway = RestorationGateway::new_default();
            let mut body = json!({"input":text});
            let admission = admit_http_request(
                &gateway,
                &AppType::Codex,
                true,
                false,
                false,
                &mut body,
                &HeaderMap::new(),
            )
            .expect("admission");
            assert!(
                matches!(admission, RestorationAdmission::Admitted(_)),
                "real runtime restoration activates default gateway"
            );
            assert!(body["input"]
                .as_str()
                .is_some_and(|text| text.starts_with(prefix)));
            let mut unsupported = json!({"messages":[{"role":"user","content":text}]});
            let admission = admit_http_request(
                &gateway,
                &AppType::Codex,
                false,
                false,
                false,
                &mut unsupported,
                &HeaderMap::new(),
            )
            .expect("unsupported admission");
            assert!(
                matches!(admission, RestorationAdmission::Intercepted(_)),
                "actual restore match on unsupported route must reject"
            );
            let mut ordinary = json!({"input":"hello"});
            runtime.builtin_action_overrides.clear();
            super::super::runtime::set_security_runtime_for_test(runtime);
            assert!(matches!(
                admit_http_request(
                    &gateway,
                    &AppType::Codex,
                    true,
                    false,
                    false,
                    &mut ordinary,
                    &HeaderMap::new()
                )
                .expect("ordinary"),
                RestorationAdmission::Passthrough
            ));
        }
    }
}

#[cfg(test)]
mod private_guard_owner_tests {
    use super::*;
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    };
    struct Probe(Arc<AtomicBool>);
    impl Drop for Probe {
        fn drop(&mut self) {
            self.0.store(true, Ordering::Relaxed);
        }
    }

    #[test]
    fn private_outbound_encoding_precharges_capacity_and_keeps_clone_owner(
    ) -> Result<(), TokenError> {
        use super::super::payload_buffer::test_support::{self, Action};
        use std::{cell::Cell, rc::Rc};
        let value = json!({"synthetic": "line\n\"quoted\" 中文", "number": 1.25});
        let expected = serde_json::to_vec(&value).map_err(|_| TokenError::RedactionFailed)?;
        let denied_budget = PayloadBudget::with_limit(expected.len() - 1);
        let observations = Rc::new(Cell::new(0));
        let observed = observations.clone();
        let hook = test_support::install(move |_| {
            observed.set(observed.get() + 1);
            Action::Continue
        });
        assert!(matches!(
            encode_outbound_with_budget(&value, denied_budget.clone()),
            Err(DlpForwardError::CapacityExceeded)
        ));
        assert_eq!(denied_budget.used(), 0);
        assert_eq!(
            observations.get(),
            0,
            "capacity refusal must happen before allocation or copying"
        );
        drop(hook);
        let budget = PayloadBudget::with_limit(expected.len());
        let bytes = encode_outbound_with_budget(&value, budget.clone())
            .map_err(|_| TokenError::RedactionFailed)?;
        assert_eq!(bytes.as_ref(), expected.as_slice());
        assert_eq!(budget.used(), expected.len());
        let clone = bytes.clone();
        drop(bytes);
        assert_eq!(budget.used(), expected.len());
        drop(clone);
        assert_eq!(budget.used(), 0);
        Ok(())
    }

    #[test]
    fn private_guard_owner_json_bytes_retain_until_last_clone_drops() -> Result<(), TokenError> {
        let dropped = Arc::new(AtomicBool::new(false));
        let output =
            PayloadBuffer::try_from_slice(b"synthetic", PayloadBudget::with_limit(1024), 1024)?;
        let bytes = bytes_with_owner(output, Probe(dropped.clone()));
        assert!(!dropped.load(Ordering::Relaxed));
        let clone = bytes.clone();
        drop(bytes);
        assert!(!dropped.load(Ordering::Relaxed));
        drop(clone);
        assert!(dropped.load(Ordering::Relaxed));
        Ok(())
    }

    #[tokio::test]
    async fn private_guard_owner_sse_retains_on_pending_and_releases_on_cancel() {
        use futures::{poll, stream};
        let dropped = Arc::new(AtomicBool::new(false));
        let stream =
            stream::once(async { Ok::<_, std::io::Error>(Bytes::from_static(b": fixture\n\n")) })
                .chain(stream::pending());
        let mut guarded = retain_stream_owner(stream, Probe(dropped.clone()));
        assert!(!dropped.load(Ordering::Relaxed));
        assert!(guarded.next().await.is_some());
        assert!(poll!(guarded.next()).is_pending());
        assert!(!dropped.load(Ordering::Relaxed));
        drop(guarded);
        assert!(dropped.load(Ordering::Relaxed));
    }
}

fn bytes_with_owner<T: Send + 'static>(output: PayloadBuffer, owner: T) -> Bytes {
    struct OwnedBytes<T> {
        output: PayloadBuffer,
        _owner: T,
    }
    impl<T> AsRef<[u8]> for OwnedBytes<T> {
        fn as_ref(&self) -> &[u8] {
            self.output.as_slice()
        }
    }
    Bytes::from_owner(OwnedBytes {
        output,
        _owner: owner,
    })
}

fn retain_stream_owner<S, T>(
    stream: S,
    owner: T,
) -> std::pin::Pin<Box<dyn Stream<Item = Result<Bytes, std::io::Error>> + Send>>
where
    S: Stream<Item = Result<Bytes, std::io::Error>> + Send + 'static,
    T: Send + 'static,
{
    Box::pin(async_stream::stream! {
        let _owner = owner;
        let mut stream = Box::pin(stream);
        while let Some(chunk) = stream.next().await { yield chunk; }
    })
}

#[cfg(test)]
mod response_namespace_boundary_tests {
    use super::*;
    use crate::proxy::providers::{
        transform_codex_chat::flatten_namespace_tool_name,
        transform_codex_responses_namespace::{
            namespace_restore_map, restore_response_namespaces, NamespaceLookup,
        },
        transform_codex_responses_xai_sanitize::create_xai_native_responses_sse_stream,
    };
    use std::{cell::RefCell, rc::Rc};

    fn fixture(count: usize) -> Value {
        json!({"tools":[{"type":"namespace","name":"synthetic_ns","tools":
            (0..count).map(|i| json!({"type":"function","name":format!("tool_{i}"),"parameters":{"type":"object"}})).collect::<Vec<_>>()
        }]})
    }

    #[test]
    fn guarded_response_namespace_exact_capacity_and_independent_owners() -> Result<(), TokenError>
    {
        let body = fixture(1);
        let exact = 4 * std::mem::size_of::<ResponseNamespaceEntry>()
            + "synthetic_ns".len()
            + "tool_0".len();
        let budget = PayloadBudget::with_limit(exact);
        let table = DlpResponseNamespaceTable::build(&body, budget.clone())?;
        assert_eq!(budget.used(), exact);
        assert_eq!(
            table.lookup("synthetic_ns__tool_0"),
            Some(("synthetic_ns", "tool_0"))
        );
        drop(table);
        assert_eq!(budget.used(), 0);
        let tiny = PayloadBudget::with_limit(exact - 1);
        assert!(matches!(
            DlpResponseNamespaceTable::build(&body, tiny.clone()),
            Err(TokenError::CapacityExceeded)
        ));
        assert_eq!(tiny.used(), 0);
        let budget = PayloadBudget::with_limit(exact * 2 + 37);
        let foreign = budget.reserve(37)?;
        let first = DlpResponseNamespaceTable::build(&body, budget.clone())?;
        let second = DlpResponseNamespaceTable::build(&body, budget.clone())?;
        assert_eq!(budget.used(), exact * 2 + 37);
        drop(first);
        assert_eq!(budget.used(), exact + 37);
        drop(second);
        assert_eq!(budget.used(), 37);
        drop(foreign);
        assert_eq!(budget.used(), 0);
        Ok(())
    }

    #[test]
    fn guarded_response_namespace_empty_table_has_zero_backing() -> Result<(), TokenError> {
        for body in [
            json!({}),
            json!({"tools":null}),
            json!({"tools":[{"type":"namespace","name":"  ","tools":[]}]}),
            json!({"tools":[{"type":"namespace","name":"ns","tools":null,"children":[{"type":"function","name":"blocked"}]}]}),
        ] {
            let budget = PayloadBudget::with_limit(0);
            let table = DlpResponseNamespaceTable::build(&body, budget.clone())?;
            assert!(table.is_empty());
            assert_eq!(budget.used(), 0);
        }
        Ok(())
    }

    #[test]
    fn guarded_response_namespace_legacy_lookup_and_exact_response_bytes() -> Result<(), TokenError>
    {
        let long_namespace = " 合成工具🦀 ".repeat(24);
        let long_name = "工具é".repeat(30);
        let body = json!({"tools":[
            {"type":"function","name":"untouched"},
            {"type":"namespace","name":" a__b ","tools":[{"type":"function","name":" c "},{"type":"function","name":"c"}]},
            {"type":"namespace","name":"a","children":[{"type":"function","name":"b__c"}]},
            {"type":"namespace","name":long_namespace,"children":[{"type":"function","name":long_name},{"type":"custom","name":"ignored"}]},
            {"type":"namespace","name":"blocked","tools":false,"children":[{"type":"function","name":"skip"}]},
            {"type":"namespace","name":"empty","tools":[{}, {"type":"function","name":" "}]}]});
        let source = body.to_string();
        let legacy = namespace_restore_map(&body);
        let budget = PayloadBudget::with_limit(1024 * 1024);
        let charged = DlpResponseNamespaceTable::build(&body, budget.clone())?;
        assert_eq!(charged.entries.len(), legacy.len());
        for (flat, entry) in &legacy {
            assert_eq!(
                charged.lookup(flat),
                Some((entry.namespace.as_str(), entry.name.as_str()))
            );
        }
        assert_eq!(
            charged.lookup("a__b__c"),
            Some(("a__b", "c")),
            "first collision wins"
        );
        assert!(charged.lookup("blocked__skip").is_none());
        assert!(charged.lookup("missing").is_none());
        let mut expected = json!({"output":legacy.keys().map(|name| json!({"type":"function_call","name":name,"arguments":"{}","nested":{"type":"function_call","name":name}})).collect::<Vec<_>>(), "negative_zero":-0.0});
        let mut actual = expected.clone();
        assert!(restore_response_namespaces(&mut expected, &legacy));
        assert!(restore_response_namespaces(&mut actual, &charged));
        assert_eq!(actual.to_string(), expected.to_string());
        assert_eq!(body.to_string(), source);
        let flat = flatten_namespace_tool_name(long_namespace.trim(), &long_name);
        assert!(flat.len() <= 64 && charged.lookup(&flat).is_some());
        drop(body);
        assert_eq!(
            charged.lookup("a__b__c"),
            Some(("a__b", "c")),
            "table owns names without retaining source tree"
        );
        drop(charged);
        assert_eq!(budget.used(), 0);
        Ok(())
    }

    #[test]
    fn guarded_response_namespace_growth_charges_old_new_and_rolls_back() -> Result<(), TokenError>
    {
        use super::super::payload_metadata::test_support as meta;
        let budget = PayloadBudget::with_limit(1024 * 1024);
        let events = Rc::new(RefCell::new(Vec::new()));
        let captured = events.clone();
        let observer_budget = budget.clone();
        let _hook = meta::install(move |event| {
            if event.kind == meta::Kind::Vector {
                assert_eq!(event.used, observer_budget.used());
                captured
                    .borrow_mut()
                    .push((event.phase, event.bytes, event.used));
            }
            meta::Action::Continue
        });
        let table = DlpResponseNamespaceTable::build(&fixture(5), budget.clone())?;
        let entry = std::mem::size_of::<ResponseNamespaceEntry>();
        let text = 5 * ("synthetic_ns".len() + "tool_0".len());
        let observed = events.borrow();
        let growth = observed
            .iter()
            .find(|(phase, bytes, _)| {
                *phase == meta::Phase::BeforeAllocation && *bytes == 8 * entry
            })
            .expect("second vector growth");
        assert_eq!(
            growth.2,
            text + (4 + 8) * entry,
            "old and replacement backing coexist"
        );
        assert_eq!(budget.used(), text + 8 * entry);
        drop(observed);
        drop(table);
        assert_eq!(budget.used(), 0);
        drop(_hook);
        let tiny = PayloadBudget::with_limit(text + 12 * entry - 1);
        assert!(matches!(
            DlpResponseNamespaceTable::build(&fixture(5), tiny.clone()),
            Err(TokenError::CapacityExceeded)
        ));
        assert_eq!(tiny.used(), 0);
        Ok(())
    }

    #[test]
    fn guarded_response_namespace_each_allocation_failure_rolls_back() -> Result<(), TokenError> {
        use super::super::{
            payload_metadata::test_support as meta, payload_text::test_support as text,
        };
        for phase in [meta::Phase::BeforeAllocation, meta::Phase::AfterAllocation] {
            for nth in [1, 2] {
                let budget = PayloadBudget::with_limit(1024 * 1024);
                let foreign = budget.reserve(29)?;
                let mut seen = 0;
                let _hook = meta::install(move |event| {
                    if event.kind == meta::Kind::Vector && event.phase == phase {
                        seen += 1;
                        if seen == nth {
                            return meta::Action::Fail;
                        }
                    }
                    meta::Action::Continue
                });
                assert!(matches!(
                    DlpResponseNamespaceTable::build(&fixture(5), budget.clone()),
                    Err(TokenError::Unavailable)
                ));
                assert_eq!(budget.used(), 29);
                drop(foreign);
                assert_eq!(budget.used(), 0);
            }
        }
        for phase in [
            text::Phase::BeforeAllocation,
            text::Phase::AfterAllocation,
            text::Phase::BeforeCopy,
        ] {
            for nth in 1..=10 {
                let budget = PayloadBudget::with_limit(1024 * 1024);
                let foreign = budget.reserve(31)?;
                let mut seen = 0;
                let _hook = text::install(move |event| {
                    if event.phase == phase {
                        seen += 1;
                        if seen == nth {
                            return text::Action::Fail;
                        }
                    }
                    text::Action::Continue
                });
                assert!(matches!(
                    DlpResponseNamespaceTable::build(&fixture(5), budget.clone()),
                    Err(TokenError::Unavailable)
                ));
                assert_eq!(budget.used(), 31);
                drop(foreign);
                assert_eq!(budget.used(), 0);
            }
        }
        Ok(())
    }

    #[tokio::test]
    async fn guarded_response_namespace_sse_unpolled_error_cancel_and_completion_release(
    ) -> Result<(), Box<dyn std::error::Error>> {
        for mode in ["unpolled", "error", "cancel", "complete"] {
            let budget = PayloadBudget::with_limit(1024 * 1024);
            let foreign = budget.reserve(43)?;
            let table = DlpResponseNamespaceTable::build(&fixture(1), budget.clone())?;
            let held = budget.used();
            let source = futures::stream::iter(match mode {
                "error" => vec![Err(std::io::Error::other("synthetic stream error"))],
                _ => vec![Ok(Bytes::from_static(b"data: {\"type\":\"function_call\",\"name\":\"synthetic_ns__tool_0\",\"arguments\":\"{}\"}\n\n"))],
            });
            let stream = create_xai_native_responses_sse_stream(source, table);
            let mut stream = Box::pin(stream);
            assert_eq!(budget.used(), held, "unpolled stream owns its table");
            if mode != "unpolled" {
                let item = stream.next().await.expect("synthetic stream item");
                if mode == "error" {
                    assert!(item.is_err());
                } else {
                    assert!(std::str::from_utf8(&item?)?.contains("\"namespace\":\"synthetic_ns\""));
                }
                assert_eq!(
                    budget.used(),
                    held,
                    "yield alone does not release the table"
                );
            }
            if mode == "cancel" {
                // A pending source keeps the table across await until stream drop.
                let pending = futures::stream::pending::<Result<Bytes, std::io::Error>>();
                let second = DlpResponseNamespaceTable::build(&fixture(1), budget.clone())?;
                let mut waiting = Box::pin(create_xai_native_responses_sse_stream(pending, second));
                assert!(futures::poll!(waiting.next()).is_pending());
                drop(waiting);
                assert_eq!(budget.used(), held);
            }
            if mode == "complete" {
                assert!(stream.next().await.is_none());
            }
            drop(stream);
            assert_eq!(budget.used(), 43);
            drop(foreign);
            assert_eq!(budget.used(), 0);
        }
        Ok(())
    }
}

#[cfg(test)]
pub(super) mod response_namespace_test_support {
    use super::{DlpForwardError, PayloadBudget};
    use std::{
        cell::RefCell,
        marker::PhantomData,
        rc::Rc,
        sync::{Arc, Mutex},
    };

    #[derive(Clone, Copy, Default, Debug)]
    pub(in super::super) struct Snapshot {
        pub attempts: usize,
        pub created: usize,
        pub released: usize,
        pub live_charge: usize,
        pub failure: Option<DlpForwardError>,
    }
    #[derive(Default)]
    struct State {
        snapshot: Snapshot,
        exhaust: bool,
        blocker: Option<super::super::payload_budget::Reservation>,
    }
    thread_local! { static ACTIVE: RefCell<Option<Arc<Mutex<State>>>> = const { RefCell::new(None) }; }
    pub(in super::super) struct Probe {
        state: Arc<Mutex<State>>,
        previous: Option<Arc<Mutex<State>>>,
        _thread_bound: PhantomData<Rc<()>>,
    }
    impl Probe {
        pub(in super::super) fn snapshot(&self) -> Snapshot {
            self.state.lock().expect("synthetic table probe").snapshot
        }
        pub(in super::super) fn release_blocker(&self) {
            self.state
                .lock()
                .expect("synthetic table probe")
                .blocker
                .take();
        }
    }
    impl Drop for Probe {
        fn drop(&mut self) {
            ACTIVE.with(|slot| slot.replace(self.previous.take()));
        }
    }
    pub(in super::super) fn install(exhaust: bool) -> Probe {
        let state = Arc::new(Mutex::new(State {
            exhaust,
            ..Default::default()
        }));
        Probe {
            previous: ACTIVE.with(|slot| slot.replace(Some(state.clone()))),
            state,
            _thread_bound: PhantomData,
        }
    }
    pub(super) fn before_build(budget: &PayloadBudget) -> Result<(), DlpForwardError> {
        ACTIVE.with(|slot| {
            if let Some(state) = slot.borrow().as_ref() {
                let mut state = state.lock().expect("synthetic table probe");
                state.snapshot.attempts += 1;
                if state.exhaust {
                    state.blocker = Some(
                        budget
                            .reserve(budget.limit() - budget.used())
                            .map_err(DlpForwardError::from_token_error)?,
                    );
                }
            }
            Ok(())
        })
    }
    pub(super) fn after_build(failure: Option<DlpForwardError>) {
        ACTIVE.with(|slot| {
            if let Some(state) = slot.borrow().as_ref() {
                state
                    .lock()
                    .expect("synthetic table probe")
                    .snapshot
                    .failure = failure;
            }
        });
    }
    pub(super) struct OwnerProbe(Option<(Arc<Mutex<State>>, usize)>);
    impl OwnerProbe {
        pub(super) fn new(bytes: usize) -> Self {
            Self(ACTIVE.with(|slot| {
                slot.borrow().as_ref().map(|state| {
                    let mut snapshot = state.lock().expect("synthetic table probe");
                    snapshot.snapshot.created += 1;
                    snapshot.snapshot.live_charge += bytes;
                    (state.clone(), bytes)
                })
            }))
        }
    }
    impl Drop for OwnerProbe {
        fn drop(&mut self) {
            if let Some((state, bytes)) = self.0.take() {
                let mut state = state.lock().expect("synthetic table probe");
                state.snapshot.released += 1;
                state.snapshot.live_charge -= bytes;
            }
        }
    }
}

/// Synthetic native Responses adapter; no production dispatch or completion publication.
#[cfg(test)]
pub(super) async fn prepare_text_projection_http_for_test(
    response: Response,
    lease: DlpRequestLease,
    budget: PayloadBudget,
) -> Result<Response, RestoreError> {
    use responses_adapter::{prepare_text_projection, TextProjectionStream};

    let (mut parts, body) = response.into_parts();
    if !parts.status.is_success()
        || parts
            .headers
            .get_all(header::CONTENT_ENCODING)
            .iter()
            .any(|value| {
                !value
                    .to_str()
                    .is_ok_and(|value| value.trim().eq_ignore_ascii_case("identity"))
            })
    {
        return Err(RestoreError::RestoreFailed);
    }
    let content_type = parts
        .headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(';').next())
        .map(str::trim)
        .ok_or(RestoreError::RestoreUnsupported)?;
    let is_sse = if content_type.eq_ignore_ascii_case("text/event-stream") {
        true
    } else if content_type.eq_ignore_ascii_case("application/json") {
        false
    } else {
        return Err(RestoreError::RestoreUnsupported);
    };
    let capacity_error =
        |_| RestoreError::FailedBecause(super::restore_diagnostics::RestoreReason::RestoreCapacity);
    let mut stream = body.into_data_stream();
    if is_sse {
        let mut projection = TextProjectionStream::new(lease, budget.clone())?;
        projection.initialize()?;
        let mut frame = PayloadBuffer::new(budget, 1024 * 1024).map_err(capacity_error)?;
        let mut line_start = 0;
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|_| RestoreError::RestoreFailed)?;
            let mut offset = 0;
            while offset < chunk.len() {
                let end = chunk[offset..]
                    .iter()
                    .position(|byte| *byte == b'\n')
                    .map_or(chunk.len(), |position| offset + position + 1);
                frame.append(&chunk[offset..end]).map_err(capacity_error)?;
                offset = end;
                if chunk[end - 1] != b'\n' {
                    continue;
                }
                let line = &frame.as_slice()[line_start..];
                let blank = line == b"\n" || line == b"\r\n";
                line_start = frame.len();
                if !blank {
                    continue;
                }
                let raw = std::str::from_utf8(frame.as_slice())
                    .map_err(|_| RestoreError::RestoreFailed)?;
                projection.push_frame(raw)?;
                frame.clear();
                line_start = 0;
            }
        }
        if frame.len() != 0 {
            return Err(RestoreError::RestoreFailed);
        }
        let frames = projection.finish(true)?;
        clear_entity_headers(&mut parts.headers);
        // The charged vector stays alive while each Bytes retains its own output owner.
        let output = async_stream::stream! {
            for bytes in frames.as_slice() {
                yield Ok::<_, std::io::Error>(bytes.clone());
            }
        };
        return Ok(Response::from_parts(parts, Body::from_stream(output)));
    }

    let mut input = PayloadBuffer::new(
        budget.clone(),
        crate::proxy::hyper_client::MAX_RESPONSE_BODY_BYTES,
    )
    .map_err(capacity_error)?;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|_| RestoreError::RestoreFailed)?;
        input.append(&chunk).map_err(capacity_error)?;
    }
    let raw = std::str::from_utf8(input.as_slice()).map_err(|_| RestoreError::RestoreFailed)?;
    let parsed =
        PayloadJson::try_from_str(raw, budget.clone(), budget.limit()).map_err(capacity_error)?;
    let projected = prepare_text_projection(parsed.as_value(), &lease, &budget)?;
    let output = responses_adapter::serialize_json_with_budget(
        projected.as_value(),
        &budget,
        crate::proxy::hyper_client::MAX_RESPONSE_BODY_BYTES,
    )?;
    let bytes = bytes_with_owner(output, ());
    let length =
        HeaderValue::from_str(&bytes.len().to_string()).map_err(|_| RestoreError::RestoreFailed)?;
    clear_entity_headers(&mut parts.headers);
    parts.headers.insert(header::CONTENT_LENGTH, length);
    Ok(Response::from_parts(parts, Body::from(bytes)))
}

#[cfg(test)]
pub(super) mod completion_selection_test_support {
    use std::cell::RefCell;

    thread_local! {
        static AFTER_SELECTION: RefCell<Option<Box<dyn FnOnce()>>> = RefCell::new(None);
    }

    pub(crate) struct Guard(Option<Box<dyn FnOnce()>>);

    pub(crate) fn install(hook: impl FnOnce() + 'static) -> Guard {
        Guard(AFTER_SELECTION.with(|slot| slot.replace(Some(Box::new(hook)))))
    }

    pub(super) fn after_selection() {
        let hook = AFTER_SELECTION.with(|slot| slot.take());
        if let Some(hook) = hook {
            hook();
        }
    }

    impl Drop for Guard {
        fn drop(&mut self) {
            AFTER_SELECTION.with(|slot| {
                slot.replace(self.0.take());
            });
        }
    }
}

#[cfg(test)]
mod c3_early_inspection_tests {
    use super::super::tokenization::publication_test_support;
    use super::*;
    use http_body_util::BodyExt;
    type TestResult = Result<(), Box<dyn std::error::Error>>;

    async fn early_exit(mode: &str) -> TestResult {
        let _runtime = super::super::unified_test_support::ResponsesFixture::new();
        let store = DlpTokenStore::new();
        let budget = PayloadBudget::with_limit(4 * 1024 * 1024);
        let gateway = RestorationGateway::with_test_store_and_rules(
            store.clone(),
            &["chinese_phone_number"],
            budget.clone(),
        );
        let mut body = json!({"model":"synthetic-model","input":"13800138000","stream":true});
        let mut headers = HeaderMap::new();
        headers.insert(
            "session-id",
            HeaderValue::from_static("synthetic-early-session"),
        );
        let request = match admit_http_request(
            &gateway,
            &AppType::Codex,
            true,
            false,
            false,
            &mut body,
            &headers,
        )? {
            RestorationAdmission::Admitted(request) => request,
            _ => return Err("synthetic early-inspection admission failed".into()),
        };
        let (attempt, owner) = request.prepare_owned_attempt_for_proxy(
            "synthetic-provider",
            "anthropic",
            "https://synthetic.invalid/v1/messages",
            Some("synthetic-principal"),
            &mut body,
            Ok,
        )?;
        let tracker = attempt
            .upstream_sse_evidence_tracker()
            .ok_or("synthetic tracker missing")?;
        assert!(
            tracker
                .lock()
                .map_err(|_| "synthetic tracker poisoned")?
                .inspection
                .is_some(),
            "c3_early_inspection_precondition"
        );
        match mode {
            "attempt_drop" => drop(attempt),
            "unpolled_wrapper" => {
                let response = Response::builder()
                    .header("content-type", "text/event-stream")
                    .body(Body::empty())?;
                drop(attempt.restore_client_http_response(
                    response,
                    ArgumentNormalizationMode::Exact,
                    Some(tracker.clone()),
                ));
            }
            "pending_wrapper" | "unpolled_body" | "repeat_tracker" => {
                if mode == "repeat_tracker" {
                    let baseline =
                        super::super::restoration_core::ResponseLifecycle::begin_count_for_test();
                    let repeated = attempt
                        .upstream_sse_evidence_tracker()
                        .ok_or("synthetic repeat missing")?;
                    assert!(
                        std::sync::Arc::ptr_eq(&tracker, &repeated),
                        "c3_early_exit_replaced_tracker"
                    );
                    assert_eq!(
                        super::super::restoration_core::ResponseLifecycle::begin_count_for_test(),
                        baseline,
                        "c3_early_exit_created_second_owner"
                    );
                }
                let response = Response::builder()
                    .header("content-type", "text/event-stream")
                    .body(Body::from_stream(futures::stream::pending::<
                        Result<Bytes, std::io::Error>,
                    >()))?;
                let delivered = attempt
                    .restore_client_http_response(
                        response,
                        ArgumentNormalizationMode::Exact,
                        Some(tracker.clone()),
                    )
                    .await;
                if mode == "pending_wrapper" {
                    let mut stream = delivered.into_body().into_data_stream();
                    assert!(
                        futures::poll!(stream.next()).is_pending(),
                        "c3_pending_precondition"
                    );
                    assert!(
                        tracker
                            .lock()
                            .map_err(|_| "synthetic tracker poisoned")?
                            .inspection
                            .is_some(),
                        "c3_pending_owner_not_lent"
                    );
                    drop(stream);
                } else {
                    drop(delivered);
                }
            }
            "poisoned_attempt_drop" | "poisoned_wrapper" => {
                let other = tracker.clone();
                let _ = std::thread::spawn(move || {
                    if let Ok(_lock) = other.lock() {
                        panic!("synthetic cleanup poison");
                    }
                })
                .join();
                if mode == "poisoned_wrapper" {
                    let response = Response::builder()
                        .header("content-type", "text/event-stream")
                        .body(Body::empty())?;
                    let delivered = attempt
                        .restore_client_http_response(
                            response,
                            ArgumentNormalizationMode::Exact,
                            Some(tracker.clone()),
                        )
                        .await;
                    assert_eq!(delivered.status(), StatusCode::INTERNAL_SERVER_ERROR);
                    drop(delivered);
                } else {
                    drop(attempt);
                }
            }
            "upstream_error" | "encoded" => {
                let mut builder = Response::builder().header("content-type", "text/event-stream");
                if mode == "encoded" {
                    builder = builder.header("content-encoding", "gzip");
                } else {
                    builder = builder.status(429);
                }
                let response = builder.body(Body::from("synthetic upstream body"))?;
                let delivered = attempt
                    .restore_client_http_response(
                        response,
                        ArgumentNormalizationMode::Exact,
                        Some(tracker.clone()),
                    )
                    .await;
                assert_eq!(
                    delivered.status().as_u16(),
                    if mode == "encoded" { 500 } else { 429 }
                );
                if mode == "upstream_error" {
                    assert_eq!(
                        delivered.into_body().collect().await?.to_bytes().as_ref(),
                        b"synthetic upstream body",
                        "c3_early_exit_changed_upstream_error"
                    );
                } else {
                    drop(delivered);
                }
            }
            _ => return Err("synthetic early-inspection mode unknown".into()),
        }
        {
            let retained = match tracker.lock() {
                Ok(lock) => lock,
                Err(poisoned) => poisoned.into_inner(),
            };
            assert!(
                retained.inspection.is_none(),
                "c3_early_exit_stranded_inspection"
            );
            assert!(
                retained.state.is_none(),
                "c3_early_exit_stranded_original_evidence"
            );
        }
        assert_eq!(
            publication_test_support::counts(&store)?,
            (0, 0, 0, 0),
            "c3_early_exit_published_context"
        );
        drop(owner);
        drop(request);
        drop(gateway);
        assert_eq!(budget.used(), 0, "c3_early_exit_retained_payload_charge");
        drop(tracker);
        Ok(())
    }

    macro_rules! early_cases {
        ($($name:ident => $mode:literal),+ $(,)?) => {$(
            #[tokio::test(flavor = "current_thread")]
            #[serial_test::serial]
            async fn $name() -> TestResult { early_exit($mode).await }
        )+};
    }
    early_cases!(
        c3_early_inspection_attempt_drop => "attempt_drop",
        c3_early_inspection_unpolled_wrapper => "unpolled_wrapper",
        c3_early_inspection_poisoned_attempt_drop => "poisoned_attempt_drop",
        c3_early_inspection_upstream_error => "upstream_error",
        c3_early_inspection_encoded => "encoded",
        c3_early_inspection_pending_wrapper => "pending_wrapper",
        c3_early_inspection_unpolled_body => "unpolled_body",
        c3_early_inspection_repeat_tracker => "repeat_tracker",
        c3_early_inspection_poisoned_wrapper => "poisoned_wrapper",
    );
}
