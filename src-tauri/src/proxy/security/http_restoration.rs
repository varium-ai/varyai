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
    completed_context::{
        valid_external_id, ActualAttemptBinding, CompletedCallSpec, CompletionWitness,
    },
    completion_evidence::CompletionObservation,
    completion_json::{JsonCompletion, JsonCompletionProtocol},
    extractor::extract_payload_texts,
    inspect_and_intercept_mut_with_headers,
    payload_budget::PayloadBudget,
    payload_buffer::PayloadBuffer,
    payload_json::{JsonStringReplacement, PayloadJson},
    payload_metadata::PayloadVec,
    payload_text::PayloadText,
    request_carriers::CodexRequestCarriers,
    request_detection::detect_spans_with_budget,
    request_plan::{EffectiveAction, PlanError},
    request_preparation::prepare_codex_request_with_budget,
    response_restoration::{
        failed_sse_frame, stage_json_response, ArgumentNormalizationMode, RestoreError,
        SseRestorationState,
    },
    restoration_audit::{PendingRequestAudit, RequestAuditState},
    runtime::{admit_security_runtime, SecurityRuntimeConfig},
    token_store_owners::{SharedAllocation, StoreText, StoreVec},
    tokenization::{visit_token_references, DlpRequestLease, DlpTokenStore, TokenError},
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
    base_lease: DlpRequestLease,
    runtime: SecurityRuntimeConfig,
    restore_rules: StoreVec<StoreText>,
    budget: PayloadBudget,
    source_body: PayloadJson,
    pretokenized: bool,
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

pub(crate) struct DlpPreparedAttempt {
    pub(super) lease: DlpRequestLease,
    pub(super) binding: ActualAttemptBinding,
    pub(super) budget: PayloadBudget,
    pub(crate) endpoint_is_compact: bool,
    upstream_json_protocol: Option<JsonCompletionProtocol>,
    json_completion: std::sync::atomic::AtomicU8,
    audit_state: std::sync::Arc<RequestAuditState>,
    initial_audit: std::sync::Arc<PendingRequestAudit>,
    pending_audit: PendingRequestAudit,
    upstream_payload_owner: Option<super::UpstreamPayloadOwner>,
}
impl DlpPreparedAttempt {
    pub(crate) async fn guard_upstream_response(
        &mut self,
        response: crate::proxy::hyper_client::ProxyResponse,
    ) -> Result<crate::proxy::hyper_client::ProxyResponse, DlpForwardError> {
        if !self.has_restoration_lease() {
            return Ok(response);
        }
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
        if !self.has_restoration_lease() {
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DlpForwardError {
    ContextUnavailable,
    PolicyConflict,
    RestoreUnsupported,
    RedactionFailed,
    Unavailable,
    CapacityExceeded,
    TokenCollision,
    Blocked,
    RestoreFailed,
}

impl DlpForwardError {
    pub(super) fn from_token_error(err: TokenError) -> Self {
        match err {
            TokenError::Unavailable => Self::Unavailable,
            TokenError::ContextUnavailable => Self::ContextUnavailable,
            TokenError::CapacityExceeded => Self::CapacityExceeded,
            TokenError::TokenCollision => Self::TokenCollision,
            TokenError::RedactionFailed => Self::RedactionFailed,
        }
    }

    pub(super) fn from_plan_error(err: PlanError) -> Self {
        match err {
            PlanError::Blocked => Self::Blocked,
            PlanError::PolicyConflict => Self::PolicyConflict,
            PlanError::Unsupported => Self::RestoreUnsupported,
            PlanError::Token(te) => Self::from_token_error(te),
        }
    }
    pub(crate) fn status_code(self) -> StatusCode {
        match self {
            Self::RestoreFailed => StatusCode::BAD_GATEWAY,
            Self::ContextUnavailable
            | Self::PolicyConflict
            | Self::RestoreUnsupported
            | Self::RedactionFailed
            | Self::Blocked => StatusCode::BAD_REQUEST,
            Self::Unavailable | Self::CapacityExceeded | Self::TokenCollision => {
                StatusCode::SERVICE_UNAVAILABLE
            }
        }
    }

    pub(crate) fn error_type(self) -> &'static str {
        match self {
            Self::RestoreFailed => "server_error",
            Self::ContextUnavailable
            | Self::PolicyConflict
            | Self::RestoreUnsupported
            | Self::RedactionFailed
            | Self::Blocked => "invalid_request_error",
            Self::Unavailable | Self::CapacityExceeded | Self::TokenCollision => "server_error",
        }
    }

    pub(crate) fn code(self) -> &'static str {
        match self {
            Self::RestoreFailed => "dlp_restore_failed",
            Self::ContextUnavailable => "dlp_context_unavailable",
            Self::PolicyConflict => "dlp_policy_conflict",
            Self::RestoreUnsupported => "dlp_restore_unsupported",
            Self::RedactionFailed => "dlp_redaction_failed",
            Self::Unavailable => "dlp_unavailable",
            Self::CapacityExceeded => "dlp_capacity_exceeded",
            Self::TokenCollision => "dlp_token_collision",
            Self::Blocked => "dlp_blocked",
        }
    }

    pub(crate) fn message(self) -> &'static str {
        match self {
            Self::RestoreFailed => "Reversible DLP response restoration failed.",
            Self::ContextUnavailable => "Reversible DLP session or cursor context is unavailable.",
            Self::PolicyConflict => {
                "Reversible DLP token conflicts with an irreversible security rule."
            }
            Self::RestoreUnsupported => {
                "Reversible DLP tokenization is not supported for this endpoint or carrier."
            }
            Self::RedactionFailed => "Reversible DLP request preparation failed.",
            Self::Unavailable => "Reversible DLP service is temporarily unavailable.",
            Self::CapacityExceeded => "Reversible DLP capacity limit exceeded.",
            Self::TokenCollision => "Reversible DLP token digest collision detected.",
            Self::Blocked => "Request blocked by local security policy.",
        }
    }

    pub(crate) fn into_codex_response(self) -> Response {
        self.into_app_response(&AppType::Codex)
    }

    pub(crate) fn into_app_response(self, app_type: &AppType) -> Response {
        let status = self.status_code();
        let code = self.code();
        let message = self.message();
        let error_type = self.error_type();
        let payload = match app_type {
            AppType::Claude | AppType::ClaudeDesktop => json!({
                "type": "error",
                "error": {
                    "type": if status == StatusCode::BAD_REQUEST { "invalid_request_error" } else { "api_error" },
                    "message": code,
                    "code": code,
                }
            }),
            AppType::Gemini => json!({
                "error": {
                    "code": status.as_u16(),
                    "message": code,
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
            b"{\"error\":{\"message\":\"DLP error\",\"type\":\"server_error\",\"code\":\"dlp_unavailable\",\"param\":null}}".to_vec()
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
    message: &'static str,
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
        b"{\"error\":{\"message\":\"DLP error\",\"type\":\"server_error\",\"code\":\"dlp_unavailable\",\"param\":null}}".to_vec()
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
    build_codex_json_error(
        StatusCode::BAD_GATEWAY,
        err.message(),
        "server_error",
        err.code(),
    )
}

fn body_contains_any_token_reference(body: &Value) -> Result<bool, TokenError> {
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
                if body_contains_any_token_reference(item)? {
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
                if found || body_contains_any_token_reference(v)? {
                    return Ok(true);
                }
            }
            Ok(false)
        }
        _ => Ok(false),
    }
}

/// Allocation ownership from cached history cannot authorize token dependencies.
/// A request without a captured restoration lease must refuse late references.
pub(crate) fn validate_unleased_enriched_body(body: &Value) -> Result<(), DlpForwardError> {
    if body_contains_any_token_reference(body).map_err(DlpForwardError::from_token_error)? {
        return Err(DlpForwardError::ContextUnavailable);
    }
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
                && found.rule_id != "catalog:generic-sk-key-notice"
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
            push_unique_payload_text(tokens, &s[range], 31, budget)
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
                    push_unique_payload_text(tokens, &k[range], 31, budget)
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

    collect_tokens_from_value(body, &mut referenced_tokens, budget)?;

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

pub(crate) fn admit_http_request(
    gateway: &RestorationGateway,
    app_type: &AppType,
    endpoint_supported_codex_responses: bool,
    endpoint_is_compact: bool,
    body: &mut Value,
    headers: &HeaderMap,
) -> Result<RestorationAdmission, crate::proxy::error::ProxyError> {
    let is_supported_codex =
        matches!(app_type, AppType::Codex) && endpoint_supported_codex_responses;
    let has_tokens = match body_contains_any_token_reference(body) {
        Ok(b) => b,
        Err(err) => {
            let fwd_err = if is_supported_codex {
                DlpForwardError::from_token_error(err)
            } else {
                DlpForwardError::RestoreUnsupported
            };
            return Ok(RestorationAdmission::Intercepted(
                fwd_err.into_app_response(app_type),
            ));
        }
    };

    let gw_state = match gateway.state() {
        Ok(Some(state)) => state,
        Ok(None) => {
            if has_tokens {
                let fwd_err = if is_supported_codex {
                    DlpForwardError::ContextUnavailable
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
                DlpForwardError::from_token_error(err).into_app_response(app_type),
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

    let dlp_active = snapshot.runtime.restoration_enabled()
        || (snapshot.runtime.enabled && !rule_refs.as_slice().is_empty());
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
    let refusal_audit =
        PendingRequestAudit::capture_refusal(body, &snapshot.runtime, budget.clone()).ok();
    let result = (|| {
        if !snapshot.runtime.enabled && has_tokens {
            let fwd_err = if is_supported_codex {
                DlpForwardError::ContextUnavailable
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
                return Ok(RestorationAdmission::Intercepted(
                    DlpForwardError::RestoreUnsupported.into_app_response(app_type),
                ));
            }
            return match inspect_and_intercept_mut_with_headers(app_type, body, headers)? {
                Some(resp) => Ok(RestorationAdmission::Intercepted(resp)),
                None => Ok(RestorationAdmission::Passthrough),
            };
        }

        let session_id = extract_codex_restoration_session_id(headers, body);
        let mut base_lease =
            match gw_state
                .store
                .admit("codex", session_id, snapshot.restoration_policy_revision)
            {
                Ok(l) => l,
                Err(err) => {
                    return Ok(RestorationAdmission::Intercepted(
                        DlpForwardError::from_token_error(err).into_app_response(app_type),
                    ));
                }
            };

        let history = match extract_request_history(body, &budget) {
            Ok(h) => h,
            Err(err) => {
                return Ok(RestorationAdmission::Intercepted(
                    DlpForwardError::from_token_error(err).into_app_response(app_type),
                ));
            }
        };

        let fresh_readable_history = super::request_preparation::is_fresh_readable_history(
            body, &budget,
        )
        .map_err(|error| {
            crate::proxy::error::ProxyError::DlpForward(DlpForwardError::from_token_error(error))
        })?;
        let needs_provider_resume = !fresh_readable_history
            && (history.previous_response_id.is_some()
                || !history.calls.as_slice().is_empty()
                || !history.opaques.as_slice().is_empty()
                || !history.orphan_output_call_ids.is_slice_empty_helper()
                || !history.referenced_tokens.as_slice().is_empty());

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
            match PendingRequestAudit::capture(
                source_body.as_value(),
                &snapshot.runtime,
                rule_refs.as_slice(),
                &base_lease,
                budget.clone(),
            ) {
                Ok(summary) => summary,
                Err(err) => {
                    return Ok(RestorationAdmission::Intercepted(
                        DlpForwardError::from_plan_error(err).into_app_response(app_type),
                    ))
                }
            },
        );

        if !needs_provider_resume && !fresh_readable_history {
            match prepare_codex_request_with_budget(
                &mut base_lease,
                source_body.as_value(),
                &snapshot.runtime,
                rule_refs.as_slice(),
                &[],
                budget.clone(),
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
                        base_lease,
                        runtime: (*snapshot.runtime).clone(),
                        restore_rules: rules_copy,
                        budget,
                        source_body: pretokenized_body,
                        pretokenized: true,
                        endpoint_is_compact,
                        audit_state: std::sync::Arc::clone(&audit_state),
                        initial_audit,
                    })));
                }
                Err(plan_err) => {
                    return Ok(RestorationAdmission::Intercepted(
                        DlpForwardError::from_plan_error(plan_err).into_app_response(app_type),
                    ));
                }
            }
        }

        Ok(RestorationAdmission::Admitted(Box::new(DlpHttpRequest {
            base_lease,
            runtime: (*snapshot.runtime).clone(),
            restore_rules: rules_copy,
            budget,
            source_body,
            pretokenized: false,
            endpoint_is_compact,
            audit_state: std::sync::Arc::clone(&audit_state),
            initial_audit,
        })))
    })();
    if matches!(&result, Ok(RestorationAdmission::Intercepted(_))) {
        if let Some(pending) = &refusal_audit {
            audit_state.record_refusal(pending, "", None);
        }
    }
    result
}

trait SliceEmptyHelper<T> {
    fn is_slice_empty_helper(&self) -> bool;
}

impl<T> SliceEmptyHelper<T> for PayloadVec<T> {
    fn is_slice_empty_helper(&self) -> bool {
        self.as_slice().is_empty()
    }
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
    pub(crate) fn build_response_namespace_table(
        &self,
        body: &Value,
    ) -> Result<Option<DlpResponseNamespaceTable>, DlpForwardError> {
        if !(self.runtime.restoration_enabled()
            || (self.runtime.enabled && !self.restore_rules.as_slice().is_empty())
            || !self.base_lease.is_empty())
        {
            return Ok(None);
        }
        #[cfg(test)]
        response_namespace_test_support::before_build(&self.budget)?;
        let result = DlpResponseNamespaceTable::build(body, self.budget.clone())
            .map(Some)
            .map_err(DlpForwardError::from_token_error);
        #[cfg(test)]
        response_namespace_test_support::after_build(result.as_ref().err().copied());
        result
    }

    pub(crate) fn chat_history_provenance(
        &self,
    ) -> Result<super::chat_enrichment::ChatHistoryProvenance, DlpForwardError> {
        let independent = super::request_preparation::is_fresh_readable_history(
            self.source_body.as_value(),
            &self.budget,
        )
        .map_err(DlpForwardError::from_token_error)?;
        Ok(if independent {
            super::chat_enrichment::ChatHistoryProvenance::IndependentReadable
        } else {
            super::chat_enrichment::ChatHistoryProvenance::StrictResume
        })
    }

    pub(crate) fn chat_enrichment_budget(
        &self,
    ) -> Option<super::chat_enrichment::ChatEnrichmentBudget> {
        (self.runtime.restoration_enabled()
            || (self.runtime.enabled && !self.restore_rules.as_slice().is_empty())
            || !self.base_lease.is_empty())
        .then(|| super::chat_enrichment::ChatEnrichmentBudget::new(self.budget.clone()))
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
            self.budget.clone(),
            self.budget.limit(),
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
                self.budget.clone(),
                self.budget.limit(),
            ),
            None => PayloadJson::try_from_value(body, self.budget.clone(), self.budget.limit()),
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

    pub(crate) fn build_namespace_body(
        &self,
        body: &Value,
    ) -> Result<Option<(Value, DlpNamespaceOwner)>, DlpForwardError> {
        let Some(output) =
            PayloadJson::try_namespace_body(body, self.budget.clone(), self.budget.limit())
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
            self.budget.clone(),
            self.budget.limit(),
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
        let output =
            PayloadJson::try_outbound_tail(body, patch, self.budget.clone(), self.budget.limit())
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
        F: FnOnce(Value) -> Result<Value, DlpForwardError>,
    {
        let refusal_audit =
            PendingRequestAudit::capture_refusal(enriched_body, &self.runtime, self.budget.clone())
                .ok();
        let result = (|| {
            let mut attempt_lease = self
                .base_lease
                .fork_attempt()
                .map_err(DlpForwardError::from_token_error)?;
            let binding = attempt_lease
                .bind_actual_operation_attempt(
                    provider_id,
                    protocol,
                    target_url,
                    auth_principal,
                    operation,
                )
                .map_err(DlpForwardError::from_token_error)?;

            let history = extract_request_history(enriched_body, &self.budget)
                .map_err(DlpForwardError::from_token_error)?;

            let mut call_specs =
                PayloadVec::new(self.budget.clone()).map_err(DlpForwardError::from_token_error)?;
            for c in history.calls.as_slice() {
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
            let mut opaque_refs =
                PayloadVec::new(self.budget.clone()).map_err(DlpForwardError::from_token_error)?;
            for o in history.opaques.as_slice() {
                opaque_refs
                    .push(o.as_str())
                    .map_err(DlpForwardError::from_token_error)?;
            }
            let mut orphan_refs =
                PayloadVec::new(self.budget.clone()).map_err(DlpForwardError::from_token_error)?;
            for cid in history.orphan_output_call_ids.as_slice() {
                orphan_refs
                    .push(cid.as_str())
                    .map_err(DlpForwardError::from_token_error)?;
            }

            if !matches!(
                self.chat_history_provenance()?,
                super::chat_enrichment::ChatHistoryProvenance::IndependentReadable
            ) {
                attempt_lease
                    .resume_completed_context(
                        &binding,
                        history
                            .previous_response_id
                            .as_ref()
                            .map(PayloadText::as_str),
                        call_specs.as_slice(),
                        opaque_refs.as_slice(),
                    )
                    .map_err(DlpForwardError::from_token_error)?;

                attempt_lease
                    .verify_output_call_ids(
                        &binding,
                        history
                            .previous_response_id
                            .as_ref()
                            .map(PayloadText::as_str),
                        orphan_refs.as_slice(),
                    )
                    .map_err(DlpForwardError::from_token_error)?;
            }

            let mut token_refs =
                PayloadVec::new(self.budget.clone()).map_err(DlpForwardError::from_token_error)?;
            for t in history.referenced_tokens.as_slice() {
                token_refs
                    .push(t.as_str())
                    .map_err(DlpForwardError::from_token_error)?;
            }
            attempt_lease
                .resume_completed_tokens(&binding, token_refs.as_slice())
                .map_err(DlpForwardError::from_token_error)?;

            let mut rule_refs =
                PayloadVec::new(self.budget.clone()).map_err(DlpForwardError::from_token_error)?;
            for r in self.restore_rules.as_slice() {
                rule_refs
                    .push(r.as_str())
                    .map_err(DlpForwardError::from_token_error)?;
            }

            let pending_audit = PendingRequestAudit::capture(
                enriched_body,
                &self.runtime,
                rule_refs.as_slice(),
                &attempt_lease,
                self.budget.clone(),
            )
            .map_err(DlpForwardError::from_plan_error)?;
            let prepared = prepare_codex_request_with_budget(
                &mut attempt_lease,
                enriched_body,
                &self.runtime,
                rule_refs.as_slice(),
                token_refs.as_slice(),
                self.budget.clone(),
                |candidate_val, _encoded, proof| {
                    validate_non_carrier_tokens(candidate_val, proof)?;
                    Ok(())
                },
            )
            .map_err(DlpForwardError::from_plan_error)?;

            #[cfg(test)]
            transform_input_test_support::prepared(prepared.body.as_value());
            // Declare the reservation owner before every moved form of its tree.
            // Retaining it does not authorize allocations made by the callback.
            let input_owner;
            let prepared_body = {
                let (body, reservation) = prepared.body.into_parts();
                input_owner = DlpTransformInputOwner {
                    _reservation: reservation,
                };
                body
            };
            let final_body = transform_and_override(prepared_body)?;
            validate_final_outbound_body(
                &final_body,
                protocol,
                &attempt_lease,
                &self.runtime,
                &self.budget,
                &self.audit_state,
                provider_id,
            )?;
            *enriched_body = final_body;

            Ok((
                DlpPreparedAttempt {
                    lease: attempt_lease,
                    binding,
                    budget: self.budget.clone(),
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
                    json_completion: std::sync::atomic::AtomicU8::new(0),
                    audit_state: std::sync::Arc::clone(&self.audit_state),
                    initial_audit: std::sync::Arc::clone(&self.initial_audit),
                    pending_audit,
                    upstream_payload_owner: None,
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
            |body| transform_and_override(&body),
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
        let mut proxy_err: Option<crate::proxy::error::ProxyError> = None;
        let res = self.prepare_owned_attempt(
            provider_id,
            protocol,
            target_url,
            auth_principal,
            operation,
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

fn final_inspection_carriers(
    body: &Value,
    protocol: &str,
    budget: &PayloadBudget,
    for_refusal_audit: bool,
) -> Result<CodexRequestCarriers, DlpForwardError> {
    let collect = |value: &Value| {
        if for_refusal_audit {
            CodexRequestCarriers::from_body_for_refusal_audit(value, budget.clone())
        } else {
            CodexRequestCarriers::from_body_with_budget(value, budget.clone())
        }
        .map_err(DlpForwardError::from_token_error)
    };
    if protocol == "openai_responses" {
        return collect(body);
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
        for message in messages {
            if let Some(content) = message.get("content") {
                collect_final_content(content, anthropic, &mut items)
                    .map_err(DlpForwardError::from_token_error)?;
            }
            if !anthropic {
                collect_final_metadata(message, &["name", "tool_call_id"], &mut items)
                    .map_err(DlpForwardError::from_token_error)?;
                for key in ["reasoning_content", "reasoning", "refusal"] {
                    if let Some(text) = message.get(key).and_then(Value::as_str) {
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
    collect(projection.as_value())
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
    carriers: &CodexRequestCarriers,
    runtime: &SecurityRuntimeConfig,
    budget: &PayloadBudget,
    audit_state: &RequestAuditState,
    provider_id: &str,
) -> Result<(), TokenError> {
    carriers.visit(|leaf| {
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
    })
}

fn validate_final_outbound_body(
    body: &Value,
    protocol: &str,
    lease: &DlpRequestLease,
    runtime: &SecurityRuntimeConfig,
    budget: &PayloadBudget,
    audit_state: &RequestAuditState,
    provider_id: &str,
) -> Result<(), DlpForwardError> {
    // Bound the actual tree before recursive token checks or borrowed projection traversal.
    let bounded_body = PayloadJson::try_from_value(body, budget.clone(), budget.limit())
        .map_err(DlpForwardError::from_token_error)?;
    let carriers = match final_inspection_carriers(bounded_body.as_value(), protocol, budget, false)
    {
        Ok(carriers) => carriers,
        Err(error) => {
            // Collect valid siblings for refusal evidence only; this path never authorizes a send.
            let audit = final_inspection_carriers(bounded_body.as_value(), protocol, budget, true)
                .and_then(|carriers| {
                    audit_final_refusal(&carriers, runtime, budget, audit_state, provider_id)
                        .map_err(DlpForwardError::from_token_error)
                });
            if audit.is_err() {
                log::warn!("[SecurityAudit] stage=final_refusal category=unavailable");
            }
            return Err(error);
        }
    };
    let result = (|| {
        visit_value_tokens(body, &mut |tok| lease.resolve(tok).map(|_| ()))
            .map_err(DlpForwardError::from_token_error)?;
        let mut refusal = None;
        carriers
            .visit(|leaf| {
                let mut refs = PayloadVec::new(budget.clone())?;
                visit_token_references(leaf.text, |range| {
                    lease.resolve(&leaf.text[range.clone()])?;
                    refs.push(range)
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
                        EffectiveAction::Redact | EffectiveAction::Restore | EffectiveAction::Block
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
            })
            .map_err(DlpForwardError::from_token_error)?;
        refusal.map_or(Ok(()), Err)
    })();
    if result.is_err() {
        // Auditing is auxiliary: an audit failure cannot turn a refusal into forwarding.
        if audit_final_refusal(&carriers, runtime, budget, audit_state, provider_id).is_err() {
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

    pub(crate) fn verify_upstream_json_completion(
        &self,
        protocol: JsonCompletionProtocol,
        raw_json: &str,
    ) -> Result<(), RestoreError> {
        use std::sync::atomic::Ordering;
        let matches_protocol = matches!(
            (self.upstream_json_protocol, protocol),
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
        completion.observe_raw_json(raw_json, &self.budget);
        if !matches_protocol
            || !completion.permits_completion()
            || self
                .json_completion
                .compare_exchange(0, 1, Ordering::Relaxed, Ordering::Relaxed)
                .is_err()
        {
            let error = if matches_protocol && completion.has_unsupported_carrier() {
                RestoreError::RestoreUnsupported
            } else {
                RestoreError::RestoreFailed
            };
            self.json_completion.store(
                if error == RestoreError::RestoreUnsupported {
                    3
                } else {
                    2
                },
                Ordering::Relaxed,
            );
            return Err(error);
        }
        Ok(())
    }

    #[cfg(test)]
    fn restore_json_response(
        self,
        status: u16,
        body: &Value,
        _norm_mode: ArgumentNormalizationMode,
        observation: Option<&CompletionObservation<'_>>,
    ) -> Result<super::response_restoration::RestoredJsonResponse, Response> {
        let witness = self
            .json_completion_witness(status, observation)
            .map_err(build_codex_restore_error_response)?;
        stage_json_response(body, self.endpoint_is_compact, &self.lease, &self.budget)
            .and_then(|staged| staged.publish(&self.lease, &self.binding, &witness, &self.budget))
            .map_err(build_codex_restore_error_response)
    }

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
        CompletionWitness::from_verified_completion(
            self.json_completion
                .load(std::sync::atomic::Ordering::Relaxed)
                == 1,
        )
        .map_err(|_| RestoreError::RestoreFailed)
    }

    /// Only the actual xAI SSE branch requests original-byte evidence.
    pub(crate) fn xai_original_sse_evidence_tracker(
        &self,
    ) -> Option<std::sync::Arc<std::sync::Mutex<UpstreamSseEvidenceTracker>>> {
        self.has_active_tokens().then(|| {
            std::sync::Arc::new(std::sync::Mutex::new(
                UpstreamSseEvidenceTracker::xai_original(self.budget.clone()),
            ))
        })
    }

    pub(crate) fn upstream_sse_evidence_tracker(
        &self,
    ) -> Option<std::sync::Arc<std::sync::Mutex<UpstreamSseEvidenceTracker>>> {
        if !self.has_active_tokens() {
            return None;
        }
        let tracker = match self.upstream_json_protocol {
            Some(JsonCompletionProtocol::ChatCompletions) => {
                UpstreamSseEvidenceTracker::chat(self.budget.clone())
            }
            Some(JsonCompletionProtocol::AnthropicMessages) => {
                UpstreamSseEvidenceTracker::anthropic(self.budget.clone())
            }
            _ => return None,
        };
        Some(std::sync::Arc::new(std::sync::Mutex::new(tracker)))
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
                self.json_completion
                    .store(2, std::sync::atomic::Ordering::Relaxed);
                return Err(RestoreError::RestoreFailed);
            }
        };
        let checked = (|| -> Result<(), RestoreError> {
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
                tracker.observe(&CompletionObservation::RawFrame(block));
                let mut data = PayloadBuffer::new(self.budget.clone(), 1024 * 1024)
                    .map_err(|_| RestoreError::RestoreFailed)?;
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
                            data.append(b"\n")
                                .map_err(|_| RestoreError::RestoreFailed)?;
                        }
                        count += 1;
                        data.append(value.strip_prefix(' ').unwrap_or(value).as_bytes())
                            .map_err(|_| RestoreError::RestoreFailed)?;
                    }
                }
                if count == 0 {
                    continue;
                }
                let raw = std::str::from_utf8(data.as_slice())
                    .map_err(|_| RestoreError::RestoreFailed)?;
                if raw.trim() == "[DONE]" {
                    tracker.observe(&CompletionObservation::ChatDone);
                } else {
                    let parsed =
                        PayloadJson::try_from_str(raw, self.budget.clone(), self.budget.limit())
                            .map_err(|_| RestoreError::RestoreFailed)?;
                    match protocol {
                        JsonCompletionProtocol::ChatCompletions => {
                            tracker.observe(&CompletionObservation::ChatChunk(parsed.as_value()))
                        }
                        JsonCompletionProtocol::AnthropicMessages => {
                            tracker.observe(&CompletionObservation::AnthropicEvent {
                                event_name: event,
                                data: parsed.as_value(),
                            })
                        }
                        _ => return Err(RestoreError::RestoreFailed),
                    }
                }
            }
            tracker.observe(&CompletionObservation::EntityEnd {
                reached_eof: true,
                unfinished_frame: frame_start != raw_sse.len(),
                unfinished_utf8: false,
            });
            tracker.completion_result()
        })();
        use std::sync::atomic::Ordering;
        if checked.is_err()
            || self
                .json_completion
                .compare_exchange(0, 1, Ordering::Relaxed, Ordering::Relaxed)
                .is_err()
        {
            let error = checked.err().unwrap_or(RestoreError::RestoreFailed);
            self.json_completion.store(
                if error == RestoreError::RestoreUnsupported {
                    3
                } else {
                    2
                },
                Ordering::Relaxed,
            );
            return Err(error);
        }
        Ok(())
    }

    pub(crate) async fn restore_client_http_response(
        self,
        response: Response,
        mode: ArgumentNormalizationMode,
        evidence: Option<std::sync::Arc<std::sync::Mutex<UpstreamSseEvidenceTracker>>>,
    ) -> Response {
        let upstream_owner = self.upstream_payload_owner();
        if !self.has_active_tokens() || !response.status().is_success() {
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
            return build_codex_restore_error_response(RestoreError::RestoreFailed);
        }
        clear_entity_headers(&mut parts.headers);
        if is_sse {
            let restorer = match self.into_sse_restorer(parts.status.as_u16(), mode) {
                Ok(restorer) => restorer,
                Err(error) => return build_codex_restore_error_response(error),
            };
            let stream = body
                .into_data_stream()
                .map(|chunk| chunk.map_err(|_| std::io::Error::other("Response stream failed.")));
            return Response::from_parts(
                parts,
                Body::from_stream(retain_stream_owner(
                    wrap_sse_stream_with_restoration_and_evidence(stream, restorer, evidence),
                    upstream_owner,
                )),
            );
        }
        let budget = self.budget.clone();
        let result = async {
            let mut input = PayloadBuffer::new(
                budget.clone(),
                crate::proxy::hyper_client::MAX_RESPONSE_BODY_BYTES,
            )
            .map_err(|_| RestoreError::RestoreFailed)?;
            let mut stream = body.into_data_stream();
            while let Some(chunk) = stream.next().await {
                let chunk = chunk.map_err(|_| RestoreError::RestoreFailed)?;
                input
                    .append(&chunk)
                    .map_err(|_| RestoreError::RestoreFailed)?;
            }
            let raw =
                std::str::from_utf8(input.as_slice()).map_err(|_| RestoreError::RestoreFailed)?;
            if self
                .json_completion
                .load(std::sync::atomic::Ordering::Relaxed)
                == 0
            {
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
            let parsed = PayloadJson::try_from_str(raw, budget.clone(), budget.limit())
                .map_err(|_| RestoreError::RestoreFailed)?;
            let witness = self.json_completion_witness(parts.status.as_u16(), None)?;
            let staged = stage_json_response(
                parsed.as_value(),
                self.endpoint_is_compact,
                &self.lease,
                &budget,
            )?;
            let output = super::response_restoration::serialize_json_with_budget(
                staged.as_value(),
                &budget,
                crate::proxy::hyper_client::MAX_RESPONSE_BODY_BYTES,
            )?;
            // The final bytes own their reservation before continuity can be published.
            let bytes = bytes_with_owner(output, upstream_owner);
            let _restored = staged.publish(&self.lease, &self.binding, &witness, &budget)?;
            Ok::<_, RestoreError>(bytes)
        }
        .await;
        match result {
            Ok(bytes) => {
                if let Ok(length) = HeaderValue::from_str(&bytes.len().to_string()) {
                    parts.headers.insert(header::CONTENT_LENGTH, length);
                }
                Response::from_parts(parts, Body::from(bytes))
            }
            Err(error) => build_codex_restore_error_response(error),
        }
    }

    pub(super) fn into_sse_restorer(
        self,
        status: u16,
        norm_mode: ArgumentNormalizationMode,
    ) -> Result<SseRestorationState, RestoreError> {
        if status != 200 || self.endpoint_is_compact {
            return Err(RestoreError::RestoreFailed);
        }
        SseRestorationState::new(self.lease, self.binding, self.budget, norm_mode)
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

#[derive(Clone, Copy)]
enum XaiOriginalCompletion {
    Pending,
    Verified,
    Failed,
}

pub(crate) struct UpstreamSseEvidenceTracker {
    xai_original: Option<XaiOriginalCompletion>,
    xai_original_output: Option<PayloadJson>,
    sse: super::completion_evidence::SseCompletion,
    json_fallback: Option<JsonCompletion>,
    observations_seen: bool,
    transport: super::completion_evidence::CompletionTransport,
    budget: PayloadBudget,
}

impl UpstreamSseEvidenceTracker {
    pub(super) fn chat(budget: PayloadBudget) -> Self {
        Self {
            xai_original: None,
            xai_original_output: None,
            sse: super::completion_evidence::SseCompletion::chat(),
            json_fallback: None,
            observations_seen: false,
            transport: super::completion_evidence::CompletionTransport::default(),
            budget,
        }
    }

    pub(super) fn anthropic(budget: PayloadBudget) -> Self {
        Self {
            xai_original: None,
            xai_original_output: None,
            sse: super::completion_evidence::SseCompletion::anthropic(),
            json_fallback: Some(JsonCompletion::new(
                JsonCompletionProtocol::AnthropicMessages,
            )),
            observations_seen: false,
            transport: super::completion_evidence::CompletionTransport::default(),
            budget,
        }
    }

    fn xai_original(budget: PayloadBudget) -> Self {
        let mut tracker = Self::chat(budget);
        tracker.xai_original = Some(XaiOriginalCompletion::Pending);
        tracker
    }

    fn finish_xai_original(&mut self, verified: bool) {
        self.xai_original = Some(match (self.xai_original, verified) {
            (Some(XaiOriginalCompletion::Pending), true) => XaiOriginalCompletion::Verified,
            _ => XaiOriginalCompletion::Failed,
        });
    }

    /// Called only after the handler fully reads an original explicit JSON entity.
    /// This records JSON completion separately and never invents an SSE frame or EOF.
    pub(crate) fn observe_fully_read_anthropic_json(&mut self, raw: &str) {
        if self.observations_seen || self.xai_original.is_some() || self.json_fallback.is_none() {
            self.transport.abort();
            if self.xai_original.is_some() {
                self.finish_xai_original(false);
            }
            return;
        }
        self.observations_seen = true;
        if let Some(json) = &mut self.json_fallback {
            json.observe_raw_json(raw, &self.budget);
        }
        self.transport.observe_fully_read_json();
    }

    pub(crate) fn observe(&mut self, fact: &CompletionObservation<'_>) {
        self.observations_seen = true;
        if self.xai_original.is_some() {
            self.finish_xai_original(false);
            return;
        }
        self.transport.observe(fact);
        match fact {
            CompletionObservation::RawJson(raw) => {
                if let Some(jf) = &mut self.json_fallback {
                    jf.observe_raw_json(raw, &self.budget);
                } else {
                    self.sse.observe(fact, &self.budget);
                }
            }
            CompletionObservation::AnthropicJson(_) => {
                if self.json_fallback.is_none() {
                    self.sse.observe(fact, &self.budget);
                }
            }
            _ => {
                self.sse.observe(fact, &self.budget);
            }
        }
    }

    fn has_unsupported_carrier(&self) -> bool {
        if self.xai_original.is_some() {
            return false;
        }
        self.sse.has_unsupported_carrier()
            || self
                .json_fallback
                .as_ref()
                .is_some_and(JsonCompletion::has_unsupported_carrier)
    }

    fn completion_result(&self) -> Result<(), RestoreError> {
        if self.has_unsupported_carrier() {
            Err(RestoreError::RestoreUnsupported)
        } else if self.permits_eof() {
            Ok(())
        } else {
            Err(RestoreError::RestoreFailed)
        }
    }

    fn permits_terminal(&self) -> bool {
        if let Some(state) = self.xai_original {
            return matches!(state, XaiOriginalCompletion::Verified);
        }
        if self.transport.legacy_failed() {
            return false;
        }
        self.sse.permits_completion()
            || self
                .json_fallback
                .as_ref()
                .is_some_and(JsonCompletion::permits_completion)
    }

    fn permits_eof(&self) -> bool {
        if let Some(state) = self.xai_original {
            return matches!(state, XaiOriginalCompletion::Verified);
        }
        if !self.transport.permits_completion() {
            return false;
        }
        self.sse.permits_completion()
            || self
                .json_fallback
                .as_ref()
                .is_some_and(JsonCompletion::permits_completion)
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
            Ok(tracker) => Some(tracker.budget.clone()),
            Err(_) => None,
        };
        let Some(budget) = budget else {
            yield Err(std::io::Error::other("Response stream failed."));
            return;
        };
        let mut native = super::completion_json::NativeSseCompletion::new();
        let mut original_arguments = match super::response_restoration::OriginalXaiArguments::new(budget.clone()) {
            Ok(arguments) => arguments,
            Err(_) => {
                if let Ok(mut tracker) = evidence.lock() { tracker.finish_xai_original(false); }
                yield Err(std::io::Error::other("Response stream failed."));
                return;
            }
        };
        let mut transport = super::completion_evidence::CompletionTransport::default();
        let mut frame = match PayloadBuffer::new(budget.clone(), 1024 * 1024) {
            Ok(frame) => frame,
            Err(_) => {
                if let Ok(mut tracker) = evidence.lock() { tracker.finish_xai_original(false); }
                yield Err(std::io::Error::other("Response stream failed."));
                return;
            }
        };
        let mut line_start = 0;
        while let Some(chunk) = stream.next().await {
            let chunk = match chunk {
                Ok(chunk) => chunk,
                Err(_) => {
                    if let Ok(mut tracker) = evidence.lock() { tracker.finish_xai_original(false); }
                    yield Err(std::io::Error::other("Response stream failed."));
                    return;
                }
            };
            let mut offset = 0;
            let checked = (|| -> Result<(), RestoreError> {
                while offset < chunk.len() {
                    let end = chunk[offset..].iter().position(|byte| *byte == b'\n')
                        .map_or(chunk.len(), |pos| offset + pos + 1);
                    frame.append(&chunk[offset..end]).map_err(|_| RestoreError::RestoreFailed)?;
                    offset = end;
                    if chunk[end - 1] != b'\n' { continue; }
                    let line = &frame.as_slice()[line_start..];
                    let blank = line == b"\n" || line == b"\r\n";
                    line_start = frame.len();
                    if !blank { continue; }
                    let raw = std::str::from_utf8(frame.as_slice()).map_err(|_| RestoreError::RestoreFailed)?;
                    observe_xai_original_frame(raw, &mut native, &mut original_arguments, &budget)?;
                    frame.clear();
                    line_start = 0;
                }
                Ok(())
            })();
            if checked.is_err() {
                if let Ok(mut tracker) = evidence.lock() { tracker.finish_xai_original(false); }
                yield Err(std::io::Error::other("Response stream failed."));
                return;
            }
            yield Ok(chunk);
        }
        let end = CompletionObservation::EntityEnd {
            reached_eof: true,
            unfinished_frame: frame.len() != 0,
            unfinished_utf8: std::str::from_utf8(frame.as_slice()).is_err(),
        };
        native.observe(&end, &budget);
        transport.observe(&end);
        let verified = native.permits_completion() && transport.permits_completion();
        let original_output = if verified { original_arguments.take_completed_output().ok() } else { None };
        if let Ok(mut tracker) = evidence.lock() {
            tracker.finish_xai_original(verified && original_output.is_some());
            if matches!(tracker.xai_original, Some(XaiOriginalCompletion::Verified)) {
                tracker.xai_original_output = original_output;
            }
        };
        // Dropping/cancelling this stream never synthesizes an EOF or authority.
    })
}

fn observe_xai_original_frame(
    raw: &str,
    native: &mut super::completion_json::NativeSseCompletion,
    original_arguments: &mut super::response_restoration::OriginalXaiArguments,
    budget: &PayloadBudget,
) -> Result<(), RestoreError> {
    native.observe(&CompletionObservation::RawFrame(raw), budget);
    let mut data =
        PayloadBuffer::new(budget.clone(), 1024 * 1024).map_err(|_| RestoreError::RestoreFailed)?;
    let mut event = "";
    let mut count = 0;
    for line in raw.lines() {
        if let Some(value) = line.strip_prefix("event:") {
            event = value.trim();
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
        let raw_json =
            std::str::from_utf8(data.as_slice()).map_err(|_| RestoreError::RestoreFailed)?;
        if raw_json.trim() != "[DONE]" {
            let parsed = PayloadJson::try_from_str(raw_json, budget.clone(), budget.limit())
                .map_err(|_| RestoreError::RestoreFailed)?;
            original_arguments.observe(parsed.as_value())?;
            native.observe(
                &CompletionObservation::NativeEvent {
                    event_name: event,
                    data: parsed.as_value(),
                },
                budget,
            );
        }
    }
    Ok(())
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
                yield Ok(failed_sse_frame(RestoreError::RestoreFailed));
                return;
            }
        };
        let mut line_start = 0usize;
        while let Some(chunk_res) = upstream_stream.next().await {
            let chunk = match chunk_res {
                Ok(chunk) => chunk,
                Err(_) => {
                    yield Ok(failed_sse_frame(RestoreError::RestoreFailed));
                    return;
                }
            };
            let mut offset = 0usize;
            while offset < chunk.len() {
                let end = chunk[offset..].iter().position(|&byte| byte == b'\n')
                    .map_or(chunk.len(), |pos| offset + pos + 1);
                if frame.append(&chunk[offset..end]).is_err() {
                    yield Ok(failed_sse_frame(RestoreError::RestoreFailed));
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
                        yield Ok(failed_sse_frame(RestoreError::RestoreFailed));
                        return;
                    }
                };
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
                        yield Ok(failed_sse_frame(error));
                        return;
                    }
                }
            }
        }
        // EOF is not an SSE delimiter and cannot authorize an unfinished event.
        if frame.len() != 0 {
            yield Ok(failed_sse_frame(RestoreError::RestoreFailed));
            return;
        }
        if let Some(ref evidence) = evidence {
            let checked = evidence.lock().map_err(|_| RestoreError::RestoreFailed)
                .and_then(|guard| {
                    guard.completion_result()?;
                    if guard.xai_original.is_some() {
                        let original = guard.xai_original_output.as_ref().ok_or(RestoreError::RestoreFailed)?;
                        restorer.verify_original_xai_output(original.as_value())?;
                    }
                    Ok(())
                });
            if let Err(error) = checked {
                yield Ok(failed_sse_frame(error));
                return;
            }
        }
        match restorer.finalize_and_flush(true) {
            Ok(frames) => {
                for frame in frames {
                    yield Ok(frame);
                }
            }
            Err(err) => yield Ok(failed_sse_frame(err)),
        }
    })
}

#[cfg(test)]
#[path = "json_restoration_tests.rs"]
mod json_restoration_tests;

#[cfg(test)]
#[path = "final_validation_tests.rs"]
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
            base_lease: store.admit("codex", None, 1).expect("synthetic lease"),
            runtime: SecurityRuntimeConfig::default(),
            restore_rules: StoreVec::new(store.allocation_ledger().expect("ledger"), 4096),
            source_body: PayloadJson::try_from_value(&request_body, budget.clone(), budget.limit())
                .expect("synthetic source"),
            pretokenized: false,
            endpoint_is_compact: false,
            audit_state: std::sync::Arc::new(
                RequestAuditState::new(&AppType::Codex, None, budget.clone())
                    .expect("synthetic audit"),
            ),
            initial_audit: std::sync::Arc::new(
                PendingRequestAudit::empty(budget.clone()).expect("synthetic summary"),
            ),
            budget,
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
                                .base_lease
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
        assert_eq!(budget.used(), baseline + charge);
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
        assert_eq!(budget.used(), baseline + 2 * charge);
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
            base_lease: store.admit("codex", session, 1).expect("admit fixture"),
            runtime: SecurityRuntimeConfig::default(),
            restore_rules: StoreVec::new(store.allocation_ledger().expect("ledger"), 4096),
            budget: budget.clone(),
            source_body: PayloadJson::try_from_value(
                &request_body,
                budget.clone(),
                4 * 1024 * 1024,
            )
            .expect("source fixture"),
            pretokenized: false,
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
                &mut completed_response(),
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
                &mut completed_response(),
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
                &mut completed_response(),
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
                &mut completed_response(),
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
                &mut completed_response(),
                ArgumentNormalizationMode::Exact,
                None
            )
            .is_ok());
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
        assert_eq!(budget.used(), baseline, "cancel releases frame reservation");
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
            .contains("dlp_restore_failed"));
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
                .contains("dlp_restore_failed"),
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
            json_completion: std::sync::atomic::AtomicU8::new(0),
            audit_state: std::sync::Arc::new(
                RequestAuditState::new(&AppType::Codex, None, budget.clone())
                    .expect("audit state fixture"),
            ),
            initial_audit: std::sync::Arc::new(
                PendingRequestAudit::empty(budget.clone()).expect("audit summary fixture"),
            ),
            upstream_payload_owner: None,
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
        let mut runtime = SecurityRuntimeConfig::default();
        runtime.builtin_action_overrides.insert(
            "chinese_phone_number".into(),
            super::super::types::AuditAction::RedactRestore,
        );
        super::super::runtime::set_security_runtime_for_test(runtime.clone());
        let gateway = RestorationGateway::new_default();
        let mut body = json!({"input":"13800138000"});
        let admission = admit_http_request(
            &gateway,
            &AppType::Codex,
            true,
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
            .is_some_and(|text| text.starts_with("vary_ph_")));
        let mut unsupported = json!({"messages":[{"role":"user","content":"13800138000"}]});
        let admission = admit_http_request(
            &gateway,
            &AppType::Codex,
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
                &mut ordinary,
                &HeaderMap::new()
            )
            .expect("ordinary"),
            RestorationAdmission::Passthrough
        ));
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
