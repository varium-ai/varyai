//! Post-consumer Codex response restoration, delta/snapshot consistency, and tool commit barrier.
//!
//! No `unwrap()`/`expect()` is permitted here. All payload copies and event buffers
//! reserve capacity from `PayloadBudget` before growth, and completed-context
//! publication requires consuming verifier evidence and a matching delivery continuation.
use std::{
    borrow::Cow,
    io::{self, Write},
};

use bytes::Bytes;
use serde_json::{json, Value};

use super::{
    completed_context::{valid_external_id, ActualAttemptBinding, CompletedCallSpec},
    completion_json::permits_chat_argument_normalization,
    payload_budget::PayloadBudget,
    payload_buffer::PayloadBuffer,
    payload_json::{JsonStringReplacement, PayloadJson},
    payload_metadata::PayloadVec,
    payload_text::PayloadText,
    restoration_contract::{ProtocolPair, RestoreDecision, TextPolicy, UpstreamProtocol},
    restoration_core::{FinishedInspection, LifecycleLimits, ResponseLifecycle, TextStreamId},
    restore_diagnostics::{RestoreReason, RestoreStage},
    token_format_diagnostics::{Carrier, Site, TokenFormatDiagnosticState},
    tokenization::{DlpRequestLease, TokenError},
};

#[cfg(test)]
use super::{completed_context::CompletionWitness, restoration_contract::ClientProtocol};

#[path = "response_restoration/finalization.rs"]
mod finalization;
// Frozen native entry points remain available while their production lanes are disabled.
#[allow(unused_imports)]
pub(super) use finalization::{
    finalize_native_json, finalize_native_sse, DeliveryResume, FinalizedNativeJson,
    FinalizedNativeSse, NativePublicationSpec, NativeSseFinalizationKey, PublicationSpec,
    ReleaseIdentity,
};

const MAX_FRAME_BYTES: usize = 1024 * 1024;
const MAX_ARGUMENT_BYTES: usize = 1024 * 1024;
const MAX_BARRIER_BYTES: usize = 8 * 1024 * 1024;
const MAX_BARRIER_TOOLS: usize = 128;
const MAX_RESPONSE_ITEMS: usize = 4096;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DlpRestoreOutcome {
    NotRequired,
    InProgress,
    Completed { restored_occurrences: usize },
    Failed { code: &'static str },
    Cancelled,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RestoreError {
    RestoreFailed,
    FailedBecause(RestoreReason),
    RestoreUnsupported,
}

impl RestoreError {
    pub(crate) fn code(self) -> &'static str {
        match self {
            Self::RestoreFailed | Self::FailedBecause(_) => "dlp_restore_failed",
            Self::RestoreUnsupported => "dlp_restore_unsupported",
        }
    }

    pub(crate) fn message(self) -> &'static str {
        match self {
            Self::RestoreFailed | Self::FailedBecause(_) => RestoreStage::Unknown.message(),
            Self::RestoreUnsupported => "Reversible DLP response carrier is unsupported.",
        }
    }

    pub(super) fn reason_at(self, stage: RestoreStage) -> Option<RestoreReason> {
        match self {
            Self::FailedBecause(reason) => Some(reason),
            Self::RestoreFailed if stage == RestoreStage::TokenRestore => {
                Some(RestoreReason::Unknown)
            }
            _ => None,
        }
    }

    pub(super) fn message_at(self, stage: RestoreStage) -> Cow<'static, str> {
        if let Some(reason) = self.reason_at(stage) {
            Cow::Owned(format!(
                "Reversible DLP response restoration failed. [stage={} reason={}]",
                stage.label(),
                reason.label()
            ))
        } else if self == Self::RestoreFailed {
            Cow::Borrowed(stage.message())
        } else {
            Cow::Borrowed(self.message())
        }
    }

    /// A real local resource limit, not a restoration limitation. The proxy could not
    /// hold the payload, so it must refuse instead of forwarding bytes it could not
    /// inspect; every other failure degrades to the already-redacted upstream answer.
    pub(super) fn is_resource_exhaustion(self) -> bool {
        matches!(self, Self::FailedBecause(RestoreReason::RestoreCapacity))
    }

    pub(super) fn capacity(_: TokenError) -> Self {
        Self::FailedBecause(RestoreReason::RestoreCapacity)
    }

    /// Contract violations are shape failures; only the resource variant carries a real
    /// local limit that has to survive projection.
    pub(super) fn contract(error: super::restoration_contract::ContractError) -> Self {
        match error {
            super::restoration_contract::ContractError::Resource(token) => Self::scan_error(token),
            _ => Self::RestoreFailed,
        }
    }

    /// Reconstruction and encoding are not pure allocation sites: a generic
    /// unavailability there is a step failure, and only an explicit capacity denial is
    /// a real local limit.
    pub(super) fn step(error: TokenError) -> Self {
        match error {
            TokenError::CapacityExceeded => Self::capacity(error),
            _ => Self::RestoreFailed,
        }
    }

    pub(super) fn scan_error(error: TokenError) -> Self {
        match error {
            TokenError::ContextUnavailable => Self::FailedBecause(RestoreReason::TokenFormat),
            TokenError::CapacityExceeded | TokenError::Unavailable => Self::capacity(error),
            _ => Self::RestoreFailed,
        }
    }

    fn arguments_error(error: TokenError, checkpoint: RestoreReason) -> Self {
        match error {
            TokenError::CapacityExceeded | TokenError::Unavailable => Self::capacity(error),
            _ => Self::FailedBecause(checkpoint),
        }
    }

    fn with_fallback_reason(self, reason: RestoreReason) -> Self {
        match self {
            Self::RestoreFailed => Self::FailedBecause(reason),
            _ => self,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum ArgumentNormalizationMode {
    Exact,
    ChatCanonical,
    XaiWholeFloat,
}

fn push_unique_text(
    list: &mut PayloadVec<PayloadText>,
    text: &str,
    limit: usize,
    budget: &PayloadBudget,
) -> Result<(), RestoreError> {
    if list.as_slice().iter().any(|item| item.as_str() == text) {
        return Ok(());
    }
    if list.len() >= MAX_RESPONSE_ITEMS {
        return Err(RestoreError::FailedBecause(RestoreReason::RestoreCapacity));
    }
    let owned =
        PayloadText::try_from_str(text, budget.clone(), limit).map_err(RestoreError::capacity)?;
    list.push(owned).map_err(RestoreError::capacity)
}

pub(super) fn check_no_tokens_in_str(
    text: &str,
    lease: &DlpRequestLease,
) -> Result<(), RestoreError> {
    // Unsupported carriers retain literals, but cannot carry an authorized reference.
    let mut found_authorized = false;
    super::output_text_matching::visit_complete_references(text, |range| {
        if lease.resolve_if_authorized(&text[range])?.is_some() {
            found_authorized = true;
        }
        Ok(())
    })
    .map_err(RestoreError::scan_error)?;
    if found_authorized {
        return Err(RestoreError::FailedBecause(RestoreReason::TokenContext));
    }
    Ok(())
}

fn check_no_tokens_in_value(value: &Value, lease: &DlpRequestLease) -> Result<(), RestoreError> {
    match value {
        Value::String(s) => check_no_tokens_in_str(s, lease),
        Value::Array(items) => {
            for item in items {
                check_no_tokens_in_value(item, lease)?;
            }
            Ok(())
        }
        Value::Object(map) => {
            for (k, v) in map {
                check_no_tokens_in_str(k, lease)?;
                check_no_tokens_in_value(v, lease)?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

fn check_no_complete_output_references(
    text: &str,
    lease: &DlpRequestLease,
) -> Result<(), RestoreError> {
    check_no_tokens_in_str(text, lease)
}

fn check_restored_output_part(part: &Value, lease: &DlpRequestLease) -> Result<(), RestoreError> {
    if part.get("type").and_then(Value::as_str) != Some("output_text") {
        return check_no_tokens_in_value(part, lease);
    }
    for (key, value) in part.as_object().ok_or(RestoreError::RestoreFailed)? {
        check_no_tokens_in_str(key, lease)?;
        if key == "text" {
            check_no_complete_output_references(
                value.as_str().ok_or(RestoreError::RestoreFailed)?,
                lease,
            )?;
        } else {
            check_no_tokens_in_value(value, lease)?;
        }
    }
    Ok(())
}

// Exemption is limited to protocol-identified text leaves, never their siblings.
fn check_reasoning_part(
    part: &Value,
    expected_kind: &str,
    lease: &DlpRequestLease,
) -> Result<(), RestoreError> {
    let object = part.as_object().ok_or(RestoreError::RestoreFailed)?;
    if part.get("type").and_then(Value::as_str) != Some(expected_kind)
        || !part.get("text").is_some_and(Value::is_string)
    {
        check_no_tokens_in_value(part, lease)?;
        return Err(RestoreError::RestoreFailed);
    }
    for (key, value) in object {
        check_no_tokens_in_str(key, lease)?;
        if key != "text" {
            check_no_tokens_in_value(value, lease)?;
        }
    }
    Ok(())
}

fn check_reasoning_item(item: &Value, lease: &DlpRequestLease) -> Result<(), RestoreError> {
    for (key, value) in item.as_object().ok_or(RestoreError::RestoreFailed)? {
        check_no_tokens_in_str(key, lease)?;
        match key.as_str() {
            "summary" | "content" => {
                let expected = if key == "summary" {
                    "summary_text"
                } else {
                    "reasoning_text"
                };
                for part in value.as_array().ok_or(RestoreError::RestoreFailed)? {
                    check_reasoning_part(part, expected, lease)?;
                }
            }
            _ => check_no_tokens_in_value(value, lease)?,
        }
    }
    Ok(())
}

fn check_restored_output_item(item: &Value, lease: &DlpRequestLease) -> Result<(), RestoreError> {
    let kind = item.get("type").and_then(Value::as_str);
    if kind == Some("reasoning") {
        return check_reasoning_item(item, lease);
    }
    if !matches!(kind, Some("message" | "function_call" | "custom_tool_call")) {
        return check_no_tokens_in_value(item, lease);
    }
    for (key, value) in item.as_object().ok_or(RestoreError::RestoreFailed)? {
        check_no_tokens_in_str(key, lease)?;
        if kind == Some("message") && key == "content" {
            for part in value.as_array().ok_or(RestoreError::RestoreFailed)? {
                check_restored_output_part(part, lease)?;
            }
        } else if kind != Some("message") && key == "reasoning_content" {
            if !value.is_null() && !value.is_string() {
                return Err(RestoreError::RestoreFailed);
            }
        } else {
            check_no_tokens_in_value(value, lease)?;
        }
    }
    Ok(())
}

fn check_restored_response(response: &Value, lease: &DlpRequestLease) -> Result<(), RestoreError> {
    for (key, value) in response.as_object().ok_or(RestoreError::RestoreFailed)? {
        check_no_tokens_in_str(key, lease)?;
        if key == "output" {
            for item in value.as_array().ok_or(RestoreError::RestoreFailed)? {
                check_restored_output_item(item, lease)?;
            }
        } else {
            check_no_tokens_in_value(value, lease)?;
        }
    }
    Ok(())
}

fn check_restored_event(
    evt: &str,
    data: &Value,
    lease: &DlpRequestLease,
) -> Result<(), RestoreError> {
    for (key, value) in data.as_object().ok_or(RestoreError::RestoreFailed)? {
        check_no_tokens_in_str(key, lease)?;
        match (evt, key.as_str()) {
            ("response.output_text.delta", "delta") | ("response.output_text.done", "text") => {
                check_no_complete_output_references(
                    value.as_str().ok_or(RestoreError::RestoreFailed)?,
                    lease,
                )?
            }
            ("response.reasoning_summary_text.delta", "delta")
            | ("response.reasoning_summary_text.done", "text")
            | ("response.reasoning_text.delta", "delta")
            | ("response.reasoning_text.done", "text") => {
                value.as_str().ok_or(RestoreError::RestoreFailed)?;
            }
            (
                "response.reasoning_summary_part.added" | "response.reasoning_summary_part.done",
                "part",
            ) => check_reasoning_part(value, "summary_text", lease)?,
            ("response.content_part.added" | "response.content_part.done", "part") => {
                if value.get("type").and_then(Value::as_str) == Some("reasoning_text") {
                    check_reasoning_part(value, "reasoning_text", lease)?;
                } else {
                    check_restored_output_part(value, lease)?;
                }
            }
            ("response.output_item.added" | "response.output_item.done", "item") => {
                check_restored_output_item(value, lease)?
            }
            ("response.completed", "response") => check_restored_response(value, lease)?,
            _ => check_no_tokens_in_value(value, lease)?,
        }
    }
    Ok(())
}

pub(super) fn restore_text_with_lease(
    text: &str,
    lease: &DlpRequestLease,
    budget: &PayloadBudget,
    output_tokens: &mut PayloadVec<PayloadText>,
    restored_occurrences: &mut usize,
) -> Result<PayloadText, RestoreError> {
    restore_text_with_matching(
        text,
        lease,
        budget,
        output_tokens,
        restored_occurrences,
        false,
    )
}

fn restore_text_with_matching(
    text: &str,
    lease: &DlpRequestLease,
    budget: &PayloadBudget,
    output_tokens: &mut PayloadVec<PayloadText>,
    restored_occurrences: &mut usize,
    _output_text: bool,
) -> Result<PayloadText, RestoreError> {
    super::restoration_core::restore_text(text, lease, budget, output_tokens, restored_occurrences)
}

type TextReplacements<'a> = PayloadVec<(&'a Value, PayloadText)>;

fn restore_json_leaves<'a>(
    value: &'a Value,
    lease: &DlpRequestLease,
    budget: &PayloadBudget,
    output_tokens: &mut PayloadVec<PayloadText>,
    restored_occurrences: &mut usize,
    replacements: &mut TextReplacements<'a>,
) -> Result<(), RestoreError> {
    match value {
        Value::String(text) => {
            let restored =
                restore_text_with_lease(text, lease, budget, output_tokens, restored_occurrences)?;
            replacements
                .push((value, restored))
                .map_err(RestoreError::capacity)
        }
        Value::Array(items) => {
            for item in items {
                restore_json_leaves(
                    item,
                    lease,
                    budget,
                    output_tokens,
                    restored_occurrences,
                    replacements,
                )?;
            }
            Ok(())
        }
        Value::Object(map) => {
            for (key, value) in map {
                check_no_tokens_in_str(key, lease)?;
                restore_json_leaves(
                    value,
                    lease,
                    budget,
                    output_tokens,
                    restored_occurrences,
                    replacements,
                )?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

fn apply_text_replacements(
    source: &Value,
    replacements: &TextReplacements<'_>,
    budget: &PayloadBudget,
) -> Result<PayloadJson, RestoreError> {
    let mut edits = PayloadVec::new(budget.clone()).map_err(RestoreError::scan_error)?;
    for (target, text) in replacements.as_slice() {
        edits
            .push(JsonStringReplacement {
                target,
                text: text.as_str(),
            })
            .map_err(RestoreError::scan_error)?;
    }
    PayloadJson::try_with_replacements(source, edits.as_slice(), budget.clone(), budget.limit())
        .map_err(RestoreError::step)
}

pub(super) fn serialize_json_with_budget(
    value: &Value,
    budget: &PayloadBudget,
    limit: usize,
) -> Result<PayloadBuffer, RestoreError> {
    struct CountWriter {
        len: usize,
        limit: usize,
    }
    impl Write for CountWriter {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.len = self
                .len
                .checked_add(buf.len())
                .filter(|&n| n <= self.limit)
                .ok_or_else(|| io::Error::other("capacity"))?;
            Ok(buf.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    struct BufWriter {
        buf: PayloadBuffer,
    }
    impl Write for BufWriter {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.buf
                .append(bytes)
                .map_err(|_| io::Error::other("capacity"))?;
            Ok(bytes.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    let mut counter = CountWriter { len: 0, limit };
    serde_json::to_writer(&mut counter, value).map_err(|_| RestoreError::RestoreFailed)?;
    let buf = PayloadBuffer::with_capacity(counter.len, budget.clone(), limit)
        .map_err(RestoreError::step)?;
    let mut writer = BufWriter { buf };
    serde_json::to_writer(&mut writer, value).map_err(|_| RestoreError::RestoreFailed)?;
    Ok(writer.buf)
}

pub(super) fn restore_json_arguments_string(
    raw_args: &str,
    lease: &DlpRequestLease,
    budget: &PayloadBudget,
    output_tokens: &mut PayloadVec<PayloadText>,
    restored_occurrences: &mut usize,
) -> Result<PayloadText, RestoreError> {
    if raw_args.trim().is_empty() {
        return Err(RestoreError::FailedBecause(RestoreReason::ArgumentsEmpty));
    }
    if raw_args.len() > MAX_ARGUMENT_BYTES {
        return Err(RestoreError::FailedBecause(RestoreReason::ArgumentsSize));
    }
    let parsed = PayloadJson::try_from_str(raw_args, budget.clone(), budget.limit())
        .map_err(|error| RestoreError::arguments_error(error, RestoreReason::ArgumentsJson))?;
    if !parsed.as_value().is_object() {
        return Err(RestoreError::FailedBecause(RestoreReason::ArgumentsShape));
    }
    let before = *restored_occurrences;
    let mut replacements = PayloadVec::new(budget.clone()).map_err(RestoreError::capacity)?;
    restore_json_leaves(
        parsed.as_value(),
        lease,
        budget,
        output_tokens,
        restored_occurrences,
        &mut replacements,
    )?;
    if *restored_occurrences == before {
        return PayloadText::try_from_str(raw_args, budget.clone(), MAX_ARGUMENT_BYTES)
            .map_err(RestoreError::capacity);
    }
    let restored = apply_text_replacements(parsed.as_value(), &replacements, budget)
        .map_err(|error| error.with_fallback_reason(RestoreReason::ArgumentsRebuild))?;
    let encoded = serialize_json_with_budget(restored.as_value(), budget, MAX_ARGUMENT_BYTES)
        .map_err(|error| error.with_fallback_reason(RestoreReason::ArgumentsSerialize))?;
    let text = std::str::from_utf8(encoded.as_slice())
        .map_err(|_| RestoreError::FailedBecause(RestoreReason::ArgumentsCopy))?;
    PayloadText::try_from_str(text, budget.clone(), MAX_ARGUMENT_BYTES)
        .map_err(|error| RestoreError::arguments_error(error, RestoreReason::ArgumentsCopy))
}

fn responses_lifecycle<T>(
    lease: &DlpRequestLease,
    binding: &ActualAttemptBinding,
    budget: &PayloadBudget,
    pair: ProtocolPair,
    attempt: super::restoration_core::AttemptProvenance,
) -> Result<ResponseLifecycle<T>, RestoreError> {
    ResponseLifecycle::begin_native(
        lease
            .fork_attempt_extracted()
            .map_err(RestoreError::capacity)?,
        binding.clone(),
        budget.clone(),
        pair,
        attempt,
        LifecycleLimits {
            max_text_bytes: MAX_ARGUMENT_BYTES,
            max_text_streams: MAX_RESPONSE_ITEMS,
            max_held_bytes: MAX_BARRIER_BYTES,
            max_held_units: MAX_RESPONSE_ITEMS,
        },
    )
}

fn text_part_kind(part: &Value) -> Result<(u8, &'static str), RestoreError> {
    match part.get("type").and_then(Value::as_str) {
        Some("output_text") => Ok((1, "text")),
        Some("refusal") => Ok((2, "refusal")),
        Some("summary_text") => Ok((3, "text")),
        Some("reasoning_text") => Ok((4, "text")),
        _ => Err(RestoreError::RestoreFailed),
    }
}

fn text_carrier(kind: u8) -> Carrier {
    match kind {
        1 => Carrier::OutputText,
        2 => Carrier::Refusal,
        _ => Carrier::Reasoning,
    }
}

fn text_policy(kind: u8) -> Result<TextPolicy, RestoreError> {
    match kind {
        1 => Ok(TextPolicy::Output),
        2 => Ok(TextPolicy::Refusal),
        3 | 4 => Ok(TextPolicy::Reasoning),
        _ => Err(RestoreError::RestoreFailed),
    }
}

fn decision_text(
    decision: RestoreDecision,
    original: &str,
    budget: &PayloadBudget,
    limit: usize,
) -> Result<PayloadText, RestoreError> {
    match decision {
        RestoreDecision::Unchanged => PayloadText::try_from_str(original, budget.clone(), limit)
            .map_err(RestoreError::capacity),
        RestoreDecision::Patch(text) => Ok(text),
        RestoreDecision::Hold => {
            PayloadText::try_from_str("", budget.clone(), limit).map_err(RestoreError::capacity)
        }
    }
}

fn restore_output_item<'a>(
    item: &'a Value,
    is_compact: bool,
    lease: &DlpRequestLease,
    budget: &PayloadBudget,
    lifecycle: &mut ResponseLifecycle,
    replacements: &mut TextReplacements<'a>,
) -> Result<(), RestoreError> {
    let obj = item.as_object().ok_or(RestoreError::RestoreFailed)?;
    let kind = obj
        .get("type")
        .and_then(Value::as_str)
        .ok_or(RestoreError::RestoreFailed)?;
    match kind {
        "reasoning" => {
            check_reasoning_item(item, lease)?;
            for field in ["summary", "content"] {
                if let Some(parts) = item.get(field).and_then(Value::as_array) {
                    for part in parts {
                        let text = part
                            .get("text")
                            .and_then(Value::as_str)
                            .ok_or(RestoreError::RestoreFailed)?;
                        let id = lifecycle.open_text(TextPolicy::Reasoning)?;
                        drop(lifecycle.done(&id, text)?);
                    }
                }
            }
        }
        "message" => {
            for (key, value) in obj {
                check_no_tokens_in_str(key, lease)?;
                if key != "content" {
                    check_no_tokens_in_value(value, lease)?;
                }
            }
            let parts = obj
                .get("content")
                .and_then(Value::as_array)
                .ok_or(RestoreError::RestoreFailed)?;
            for part in parts {
                let (field, policy) = match part.get("type").and_then(Value::as_str) {
                    Some("output_text") => ("text", TextPolicy::Output),
                    Some("refusal") => ("refusal", TextPolicy::Refusal),
                    _ => {
                        check_no_tokens_in_value(part, lease)?;
                        return Err(RestoreError::RestoreFailed);
                    }
                };
                for (key, value) in part.as_object().ok_or(RestoreError::RestoreFailed)? {
                    check_no_tokens_in_str(key, lease)?;
                    if key != field {
                        check_no_tokens_in_value(value, lease)?;
                    }
                }
                let target = part.get(field).ok_or(RestoreError::RestoreFailed)?;
                let text = target.as_str().ok_or(RestoreError::RestoreFailed)?;
                let id = lifecycle.open_text(policy)?;
                let restored =
                    decision_text(lifecycle.done(&id, text)?, text, budget, MAX_FRAME_BYTES)?;
                replacements
                    .push((target, restored))
                    .map_err(RestoreError::capacity)?;
            }
        }
        "function_call" | "custom_tool_call" if !is_compact => {
            let field = if kind == "function_call" {
                "arguments"
            } else {
                "input"
            };
            for (key, value) in obj {
                check_no_tokens_in_str(key, lease)?;
                if key != field && key != "reasoning_content" {
                    check_no_tokens_in_value(value, lease)?;
                }
            }
            for field in ["call_id", "name"] {
                obj.get(field)
                    .and_then(Value::as_str)
                    .filter(|id| valid_external_id(id))
                    .ok_or(RestoreError::RestoreFailed)?;
            }
            let target = obj.get(field).ok_or(RestoreError::RestoreFailed)?;
            let text = target.as_str().ok_or(RestoreError::RestoreFailed)?;
            let restored = if kind == "function_call" {
                lifecycle.arguments(text)?
            } else {
                let id = lifecycle.open_text(TextPolicy::ToolArguments)?;
                decision_text(lifecycle.done(&id, text)?, text, budget, MAX_ARGUMENT_BYTES)?
            };
            replacements
                .push((target, restored))
                .map_err(RestoreError::capacity)?;
            if obj
                .get("reasoning_content")
                .is_some_and(|v| !v.is_null() && !v.is_string())
                || obj
                    .get("encrypted_reasoning")
                    .is_some_and(|v| !v.is_null() && !v.is_string())
            {
                return Err(RestoreError::RestoreFailed);
            }
        }
        "compaction" if is_compact => {
            check_no_tokens_in_value(item, lease)?;
            obj.get("encrypted_content")
                .and_then(Value::as_str)
                .filter(|text| !text.is_empty() && text.len() <= MAX_FRAME_BYTES)
                .ok_or(RestoreError::RestoreFailed)?;
        }
        _ => {
            check_no_tokens_in_value(item, lease)?;
            return Err(RestoreError::RestoreFailed);
        }
    }
    Ok(())
}

/// A restored tree never exposes mutable or unowned extraction.
pub(super) struct RestoredJsonResponse {
    value: PayloadJson,
    restored_occurrences: usize,
}
impl RestoredJsonResponse {
    pub(super) fn as_value(&self) -> &Value {
        self.value.as_value()
    }
    pub(super) fn restored_occurrences(&self) -> usize {
        self.restored_occurrences
    }
}

/// The original tokenized snapshot remains borrowed until publication.
pub(super) struct StagedJsonResponse<'a> {
    completion_protocol: super::completion_json::JsonCompletionProtocol,
    restored: RestoredJsonResponse,
    source: &'a Value,
    output_tokens: JsonDependencies,
}

struct JsonDependencies(FinishedInspection);

impl std::ops::Deref for JsonDependencies {
    type Target = [PayloadText];

    fn deref(&self) -> &Self::Target {
        self.0.dependency_tokens()
    }
}

impl StagedJsonResponse<'_> {
    pub(super) fn as_value(&self) -> &Value {
        self.restored.as_value()
    }
    #[cfg(test)]
    pub(super) fn publish(
        self,
        lease: &DlpRequestLease,
        binding: &ActualAttemptBinding,
        witness: &CompletionWitness,
        budget: &PayloadBudget,
    ) -> Result<RestoredJsonResponse, RestoreError> {
        finalization::publish_fixture(
            self.source,
            DeliveredToolSnapshots {
                response: self.restored.as_value(),
                tracks: &[],
                events: &[],
            },
            &self.output_tokens,
            lease,
            binding,
            witness,
            budget,
        )?;
        Ok(self.restored)
    }
}

/// The same non-Clone mechanics owner moves from original inspection into client staging.
/// No original tree, lease, proof, publication capability or dependency copy can be extracted.
pub(super) struct ResponsesJsonInspection {
    lifecycle: ResponseLifecycle,
    binding: ActualAttemptBinding,
    pair: ProtocolPair,
    attempt: super::restoration_core::AttemptProvenance,
    observed: bool,
}

impl ResponsesJsonInspection {
    pub(super) fn attempt(&self) -> &super::restoration_core::AttemptProvenance {
        &self.attempt
    }

    pub(super) fn begin(
        lease: &DlpRequestLease,
        binding: &ActualAttemptBinding,
        budget: &PayloadBudget,
        pair: ProtocolPair,
        attempt: super::restoration_core::AttemptProvenance,
    ) -> Result<Self, RestoreError> {
        Ok(Self {
            lifecycle: responses_lifecycle(lease, binding, budget, pair, attempt.clone())?,
            binding: binding.clone(),
            pair,
            attempt,
            observed: false,
        })
    }

    pub(super) fn observe_original(
        &mut self,
        original: &super::restoration_contract::OriginalResponseObservation<'_>,
    ) -> Result<(), RestoreError> {
        self.lifecycle.observe_original(original)?;
        self.observed = true;
        Ok(())
    }

    fn into_lifecycle(
        self,
        binding: &ActualAttemptBinding,
        pair: ProtocolPair,
        attempt: &super::restoration_core::AttemptProvenance,
    ) -> Result<ResponseLifecycle, RestoreError> {
        let verified = &self.binding;
        if !self.observed
            || verified.store_id != binding.store_id
            || verified.namespace != binding.namespace
            || verified.generation != binding.generation
            || verified.policy_revision != binding.policy_revision
            || verified.provider_fingerprint != binding.provider_fingerprint
            || verified.reusable_principal != binding.reusable_principal
            || verified.operation_lineage != binding.operation_lineage
            || self.pair.client() != pair.client()
            || self.pair.upstream() != pair.upstream()
            || !self.attempt.matches(attempt)
        {
            return Err(RestoreError::RestoreFailed);
        }
        Ok(self.lifecycle)
    }
}

// Keep independent authority inputs explicit at this compatibility boundary.
#[allow(clippy::too_many_arguments)]
pub(super) fn stage_json_response_bound<'a>(
    response: &'a Value,
    is_compact: bool,
    lease: &DlpRequestLease,
    binding: &ActualAttemptBinding,
    budget: &PayloadBudget,
    pair: ProtocolPair,
    attempt: &super::restoration_core::AttemptProvenance,
    inspection: ResponsesJsonInspection,
) -> Result<StagedJsonResponse<'a>, RestoreError> {
    let obj = response.as_object().ok_or(RestoreError::RestoreFailed)?;
    obj.get("id")
        .and_then(Value::as_str)
        .filter(|id| valid_external_id(id))
        .ok_or(RestoreError::RestoreFailed)?;
    for (key, value) in obj {
        check_no_tokens_in_str(key, lease)?;
        if key != "output" {
            check_no_tokens_in_value(value, lease)?;
        }
    }
    let output = obj
        .get("output")
        .and_then(Value::as_array)
        .ok_or(RestoreError::RestoreFailed)?;
    if output.len() > MAX_RESPONSE_ITEMS
        || output
            .iter()
            .filter(|item| {
                matches!(
                    item.get("type").and_then(Value::as_str),
                    Some("function_call" | "custom_tool_call")
                )
            })
            .count()
            > MAX_BARRIER_TOOLS
    {
        return Err(RestoreError::RestoreFailed);
    }
    let mut replacements = PayloadVec::new(budget.clone()).map_err(RestoreError::capacity)?;
    let mut lifecycle = inspection.into_lifecycle(binding, pair, attempt)?;
    for item in output {
        restore_output_item(
            item,
            is_compact,
            lease,
            budget,
            &mut lifecycle,
            &mut replacements,
        )?;
    }
    let value = apply_text_replacements(response, &replacements, budget)?;
    let inspection = lifecycle.finish()?;
    // Converted Compact endpoints still deliver a regular Responses envelope.
    // Select from the checked protocol pair, never from untrusted response fields.
    let completion_protocol = if is_compact
        && pair.upstream().protocol() == crate::proxy::protocol_bridge::Protocol::Responses
    {
        super::completion_json::JsonCompletionProtocol::ResponsesCompact
    } else {
        super::completion_json::JsonCompletionProtocol::NativeResponses
    };
    Ok(StagedJsonResponse {
        completion_protocol,
        restored: RestoredJsonResponse {
            value,
            restored_occurrences: inspection.restored_occurrences(),
        },
        source: response,
        output_tokens: JsonDependencies(inspection),
    })
}

/// Legacy unbound fixtures cannot be selected by the production HTTP facade.
#[cfg(test)]
pub(super) fn stage_json_response<'a>(
    response: &'a Value,
    is_compact: bool,
    lease: &DlpRequestLease,
    budget: &PayloadBudget,
) -> Result<StagedJsonResponse<'a>, RestoreError> {
    let binding = lease
        .bind_actual_attempt(
            "responses-fixture",
            "openai_responses",
            "http://127.0.0.1:1/responses",
            None,
        )
        .map_err(RestoreError::scan_error)?;
    let original = super::restoration_contract::OriginalResponseObservation::json(
        response,
        UpstreamProtocol::new(crate::proxy::protocol_bridge::Protocol::Responses),
        super::restoration_core::AttemptProvenance::new(budget).map_err(RestoreError::capacity)?,
        budget.clone(),
    )?;
    let pair = ProtocolPair::new(
        ClientProtocol::new(crate::proxy::protocol_bridge::Protocol::Responses),
        original.upstream(),
    );
    let mut inspection =
        ResponsesJsonInspection::begin(lease, &binding, budget, pair, original.attempt().clone())?;
    inspection.observe_original(&original)?;
    stage_json_response_bound(
        response,
        is_compact,
        lease,
        &binding,
        budget,
        pair,
        original.attempt(),
        inspection,
    )
}

#[cfg(test)]
pub(super) fn restore_and_publish_json_response(
    response: &Value,
    is_compact: bool,
    lease: &DlpRequestLease,
    binding: &ActualAttemptBinding,
    witness: &CompletionWitness,
    budget: &PayloadBudget,
) -> Result<RestoredJsonResponse, RestoreError> {
    stage_json_response(response, is_compact, lease, budget)?
        .publish(lease, binding, witness, budget)
}

fn copy_string_edits<'a>(
    raw: &'a Value,
    restored: &'a Value,
    edits: &mut PayloadVec<JsonStringReplacement<'a>>,
) -> Result<(), RestoreError> {
    match (raw, restored) {
        (Value::String(a), Value::String(b)) if a != b => {
            edits
                .push(JsonStringReplacement {
                    target: raw,
                    text: b,
                })
                .map_err(RestoreError::scan_error)?;
        }
        (Value::Object(a), Value::Object(b)) => {
            for (key, value) in a {
                if let Some(other) = b.get(key) {
                    copy_string_edits(value, other, edits)?;
                }
            }
        }
        (Value::Array(a), Value::Array(b)) => {
            for (value, other) in a.iter().zip(b) {
                copy_string_edits(value, other, edits)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn item_string_edits<'a>(
    raw: &'a Value,
    restored: &'a Value,
    added: bool,
    preserve_payload: bool,
    edits: &mut PayloadVec<JsonStringReplacement<'a>>,
) -> Result<(), RestoreError> {
    // Only supported text is rewritten; metadata and unknown siblings stay untouched.
    let kind = raw
        .get("type")
        .and_then(Value::as_str)
        .ok_or(RestoreError::RestoreFailed)?;
    for field in ["arguments", "input", "reasoning_content"] {
        let supported = matches!(
            (kind, field),
            ("function_call", "arguments" | "reasoning_content")
                | ("custom_tool_call", "input" | "reasoning_content")
        );
        if !supported || (preserve_payload && matches!(field, "arguments" | "input")) {
            continue;
        }
        if let (Some(a), Some(b)) = (raw.get(field), restored.get(field)) {
            if !(added && a.as_str() == Some("")) {
                copy_string_edits(a, b, edits)?;
            }
        }
    }
    for field in ["content", "summary"] {
        if !matches!(
            (kind, field),
            ("message", "content") | ("reasoning", "summary" | "content")
        ) {
            continue;
        }
        if let (Some(a), Some(b)) = (
            raw.get(field).and_then(Value::as_array),
            restored.get(field).and_then(Value::as_array),
        ) {
            for (part, final_part) in a.iter().zip(b) {
                let key = if part.get("type").and_then(Value::as_str) == Some("refusal") {
                    "refusal"
                } else {
                    "text"
                };
                if let (Some(text), Some(final_text)) = (part.get(key), final_part.get(key)) {
                    if !(added && text.as_str() == Some("")) {
                        copy_string_edits(text, final_text, edits)?;
                    }
                }
            }
        }
    }
    Ok(())
}

fn check_snapshot_part(
    part: &Value,
    container: &str,
    added: bool,
    lease: &DlpRequestLease,
) -> Result<(), RestoreError> {
    let kind = part.get("type").and_then(Value::as_str);
    let field = match (container, kind) {
        ("summary", Some("summary_text")) | ("reasoning_content", Some("reasoning_text")) => {
            return check_reasoning_part(
                part,
                if container == "summary" {
                    "summary_text"
                } else {
                    "reasoning_text"
                },
                lease,
            );
        }
        ("content", Some("output_text")) => "text",
        ("content", Some("refusal")) => "refusal",
        _ => {
            check_no_tokens_in_value(part, lease)?;
            return Err(RestoreError::RestoreFailed);
        }
    };
    if !part.get(field).is_some_and(Value::is_string) && !(added && part.get(field).is_none()) {
        return Err(RestoreError::RestoreFailed);
    }
    for (key, value) in part.as_object().ok_or(RestoreError::RestoreFailed)? {
        check_no_tokens_in_str(key, lease)?;
        if key != field {
            check_no_tokens_in_value(value, lease)?;
        }
    }
    Ok(())
}

fn check_snapshot_carriers(data: &Value, lease: &DlpRequestLease) -> Result<(), RestoreError> {
    let added = data
        .get("type")
        .and_then(Value::as_str)
        .is_some_and(|event| event.ends_with(".added"));
    if let Some(item) = data.get("item") {
        let kind = item
            .get("type")
            .and_then(Value::as_str)
            .ok_or(RestoreError::RestoreFailed)?;
        for (key, value) in item.as_object().ok_or(RestoreError::RestoreFailed)? {
            check_no_tokens_in_str(key, lease)?;
            match (kind, key.as_str()) {
                ("message", "content") | ("reasoning", "summary" | "content") => {
                    let container = if kind == "reasoning" && key == "content" {
                        "reasoning_content"
                    } else {
                        key.as_str()
                    };
                    for part in value.as_array().ok_or(RestoreError::RestoreFailed)? {
                        check_snapshot_part(part, container, added, lease)?;
                    }
                }
                ("function_call", "arguments" | "reasoning_content")
                | ("custom_tool_call", "input" | "reasoning_content") => {}
                _ => check_no_tokens_in_value(value, lease)?,
            }
        }
    } else if let Some(part) = data.get("part") {
        let summary = data
            .get("type")
            .and_then(Value::as_str)
            .is_some_and(|kind| kind.starts_with("response.reasoning_summary_part."));
        let container = if summary {
            "summary"
        } else if part.get("type").and_then(Value::as_str) == Some("reasoning_text") {
            "reasoning_content"
        } else {
            "content"
        };
        check_snapshot_part(part, container, added, lease)?;
    }
    for (key, value) in data.as_object().ok_or(RestoreError::RestoreFailed)? {
        check_no_tokens_in_str(key, lease)?;
        if key != "item" && key != "part" {
            check_no_tokens_in_value(value, lease)?;
        }
    }
    Ok(())
}

fn validate_snapshot_text(
    raw: Option<&Value>,
    final_value: Option<&Value>,
    added: bool,
    arguments: bool,
    mode: ArgumentNormalizationMode,
    budget: &PayloadBudget,
) -> Result<(), RestoreError> {
    let Some(raw) = raw else {
        return if added {
            Ok(())
        } else {
            Err(RestoreError::RestoreFailed)
        };
    };
    let raw = raw.as_str().ok_or(RestoreError::RestoreFailed)?;
    if added && raw.is_empty() {
        return Ok(());
    }
    let final_text = final_value
        .and_then(Value::as_str)
        .ok_or(RestoreError::RestoreFailed)?;
    if if arguments {
        arguments_match_mode(raw, final_text, mode, budget)?
    } else {
        raw == final_text
    } {
        Ok(())
    } else {
        Err(RestoreError::RestoreFailed)
    }
}

fn validate_part_snapshot(
    raw: &Value,
    final_part: &Value,
    added: bool,
    mode: ArgumentNormalizationMode,
    budget: &PayloadBudget,
) -> Result<(), RestoreError> {
    if raw.get("type") != final_part.get("type") {
        return Err(RestoreError::RestoreFailed);
    }
    let field = match raw.get("type").and_then(Value::as_str) {
        Some("output_text" | "summary_text" | "reasoning_text") => "text",
        Some("refusal") => "refusal",
        _ => return Err(RestoreError::RestoreFailed),
    };
    validate_snapshot_text(
        raw.get(field),
        final_part.get(field),
        added,
        false,
        mode,
        budget,
    )
}

fn validate_raw_snapshot(
    data: &Value,
    output: &[Value],
    mode: ArgumentNormalizationMode,
    budget: &PayloadBudget,
) -> Result<(), RestoreError> {
    let evt = data
        .get("type")
        .and_then(Value::as_str)
        .ok_or(RestoreError::RestoreFailed)?;
    let added = evt.ends_with(".added");
    let index = data
        .get("output_index")
        .and_then(Value::as_u64)
        .and_then(|n| usize::try_from(n).ok())
        .ok_or(RestoreError::RestoreFailed)?;
    let final_item = output.get(index).ok_or(RestoreError::RestoreFailed)?;
    if evt.starts_with("response.output_item.") {
        let raw = data.get("item").ok_or(RestoreError::RestoreFailed)?;
        for key in ["id", "call_id", "name", "type", "namespace"] {
            if raw.get(key) != final_item.get(key) {
                return Err(RestoreError::RestoreFailed);
            }
        }
        match raw.get("type").and_then(Value::as_str) {
            Some("message" | "reasoning") => {
                for field in ["content", "summary"] {
                    if raw["type"] == "message" && field == "summary" {
                        continue;
                    }
                    match raw.get(field) {
                        Some(value) => {
                            let parts = value.as_array().ok_or(RestoreError::RestoreFailed)?;
                            if added && parts.is_empty() {
                                continue;
                            }
                            let final_parts = final_item
                                .get(field)
                                .and_then(Value::as_array)
                                .ok_or(RestoreError::RestoreFailed)?;
                            if parts.len() != final_parts.len() {
                                return Err(RestoreError::RestoreFailed);
                            }
                            for (part, final_part) in parts.iter().zip(final_parts) {
                                validate_part_snapshot(part, final_part, added, mode, budget)?;
                            }
                        }
                        None if added
                            || raw["type"] == "reasoning" && final_item.get(field).is_none() => {}
                        None => return Err(RestoreError::RestoreFailed),
                    }
                }
            }
            Some("function_call" | "custom_tool_call") => {
                let arguments = raw["type"] == "function_call";
                let field = if arguments { "arguments" } else { "input" };
                validate_snapshot_text(
                    raw.get(field),
                    final_item.get(field),
                    added,
                    arguments,
                    mode,
                    budget,
                )?;
                if raw.get("reasoning_content").is_some_and(|v| !v.is_null()) {
                    validate_snapshot_text(
                        raw.get("reasoning_content"),
                        final_item.get("reasoning_content"),
                        added,
                        false,
                        mode,
                        budget,
                    )?;
                }
            }
            _ => return Err(RestoreError::RestoreFailed),
        }
    } else {
        let summary = evt.starts_with("response.reasoning_summary_part.");
        let reasoning_content =
            data.pointer("/part/type").and_then(Value::as_str) == Some("reasoning_text");
        let expected_item = if summary || reasoning_content {
            "reasoning"
        } else {
            "message"
        };
        if final_item.get("type").and_then(Value::as_str) != Some(expected_item)
            || data
                .get("item_id")
                .is_some_and(|id| Some(id) != final_item.get("id"))
        {
            return Err(RestoreError::RestoreFailed);
        }
        let part_index = data
            .get(if summary {
                "summary_index"
            } else {
                "content_index"
            })
            .and_then(Value::as_u64)
            .and_then(|n| usize::try_from(n).ok())
            .ok_or(RestoreError::RestoreFailed)?;
        let final_part = final_item
            .get(if summary { "summary" } else { "content" })
            .and_then(Value::as_array)
            .and_then(|parts| parts.get(part_index))
            .ok_or(RestoreError::RestoreFailed)?;
        validate_part_snapshot(
            data.get("part").ok_or(RestoreError::RestoreFailed)?,
            final_part,
            added,
            mode,
            budget,
        )?;
    }
    Ok(())
}

fn is_raw_snapshot_event(evt: &str) -> bool {
    matches!(
        evt,
        "response.output_item.added"
            | "response.output_item.done"
            | "response.content_part.added"
            | "response.content_part.done"
            | "response.reasoning_summary_part.added"
            | "response.reasoning_summary_part.done"
    )
}

fn restore_completed_snapshot(
    response: &Value,
    lease: &DlpRequestLease,
    budget: &PayloadBudget,
    text_parts: &[TextPartTrack],
    tools: &[ToolItemTrack],
    inspection: &FinishedInspection<BufferedSseEvent>,
) -> Result<PayloadJson, RestoreError> {
    let output = response
        .get("output")
        .and_then(Value::as_array)
        .ok_or(RestoreError::RestoreFailed)?;
    let mut edits = PayloadVec::new(budget.clone()).map_err(RestoreError::capacity)?;
    for (output_index, item) in output.iter().enumerate() {
        match item.get("type").and_then(Value::as_str) {
            Some("message" | "reasoning") => {
                let fields: &[&str] =
                    if item.get("type").and_then(Value::as_str) == Some("reasoning") {
                        &["summary", "content"]
                    } else {
                        &["content"]
                    };
                for field in fields {
                    let Some(parts) = item.get(*field) else {
                        continue;
                    };
                    for (part_index, part) in parts
                        .as_array()
                        .ok_or(RestoreError::RestoreFailed)?
                        .iter()
                        .enumerate()
                    {
                        let (kind, key) = text_part_kind(part)?;
                        let track = text_parts
                            .iter()
                            .find(|track| {
                                track.output_index == output_index as u64
                                    && track.part_index == part_index as u64
                                    && track.kind == kind
                            })
                            .ok_or(RestoreError::RestoreFailed)?;
                        let text = inspection
                            .text_state(&track.text)?
                            .restored_done
                            .as_ref()
                            .ok_or(RestoreError::RestoreFailed)?;
                        edits
                            .push(JsonStringReplacement {
                                target: part.get(key).ok_or(RestoreError::RestoreFailed)?,
                                text: text.as_str(),
                            })
                            .map_err(RestoreError::capacity)?;
                    }
                }
            }
            Some("function_call" | "custom_tool_call") => {
                let key = if item.get("type").and_then(Value::as_str) == Some("function_call") {
                    "arguments"
                } else {
                    "input"
                };
                let track = tools
                    .iter()
                    .find(|track| track.output_index == output_index as u64)
                    .ok_or(RestoreError::RestoreFailed)?;
                let text = track
                    .restored_payload
                    .as_ref()
                    .ok_or(RestoreError::RestoreFailed)?;
                edits
                    .push(JsonStringReplacement {
                        target: item.get(key).ok_or(RestoreError::RestoreFailed)?,
                        text: text.as_str(),
                    })
                    .map_err(RestoreError::capacity)?;
            }
            _ => return Err(RestoreError::RestoreFailed),
        }
    }
    let restored = PayloadJson::try_with_replacements(
        response,
        edits.as_slice(),
        budget.clone(),
        budget.limit(),
    )
    .map_err(RestoreError::capacity)?;
    check_restored_response(restored.as_value(), lease)?;
    Ok(restored)
}

struct DeliveredToolSnapshots<'a> {
    response: &'a Value,
    tracks: &'a [ToolItemTrack],
    events: &'a [BufferedSseEvent],
}

fn completed_call_spec(item: &Value) -> Result<CompletedCallSpec<'_>, RestoreError> {
    let required = |key| {
        item.get(key)
            .and_then(Value::as_str)
            .ok_or(RestoreError::RestoreFailed)
    };
    let kind = required("type")?;
    Ok(CompletedCallSpec {
        call_id: required("call_id")?,
        item_type: kind,
        name: required("name")?,
        payload: required(if kind == "function_call" {
            "arguments"
        } else {
            "input"
        })?,
        reasoning: item.get("reasoning_content").and_then(Value::as_str),
        encrypted_reasoning: item.get("encrypted_reasoning").and_then(Value::as_str),
    })
}

pub(super) fn split_safe_prefix_and_tail(combined: &str) -> Result<(&str, &str), RestoreError> {
    Ok(super::output_text_matching::split_prefix_and_tail(combined))
}

fn permits_xai_argument_normalization(
    raw_delta_str: &str,
    normalized_str: &str,
    budget: &PayloadBudget,
) -> Result<bool, RestoreError> {
    super::completion_json::permits_xai_argument_normalization(
        raw_delta_str,
        normalized_str,
        true,
        budget,
    )
    .map_err(RestoreError::scan_error)
}

fn same_argument_json(raw: &Value, snapshot: &Value) -> bool {
    match (raw, snapshot) {
        (Value::Object(a), Value::Object(b)) => {
            a.len() == b.len()
                && a.iter().all(|(key, value)| {
                    b.get(key)
                        .is_some_and(|other| same_argument_json(value, other))
                })
        }
        (Value::Array(a), Value::Array(b)) => {
            a.len() == b.len()
                && a.iter()
                    .zip(b)
                    .all(|(value, other)| same_argument_json(value, other))
        }
        (Value::Number(a), Value::Number(b)) => {
            if a.is_f64() != b.is_f64() {
                return false;
            }
            if let (Some(a), Some(b)) = (a.as_f64(), b.as_f64()) {
                if a == 0.0 && b == 0.0 {
                    return a.is_sign_negative() == b.is_sign_negative();
                }
            }
            a == b
        }
        _ => raw == snapshot,
    }
}

fn arguments_match_mode(
    raw_deltas: &str,
    snapshot: &str,
    mode: ArgumentNormalizationMode,
    budget: &PayloadBudget,
) -> Result<bool, RestoreError> {
    let raw = PayloadJson::try_from_str(raw_deltas, budget.clone(), budget.limit())
        .map_err(RestoreError::scan_error)?;
    let final_value = PayloadJson::try_from_str(snapshot, budget.clone(), budget.limit())
        .map_err(RestoreError::scan_error)?;
    if !raw.as_value().is_object() || !final_value.as_value().is_object() {
        return Ok(false);
    }
    if same_argument_json(raw.as_value(), final_value.as_value()) {
        return Ok(true);
    }
    drop(raw);
    drop(final_value);
    match mode {
        ArgumentNormalizationMode::Exact => Ok(false),
        ArgumentNormalizationMode::ChatCanonical => permits_chat_argument_normalization(
            Some(&Value::String(raw_deltas.to_string())),
            snapshot,
            budget,
        )
        .map_err(RestoreError::scan_error),
        ArgumentNormalizationMode::XaiWholeFloat => {
            permits_xai_argument_normalization(raw_deltas, snapshot, budget)
        }
    }
}

// This observer has no lease, restoration or publication authority. It compares
// original xAI arguments before the legacy converter can erase contradictions.
pub(super) struct OriginalXaiArguments {
    budget: PayloadBudget,
    tracks: PayloadVec<OriginalArgumentTrack>,
    snapshots: PayloadVec<PayloadJson>,
    completed_output: Option<PayloadJson>,
}

struct OriginalArgumentTrack {
    output_index: u64,
    item_id: Option<PayloadText>,
    deltas: PayloadBuffer,
    saw_deltas: bool,
    done_seen: bool,
}

impl OriginalXaiArguments {
    pub(super) fn new(budget: PayloadBudget) -> Result<Self, RestoreError> {
        Ok(Self {
            tracks: PayloadVec::new(budget.clone()).map_err(RestoreError::scan_error)?,
            snapshots: PayloadVec::new(budget.clone()).map_err(RestoreError::scan_error)?,
            completed_output: None,
            budget,
        })
    }

    pub(super) fn take_completed_output(&mut self) -> Result<PayloadJson, RestoreError> {
        self.completed_output
            .take()
            .ok_or(RestoreError::RestoreFailed)
    }

    pub(super) fn observe(&mut self, data: &Value) -> Result<(), RestoreError> {
        let kind = data
            .get("type")
            .and_then(Value::as_str)
            .ok_or(RestoreError::RestoreFailed)?;
        if kind == "response.completed" {
            let output = data
                .pointer("/response/output")
                .and_then(Value::as_array)
                .ok_or(RestoreError::RestoreFailed)?;
            for snapshot in self.snapshots.as_slice() {
                let snapshot = snapshot.as_value();
                if snapshot["type"] == "response.function_call_arguments.done" {
                    let index = snapshot
                        .get("output_index")
                        .and_then(Value::as_u64)
                        .and_then(|n| usize::try_from(n).ok())
                        .ok_or(RestoreError::RestoreFailed)?;
                    let item = output.get(index).ok_or(RestoreError::RestoreFailed)?;
                    validate_snapshot_text(
                        snapshot.get("arguments"),
                        item.get("arguments"),
                        false,
                        true,
                        ArgumentNormalizationMode::Exact,
                        &self.budget,
                    )?;
                } else {
                    validate_raw_snapshot(
                        snapshot,
                        output,
                        ArgumentNormalizationMode::Exact,
                        &self.budget,
                    )?;
                }
            }
            for track in self.tracks.as_slice() {
                let index =
                    usize::try_from(track.output_index).map_err(|_| RestoreError::RestoreFailed)?;
                let item = output.get(index).ok_or(RestoreError::RestoreFailed)?;
                if item.get("type").and_then(Value::as_str) != Some("function_call")
                    || track.item_id.as_ref().is_some_and(|id| {
                        item.get("id").and_then(Value::as_str) != Some(id.as_str())
                    })
                {
                    return Err(RestoreError::RestoreFailed);
                }
                if track.saw_deltas {
                    let delta = std::str::from_utf8(track.deltas.as_slice())
                        .map_err(|_| RestoreError::RestoreFailed)?;
                    let final_args = item
                        .get("arguments")
                        .and_then(Value::as_str)
                        .ok_or(RestoreError::RestoreFailed)?;
                    if !arguments_match_mode(
                        delta,
                        final_args,
                        ArgumentNormalizationMode::Exact,
                        &self.budget,
                    )? {
                        return Err(RestoreError::RestoreFailed);
                    }
                }
            }
            self.completed_output = Some(
                PayloadJson::try_from_value(
                    data.pointer("/response/output")
                        .ok_or(RestoreError::RestoreFailed)?,
                    self.budget.clone(),
                    self.budget.limit(),
                )
                .map_err(RestoreError::scan_error)?,
            );
            return Ok(());
        }
        let field_event = matches!(
            kind,
            "response.function_call_arguments.delta" | "response.function_call_arguments.done"
        );
        let item_event = matches!(
            kind,
            "response.output_item.added" | "response.output_item.done"
        ) && data.pointer("/item/type").and_then(Value::as_str)
            == Some("function_call");
        if !field_event && !item_event {
            return Ok(());
        }
        if item_event || kind.ends_with(".done") {
            if self.snapshots.len() >= MAX_RESPONSE_ITEMS {
                return Err(RestoreError::RestoreFailed);
            }
            let snapshot =
                PayloadJson::try_from_value(data, self.budget.clone(), self.budget.limit())
                    .map_err(RestoreError::scan_error)?;
            self.snapshots
                .push(snapshot)
                .map_err(RestoreError::scan_error)?;
        }
        let output_index = data
            .get("output_index")
            .and_then(Value::as_u64)
            .ok_or(RestoreError::RestoreFailed)?;
        let position = match self
            .tracks
            .as_slice()
            .iter()
            .position(|track| track.output_index == output_index)
        {
            Some(position) => position,
            None => {
                if self.tracks.len() >= MAX_BARRIER_TOOLS {
                    return Err(RestoreError::RestoreFailed);
                }
                let track = OriginalArgumentTrack {
                    output_index,
                    item_id: None,
                    deltas: PayloadBuffer::new(self.budget.clone(), MAX_ARGUMENT_BYTES)
                        .map_err(RestoreError::scan_error)?,
                    saw_deltas: false,
                    done_seen: false,
                };
                self.tracks.push(track).map_err(RestoreError::scan_error)?;
                self.tracks.len() - 1
            }
        };
        let track = self
            .tracks
            .get_mut(position)
            .ok_or(RestoreError::RestoreFailed)?;
        let id = if field_event {
            data.get("item_id")
        } else {
            data.pointer("/item/id")
        };
        if let Some(id) = id {
            let id = id.as_str().ok_or(RestoreError::RestoreFailed)?;
            if track
                .item_id
                .as_ref()
                .is_some_and(|previous| previous.as_str() != id)
            {
                return Err(RestoreError::RestoreFailed);
            }
            if track.item_id.is_none() {
                track.item_id = Some(
                    PayloadText::try_from_str(id, self.budget.clone(), MAX_FRAME_BYTES)
                        .map_err(RestoreError::scan_error)?,
                );
            }
        }
        if kind == "response.function_call_arguments.delta" {
            if track.done_seen {
                return Err(RestoreError::RestoreFailed);
            }
            let delta = data
                .get("delta")
                .and_then(Value::as_str)
                .ok_or(RestoreError::RestoreFailed)?;
            track
                .deltas
                .append(delta.as_bytes())
                .map_err(RestoreError::scan_error)?;
            track.saw_deltas = true;
        } else if kind == "response.function_call_arguments.done" {
            if track.done_seen {
                return Err(RestoreError::RestoreFailed);
            }
            track.done_seen = true;
        }
        Ok(())
    }
}

struct TextPartTrack {
    output_index: u64,
    item_id: Option<PayloadText>,
    part_index: u64,
    kind: u8, // 1 = output_text, 2 = refusal, 3 = summary_text, 4 = reasoning_text
    text: TextStreamId,
    text_done_seen: bool,
    part_done_seen: bool,
    item_done_seen: bool,
}

struct ToolItemTrack {
    output_index: u64,
    is_custom: bool,
    saw_deltas: bool,
    raw_deltas: PayloadBuffer,
    raw_done: Option<PayloadText>,
    raw_item_done: Option<PayloadText>,
    restored_payload: Option<PayloadText>,
    payload_replaced: bool,
}

struct BufferedSseEvent {
    event_name: PayloadText,
    data: PayloadJson,
    prepared: Option<PayloadJson>,
}

/// Opaque mechanics-only handoff. It exposes neither mapping nor delivery authority.
pub(super) struct ResponsesSseInspection {
    lifecycle: ResponseLifecycle<BufferedSseEvent>,
    binding: ActualAttemptBinding,
    pair: ProtocolPair,
    attempt: super::restoration_core::AttemptProvenance,
    observed: bool,
    failure: Option<RestoreError>,
}

impl ResponsesSseInspection {
    pub(super) fn begin(
        lease: &DlpRequestLease,
        binding: &ActualAttemptBinding,
        budget: &PayloadBudget,
        pair: ProtocolPair,
        attempt: super::restoration_core::AttemptProvenance,
    ) -> Result<Self, RestoreError> {
        Ok(Self {
            lifecycle: responses_lifecycle(lease, binding, budget, pair, attempt.clone())?,
            binding: binding.clone(),
            pair,
            attempt,
            observed: false,
            failure: None,
        })
    }

    fn validate(
        &self,
        binding: &ActualAttemptBinding,
        pair: ProtocolPair,
        attempt: &super::restoration_core::AttemptProvenance,
    ) -> Result<(), RestoreError> {
        if let Some(error) = self.failure {
            return Err(error);
        }
        let actual = &self.binding;
        if actual.store_id != binding.store_id
            || actual.namespace != binding.namespace
            || actual.generation != binding.generation
            || actual.policy_revision != binding.policy_revision
            || actual.provider_fingerprint != binding.provider_fingerprint
            || actual.reusable_principal != binding.reusable_principal
            || actual.operation_lineage != binding.operation_lineage
            || self.pair.client() != pair.client()
            || self.pair.upstream() != pair.upstream()
            || !self.attempt.matches(attempt)
        {
            return Err(RestoreError::RestoreFailed);
        }
        Ok(())
    }

    pub(super) fn observe_frame(
        &mut self,
        raw: &str,
        budget: &PayloadBudget,
    ) -> Result<(), RestoreError> {
        let original = super::restoration_contract::OriginalResponseObservation::frame(
            raw.as_bytes(),
            self.pair.upstream(),
            self.attempt.clone(),
            budget.clone(),
        );
        self.observe(original)
    }

    pub(super) fn observe_json(
        &mut self,
        value: &Value,
        budget: &PayloadBudget,
    ) -> Result<(), RestoreError> {
        let original = super::restoration_contract::OriginalResponseObservation::json(
            value,
            self.pair.upstream(),
            self.attempt.clone(),
            budget.clone(),
        );
        self.observe(original)
    }

    fn observe(
        &mut self,
        original: Result<
            super::restoration_contract::OriginalResponseObservation<'_>,
            RestoreError,
        >,
    ) -> Result<(), RestoreError> {
        if let Some(error) = self.failure {
            return Err(error);
        }
        match original.and_then(|original| self.lifecycle.observe_original(&original)) {
            Ok(()) => {
                self.observed = true;
                Ok(())
            }
            Err(error) => {
                self.failure = Some(error);
                Err(error)
            }
        }
    }
}

pub(crate) struct SseRestorationState {
    original_evidence: Option<super::restoration_core::SseEvidenceRequirement>,
    original_upstream: UpstreamProtocol,
    completion_frames: sha2::Sha256,
    original_pair: ProtocolPair,
    original_observed: bool,
    pending_publication: Option<super::tokenization::PendingPublication>,
    frame_metadata: Option<super::payload_budget::Reservation>,
    lease: DlpRequestLease,
    binding: ActualAttemptBinding,
    budget: PayloadBudget,
    norm_mode: ArgumentNormalizationMode,
    lifecycle: Option<ResponseLifecycle<BufferedSseEvent>>,
    finished_inspection: Option<FinishedInspection<BufferedSseEvent>>,
    deferred_events: super::restoration_core::DeferredUnits<BufferedSseEvent>,
    raw_snapshots: PayloadVec<PayloadJson>,
    text_parts: PayloadVec<TextPartTrack>,
    tool_items: PayloadVec<ToolItemTrack>,
    completed_seen: bool,
    finalized: bool,
    failed: Option<RestoreError>,
    failure_stage: RestoreStage,
    token_format_diagnostics: TokenFormatDiagnosticState,
    pending_delivered_history: Option<super::history_provenance::PendingDeliveredHistory>,
}

impl SseRestorationState {
    pub(super) fn new_bound(
        lease: DlpRequestLease,
        binding: ActualAttemptBinding,
        budget: PayloadBudget,
        norm_mode: ArgumentNormalizationMode,
        pair: ProtocolPair,
        attempt: super::restoration_core::AttemptProvenance,
    ) -> Result<Self, RestoreError> {
        let inspection =
            ResponsesSseInspection::begin(&lease, &binding, &budget, pair, attempt.clone())?;
        Self::from_inspection(
            lease, binding, budget, norm_mode, pair, &attempt, inspection,
        )
    }

    pub(super) fn from_inspection(
        lease: DlpRequestLease,
        binding: ActualAttemptBinding,
        budget: PayloadBudget,
        norm_mode: ArgumentNormalizationMode,
        pair: ProtocolPair,
        attempt: &super::restoration_core::AttemptProvenance,
        inspection: ResponsesSseInspection,
    ) -> Result<Self, RestoreError> {
        inspection.validate(&binding, pair, attempt)?;
        let observed = inspection.observed;
        let mut lifecycle = inspection.lifecycle;
        let deferred_events = lifecycle.deferred_owner()?;
        Ok(Self {
            original_evidence: None,
            original_upstream: pair.upstream(),
            original_pair: pair,
            original_observed: observed,
            completion_frames: sha2::Digest::new(),
            pending_publication: None,
            frame_metadata: None,
            lease,
            binding,
            budget: budget.clone(),
            norm_mode,
            lifecycle: Some(lifecycle),
            finished_inspection: None,
            deferred_events,
            raw_snapshots: PayloadVec::new(budget.clone()).map_err(RestoreError::scan_error)?,
            text_parts: PayloadVec::new(budget.clone()).map_err(RestoreError::scan_error)?,
            tool_items: PayloadVec::new(budget.clone()).map_err(RestoreError::scan_error)?,
            completed_seen: false,
            finalized: false,
            failed: None,
            failure_stage: RestoreStage::Unknown,
            token_format_diagnostics: TokenFormatDiagnosticState::default(),
            pending_delivered_history: None,
        })
    }

    #[cfg(test)]
    pub(super) fn new(
        lease: DlpRequestLease,
        binding: ActualAttemptBinding,
        budget: PayloadBudget,
        norm_mode: ArgumentNormalizationMode,
    ) -> Result<Self, RestoreError> {
        use crate::proxy::protocol_bridge::Protocol;
        let attempt = super::restoration_core::AttemptProvenance::new(&budget)
            .map_err(RestoreError::capacity)?;
        Self::new_bound(
            lease,
            binding,
            budget,
            norm_mode,
            ProtocolPair::new(
                ClientProtocol::new(Protocol::Responses),
                UpstreamProtocol::new(Protocol::Responses),
            ),
            attempt,
        )
    }

    pub(super) fn observe_original(
        &mut self,
        original: &super::restoration_contract::OriginalResponseObservation<'_>,
    ) -> Result<(), RestoreError> {
        self.owner_mut()?.observe_original(original)?;
        self.original_observed = true;
        Ok(())
    }

    pub(super) fn needs_converted_inspection(&self) -> bool {
        use crate::proxy::protocol_bridge::Protocol;
        self.original_evidence.is_some()
            && (matches!(
                self.original_upstream.protocol(),
                Protocol::Chat | Protocol::Anthropic
            ) || (self.original_upstream.protocol() == Protocol::Responses
                && self.norm_mode == ArgumentNormalizationMode::XaiWholeFloat))
    }

    /// Move the sole mechanics owner to the original observer while upstream is polled.
    pub(super) fn lend_original_inspection(
        &mut self,
    ) -> Result<ResponsesSseInspection, RestoreError> {
        if self.finished_inspection.is_some() || self.failed.is_some() {
            return Err(RestoreError::RestoreFailed);
        }
        let attempt = self
            .completion_attempt()
            .cloned()
            .ok_or(RestoreError::RestoreFailed)?;
        Ok(ResponsesSseInspection {
            lifecycle: self.lifecycle.take().ok_or(RestoreError::RestoreFailed)?,
            binding: self.binding.clone(),
            pair: self.original_pair,
            attempt,
            observed: self.original_observed,
            failure: None,
        })
    }

    pub(super) fn reclaim_original_inspection(
        &mut self,
        inspection: ResponsesSseInspection,
    ) -> Result<(), RestoreError> {
        let validation = (|| {
            if let Some(error) = self.failed {
                return Err(error);
            }
            if self.finalized || self.lifecycle.is_some() || self.finished_inspection.is_some() {
                return Err(RestoreError::RestoreFailed);
            }
            let attempt = self
                .completion_attempt()
                .ok_or(RestoreError::RestoreFailed)?;
            inspection.validate(&self.binding, self.original_pair, attempt)?;
            if !inspection.observed {
                return Err(RestoreError::RestoreFailed);
            }
            Ok(())
        })();
        if let Err(error) = validation {
            self.pending_delivered_history = None;
            self.pending_publication = None;
            self.failed = Some(error);
            return Err(error);
        }
        self.original_observed = true;
        self.lifecycle = Some(inspection.lifecycle);
        Ok(())
    }

    /// Only direct native Responses input is still original at this boundary.
    pub(super) fn observe_native_input(&mut self, block: &str) -> Result<(), RestoreError> {
        use crate::proxy::protocol_bridge::Protocol;
        if self.original_upstream.protocol() != Protocol::Responses
            || self.norm_mode != ArgumentNormalizationMode::Exact
        {
            return Ok(());
        }
        let Some(requirement) = &self.original_evidence else {
            return Ok(());
        };
        let original = super::restoration_contract::OriginalResponseObservation::frame(
            block.as_bytes(),
            self.original_upstream,
            requirement.attempt().clone(),
            self.budget.clone(),
        )?;
        self.observe_original(&original)
    }

    pub(super) fn completion_attempt(&self) -> Option<&super::restoration_core::AttemptProvenance> {
        self.original_evidence
            .as_ref()
            .map(|requirement| requirement.attempt())
    }

    pub(super) fn completion_frame_digest(&self) -> [u8; 32] {
        use sha2::Digest;
        self.completion_frames.clone().finalize().into()
    }

    pub(super) fn require_original_evidence(
        &mut self,
        requirement: super::restoration_core::SseEvidenceRequirement,
    ) {
        self.original_evidence = Some(requirement);
    }

    pub(super) fn validate_original_evidence(
        &self,
        observed: Option<(
            &super::restoration_core::AttemptProvenance,
            super::restoration_core::OriginalSseProtocol,
        )>,
    ) -> Result<(), RestoreError> {
        self.original_evidence
            .as_ref()
            .map_or(Ok(()), |required| required.validate(observed))
    }

    /// Legacy fixtures only; production delivery owns its continuation.
    #[cfg(test)]
    pub(super) fn publish_released_history(&mut self) {
        if self.failed.is_some() || !self.finalized {
            self.pending_delivered_history = None;
            return;
        }
        if let Some(pending) = self.pending_delivered_history.take() {
            let _ = self.lease.publish_delivered_history(&self.binding, pending);
        }
    }

    pub(super) fn failure_stage(&self) -> RestoreStage {
        self.failure_stage
    }

    pub(super) fn payload_budget(&self) -> PayloadBudget {
        self.budget.clone()
    }

    /// Original-to-converted attestation grants no completion authority by itself.
    pub(super) fn verify_original_xai_output(&self, original: &Value) -> Result<(), RestoreError> {
        if self.norm_mode != ArgumentNormalizationMode::XaiWholeFloat {
            return Err(RestoreError::RestoreFailed);
        }
        let original = original.as_array().ok_or(RestoreError::RestoreFailed)?;
        let converted = self
            .held_events()
            .last()
            .filter(|event| event.event_name.as_str() == "response.completed")
            .and_then(|event| event.data.as_value().pointer("/response/output"))
            .and_then(Value::as_array)
            .ok_or(RestoreError::RestoreFailed)?;
        if original.len() != converted.len() {
            return Err(RestoreError::RestoreFailed);
        }
        for (raw, normalized) in original.iter().zip(converted) {
            if raw.get("type").and_then(Value::as_str) != Some("function_call") {
                continue;
            }
            // Names may already have undergone the existing namespace restoration.
            for key in ["type", "id", "call_id"] {
                if raw.get(key) != normalized.get(key) {
                    return Err(RestoreError::RestoreFailed);
                }
            }
            let raw_args = raw
                .get("arguments")
                .and_then(Value::as_str)
                .ok_or(RestoreError::RestoreFailed)?;
            let converted_args = normalized
                .get("arguments")
                .and_then(Value::as_str)
                .ok_or(RestoreError::RestoreFailed)?;
            if !arguments_match_mode(
                raw_args,
                converted_args,
                ArgumentNormalizationMode::XaiWholeFloat,
                &self.budget,
            )? {
                return Err(RestoreError::RestoreFailed);
            }
        }
        Ok(())
    }

    pub(crate) fn restored_occurrences(&self) -> usize {
        self.lifecycle.as_ref().map_or_else(
            || {
                self.finished_inspection
                    .as_ref()
                    .map_or(0, FinishedInspection::restored_occurrences)
            },
            ResponseLifecycle::restored_occurrences,
        )
    }

    fn owner_mut(&mut self) -> Result<&mut ResponseLifecycle<BufferedSseEvent>, RestoreError> {
        self.lifecycle.as_mut().ok_or(RestoreError::RestoreFailed)
    }

    fn held_events(&self) -> &[BufferedSseEvent] {
        if let Some(owner) = &self.lifecycle {
            owner.held_units()
        } else if let Some(inspection) = &self.finished_inspection {
            inspection.held_units()
        } else {
            &[]
        }
    }

    fn text_state(
        &self,
        index: usize,
    ) -> Result<&super::restoration_core::StreamTextState, RestoreError> {
        let track = self
            .text_parts
            .as_slice()
            .get(index)
            .ok_or(RestoreError::RestoreFailed)?;
        if let Some(owner) = &self.lifecycle {
            owner.text_state(&track.text)
        } else {
            self.finished_inspection
                .as_ref()
                .ok_or(RestoreError::RestoreFailed)?
                .text_state(&track.text)
        }
    }

    fn text_part_index(
        &mut self,
        output_index: u64,
        part_index: u64,
        kind: u8,
    ) -> Result<usize, RestoreError> {
        if let Some(index) = self.text_parts.as_slice().iter().position(|part| {
            part.output_index == output_index && part.part_index == part_index && part.kind == kind
        }) {
            return Ok(index);
        }
        if self.text_parts.len() >= MAX_RESPONSE_ITEMS {
            return Err(RestoreError::RestoreFailed);
        }
        let text = self.owner_mut()?.open_text(text_policy(kind)?)?;
        let index = self.text_parts.len();
        self.text_parts
            .push(TextPartTrack {
                output_index,
                item_id: None,
                part_index,
                kind,
                text,
                text_done_seen: false,
                part_done_seen: false,
                item_done_seen: false,
            })
            .map_err(RestoreError::capacity)?;
        Ok(index)
    }

    fn get_or_create_text_part(
        &mut self,
        output_index: u64,
        part_index: u64,
        kind: u8,
    ) -> Result<&mut TextPartTrack, RestoreError> {
        let index = self.text_part_index(output_index, part_index, kind)?;
        self.text_parts
            .get_mut(index)
            .ok_or(RestoreError::RestoreFailed)
    }

    fn snapshot_text(&mut self, index: usize, text: &str) -> Result<PayloadText, RestoreError> {
        let id = &self
            .text_parts
            .as_slice()
            .get(index)
            .ok_or(RestoreError::RestoreFailed)?
            .text;
        let decision = self
            .lifecycle
            .as_mut()
            .ok_or(RestoreError::RestoreFailed)?
            .snapshot(id, text)
            .inspect_err(|error| {
                if *error == RestoreError::RestoreFailed {
                    self.failure_stage = RestoreStage::SseSnapshot;
                }
            })?;
        decision_text(decision, text, &self.budget, MAX_FRAME_BYTES)
    }

    fn get_or_create_tool_item(
        &mut self,
        output_index: u64,
        is_custom: bool,
    ) -> Result<&mut ToolItemTrack, RestoreError> {
        let pos = self
            .tool_items
            .as_slice()
            .iter()
            .position(|t| t.output_index == output_index);
        if let Some(idx) = pos {
            let item = self
                .tool_items
                .get_mut(idx)
                .ok_or(RestoreError::RestoreFailed)?;
            if item.is_custom != is_custom {
                return Err(RestoreError::RestoreFailed);
            }
            return Ok(item);
        }
        if self.tool_items.len() >= MAX_BARRIER_TOOLS {
            return Err(RestoreError::RestoreFailed);
        }
        let idx = self.tool_items.len();
        self.tool_items
            .push(ToolItemTrack {
                output_index,
                is_custom,
                saw_deltas: false,
                raw_deltas: PayloadBuffer::new(self.budget.clone(), MAX_ARGUMENT_BYTES)
                    .map_err(RestoreError::scan_error)?,
                raw_done: None,
                raw_item_done: None,
                restored_payload: None,
                payload_replaced: false,
            })
            .map_err(RestoreError::scan_error)?;
        self.tool_items
            .get_mut(idx)
            .ok_or(RestoreError::RestoreFailed)
    }

    fn push_barrier_event(
        &mut self,
        event_name: &str,
        data: PayloadJson,
        raw_bytes_len: usize,
    ) -> Result<(), RestoreError> {
        let name = PayloadText::try_from_str(event_name, self.budget.clone(), 256)
            .map_err(RestoreError::scan_error)?;
        self.owner_mut()?.hold_native(
            BufferedSseEvent {
                event_name: name,
                data,
                prepared: None,
            },
            raw_bytes_len,
        )
    }

    pub(crate) fn process_sse_block(&mut self, block: &str) -> Result<Vec<Bytes>, RestoreError> {
        if let Some(err) = self.failed {
            return Err(err);
        }
        use sha2::Digest;
        self.completion_frames
            .update((block.len() as u64).to_le_bytes());
        self.completion_frames.update(block.as_bytes());
        let res = if self.finalized {
            self.failure_stage = RestoreStage::SseFrame;
            Err(RestoreError::RestoreFailed)
        } else {
            self.process_sse_block_inner(block)
        };
        if let Err(err) = res {
            self.pending_delivered_history = None;
            self.failed = Some(err);
            if err == RestoreError::FailedBecause(RestoreReason::TokenFormat) {
                // Missing carrier context stays unknown; never inspect unrelated fields.
                self.token_format_diagnostics
                    .observe("", 0, Carrier::Other, Site::Unknown, false);
                self.token_format_diagnostics.emit_once();
            }
            return Err(err);
        }
        res
    }

    fn process_sse_block_inner(&mut self, block: &str) -> Result<Vec<Bytes>, RestoreError> {
        self.failure_stage = RestoreStage::SseFrame;
        if block.len() > MAX_FRAME_BYTES {
            return Err(RestoreError::RestoreFailed);
        }
        if block.trim().is_empty() {
            return Ok(Vec::new());
        }
        let mut event_name: Option<&str> = None;
        let mut data_bytes = 0usize;
        let mut data_count = 0usize;
        for line in block.lines() {
            if line.is_empty() || line.starts_with(':') {
                continue;
            }
            let (name, val) = line.split_once(':').unwrap_or((line, ""));
            let val = val.strip_prefix(' ').unwrap_or(val);
            match name {
                "event" if event_name.is_none() => event_name = Some(val.trim()),
                "data" => {
                    data_bytes = data_bytes
                        .checked_add(val.len())
                        .and_then(|n| n.checked_add(usize::from(data_count != 0)))
                        .ok_or(RestoreError::RestoreFailed)?;
                    data_count += 1;
                }
                "id" | "retry" => {}
                _ => return Err(RestoreError::RestoreFailed),
            }
        }
        if data_count == 0 {
            return if event_name.is_none() {
                Ok(Vec::new())
            } else {
                Err(RestoreError::RestoreFailed)
            };
        }
        let mut data_buf =
            PayloadBuffer::with_capacity(data_bytes, self.budget.clone(), MAX_FRAME_BYTES)
                .map_err(RestoreError::scan_error)?;
        let mut first = true;
        for line in block.lines() {
            let trimmed = line.trim_start();
            if let Some(val) = trimmed
                .strip_prefix("data:")
                .or_else(|| (trimmed == "data").then_some(""))
            {
                if !first {
                    data_buf.append(b"\n").map_err(RestoreError::scan_error)?;
                }
                first = false;
                data_buf
                    .append(val.strip_prefix(' ').unwrap_or(val).as_bytes())
                    .map_err(RestoreError::scan_error)?;
            }
        }
        let data_str =
            std::str::from_utf8(data_buf.as_slice()).map_err(|_| RestoreError::RestoreFailed)?;
        if data_str.trim() == "[DONE]" {
            return Ok(Vec::new());
        }
        if self.completed_seen {
            return Err(RestoreError::RestoreFailed);
        }
        let parsed = PayloadJson::try_from_str(data_str, self.budget.clone(), self.budget.limit())
            .map_err(RestoreError::scan_error)?;
        self.failure_stage = RestoreStage::SseEvent;
        let data = parsed.as_value();
        // This copy outlives the borrow when parsed moves into a held event.
        // Keep its decoded UTF-8 allocation charged independently of the JSON.
        let kind = PayloadText::try_from_str(
            data.get("type")
                .and_then(Value::as_str)
                .ok_or(RestoreError::RestoreFailed)?,
            self.budget.clone(),
            MAX_FRAME_BYTES,
        )
        .map_err(RestoreError::capacity)?;
        let evt = event_name.unwrap_or(kind.as_str());
        self.token_format_diagnostics.record_event(
            evt,
            self.lifecycle
                .as_ref()
                .is_some_and(ResponseLifecycle::barrier_is_active),
            block.len(),
        );
        if evt != kind.as_str() {
            return Err(RestoreError::RestoreFailed);
        }
        if evt == "response.failed"
            || evt == "response.incomplete"
            || evt == "error"
            || data.get("error").is_some_and(|v| !v.is_null())
            || data
                .pointer("/response/error")
                .is_some_and(|v| !v.is_null())
        {
            return Err(RestoreError::RestoreFailed);
        }
        if evt == "response.completed"
            && (!super::completion_evidence::NativeCompletionEvent::new(evt, data)
                .permits_completion()
                || data
                    .pointer("/response/incomplete_details")
                    .is_some_and(|value| !value.is_null()))
        {
            return Err(RestoreError::RestoreFailed);
        }

        // Match native JSON completion semantics without rejecting added/in-progress items.
        let completed_items = match evt {
            "response.output_item.done" => {
                std::slice::from_ref(data.get("item").ok_or(RestoreError::RestoreFailed)?)
            }
            "response.completed" => data
                .pointer("/response/output")
                .and_then(Value::as_array)
                .ok_or(RestoreError::RestoreFailed)?
                .as_slice(),
            _ => &[],
        };
        for item in completed_items {
            if item
                .get("status")
                .and_then(Value::as_str)
                .is_some_and(|status| status != "completed")
            {
                return Err(RestoreError::RestoreFailed);
            }
        }

        self.failure_stage = RestoreStage::SseSnapshot;
        if is_raw_snapshot_event(evt) {
            check_snapshot_carriers(data, &self.lease)?;
        }
        let text_identity = match evt {
            "response.output_text.delta" | "response.output_text.done" => {
                Some(("content_index", 1))
            }
            "response.refusal.delta" | "response.refusal.done" => Some(("content_index", 2)),
            "response.reasoning_summary_text.delta" | "response.reasoning_summary_text.done" => {
                Some(("summary_index", 3))
            }
            "response.reasoning_text.delta" | "response.reasoning_text.done" => {
                Some(("content_index", 4))
            }
            _ => None,
        };
        if let Some((part_key, kind)) = text_identity {
            if let Some(id) = data.get("item_id") {
                let id = id
                    .as_str()
                    .filter(|id| valid_external_id(id))
                    .ok_or(RestoreError::RestoreFailed)?;
                let budget = self.budget.clone();
                let track = self.get_or_create_text_part(
                    data.get("output_index")
                        .and_then(Value::as_u64)
                        .unwrap_or(0),
                    data.get(part_key).and_then(Value::as_u64).unwrap_or(0),
                    kind,
                )?;
                match &track.item_id {
                    Some(prior) if prior.as_str() != id => return Err(RestoreError::RestoreFailed),
                    None => {
                        track.item_id = Some(
                            PayloadText::try_from_str(id, budget, 256)
                                .map_err(RestoreError::scan_error)?,
                        )
                    }
                    _ => {}
                }
            }
        }

        // Check if this event triggers or belongs to the tool commit barrier.
        let is_tool_event = matches!(
            evt,
            "response.function_call_arguments.delta"
                | "response.function_call_arguments.done"
                | "response.custom_tool_call_input.delta"
                | "response.custom_tool_call_input.done"
        ) || (matches!(
            evt,
            "response.output_item.added" | "response.output_item.done"
        ) && matches!(
            data.pointer("/item/type").and_then(Value::as_str),
            Some("function_call" | "custom_tool_call")
        ));

        if is_tool_event
            || evt == "response.completed"
            || self
                .lifecycle
                .as_ref()
                .is_some_and(ResponseLifecycle::barrier_is_active)
        {
            if self.deferred_events.len() != 0 {
                // Deferred text retains its checked donor lineage and original ordering.
                self.lifecycle
                    .as_mut()
                    .ok_or(RestoreError::RestoreFailed)?
                    .adopt_deferred(&mut self.deferred_events)?;
            }
            self.record_raw_event_state(evt, data)?;
            if evt == "response.completed" {
                self.completed_seen = true;
            }
            self.push_barrier_event(evt, parsed, block.len())?;
            return Ok(Vec::new());
        }

        // Retain original pre-barrier snapshots before any field is rewritten.
        // Post-barrier snapshots already have a charged owner in barrier_events.
        if is_raw_snapshot_event(evt) {
            if self.raw_snapshots.len() >= MAX_RESPONSE_ITEMS {
                return Err(RestoreError::RestoreFailed);
            }
            self.raw_snapshots
                .push(
                    PayloadJson::try_from_value(data, self.budget.clone(), self.budget.limit())
                        .map_err(RestoreError::scan_error)?,
                )
                .map_err(RestoreError::scan_error)?;
        }

        // Pre-barrier streaming path for ordinary text/reasoning events.
        if matches!(
            evt,
            "response.output_text.done"
                | "response.refusal.done"
                | "response.reasoning_summary_text.done"
                | "response.reasoning_text.done"
        ) {
            self.flush_ordinary_tail(evt, data)?;
        }
        let mut replacements =
            PayloadVec::new(self.budget.clone()).map_err(RestoreError::scan_error)?;
        self.failure_stage = RestoreStage::SseEvent;
        self.handle_pre_barrier_event(evt, data, &mut replacements)?;
        let rewritten = apply_text_replacements(data, &replacements, &self.budget)?;
        if matches!(
            evt,
            "response.content_part.done" | "response.reasoning_summary_part.done"
        ) {
            check_restored_event(evt, rewritten.as_value(), &self.lease)?;
        }
        let mut has_short_tail = false;
        for index in 0..self.text_parts.len() {
            has_short_tail |= !self.text_state(index)?.pending_tail.as_slice().is_empty();
        }
        if self.deferred_events.len() != 0 || has_short_tail {
            let mut edits =
                PayloadVec::new(self.budget.clone()).map_err(RestoreError::scan_error)?;
            copy_string_edits(data, rewritten.as_value(), &mut edits)?;
            let restored = PayloadJson::try_with_replacements(
                data,
                edits.as_slice(),
                self.budget.clone(),
                self.budget.limit(),
            )
            .map_err(RestoreError::scan_error)?;
            // Keep both versions charged: raw events are needed if a tool starts before resolution.
            self.deferred_events
                .push(
                    BufferedSseEvent {
                        event_name: PayloadText::try_from_str(evt, self.budget.clone(), 256)
                            .map_err(RestoreError::scan_error)?,
                        data: parsed,
                        prepared: Some(restored),
                    },
                    block.len(),
                )
                .map_err(RestoreError::scan_error)?;
            if has_short_tail {
                return Ok(Vec::new());
            }
            return self.flush_deferred_text();
        }
        self.mark_text_released(evt, rewritten.as_value())?;
        Ok(vec![encode_sse_frame(
            evt,
            rewritten.as_value(),
            &self.budget,
        )?])
    }
}

struct ChargedSseBytes(PayloadBuffer);

impl AsRef<[u8]> for ChargedSseBytes {
    fn as_ref(&self) -> &[u8] {
        self.0.as_slice()
    }
}

fn encode_sse_frame(
    event_name: &str,
    data: &Value,
    budget: &PayloadBudget,
) -> Result<Bytes, RestoreError> {
    let json_buf = serialize_json_with_budget(data, budget, MAX_FRAME_BYTES)?;
    let total_len = 7usize
        .checked_add(event_name.len())
        .and_then(|n| n.checked_add(7))
        .and_then(|n| n.checked_add(json_buf.len()))
        .and_then(|n| n.checked_add(2))
        .ok_or(RestoreError::RestoreFailed)?;
    let mut frame = PayloadBuffer::with_capacity(total_len, budget.clone(), MAX_FRAME_BYTES * 2)
        .map_err(RestoreError::scan_error)?;
    frame
        .append(b"event: ")
        .and_then(|_| frame.append(event_name.as_bytes()))
        .and_then(|_| frame.append(b"\ndata: "))
        .and_then(|_| frame.append(json_buf.as_slice()))
        .and_then(|_| frame.append(b"\n\n"))
        .map_err(RestoreError::scan_error)?;
    Ok(Bytes::from_owner(ChargedSseBytes(frame)))
}

pub(crate) fn failed_sse_frame(err: RestoreError) -> Bytes {
    failed_sse_frame_at(err, RestoreStage::Unknown)
}

pub(super) fn failed_sse_frame_at(err: RestoreError, stage: RestoreStage) -> Bytes {
    super::restore_diagnostics::log_failure_with_reason(err.code(), stage, err.reason_at(stage));
    let payload = json!({
        "type": "response.failed",
        "response": {
            "id": "resp_proxy_failed",
            "object": "response",
            "status": "failed",
            "error": {
                "message": super::http_restoration::CLIENT_PROCESSING_ERROR_MESSAGE,
                "type": "server_error",
                "code": super::http_restoration::CLIENT_PROCESSING_ERROR_CODE,
                "param": Value::Null,
            }
        }
    });
    let encoded = serde_json::to_string(&payload).unwrap_or_else(|_| {
        "{\"type\":\"response.failed\",\"response\":{\"status\":\"failed\"}}".to_string()
    });
    Bytes::from(format!("event: response.failed\ndata: {encoded}\n\n"))
}

fn text_event_key(evt: &str, data: &Value) -> (u64, u64, u8) {
    let (key, kind) = if evt.contains("reasoning_summary") {
        ("summary_index", 3)
    } else if evt == "response.reasoning_text.delta" || evt == "response.reasoning_text.done" {
        ("content_index", 4)
    } else if evt.contains("refusal") {
        ("content_index", 2)
    } else {
        ("content_index", 1)
    };
    (
        data.get("output_index")
            .and_then(Value::as_u64)
            .unwrap_or(0),
        data.get(key).and_then(Value::as_u64).unwrap_or(0),
        kind,
    )
}

impl SseRestorationState {
    fn mark_text_released(&mut self, evt: &str, data: &Value) -> Result<(), RestoreError> {
        if matches!(
            evt,
            "response.output_text.delta"
                | "response.refusal.delta"
                | "response.reasoning_summary_text.delta"
                | "response.reasoning_text.delta"
        ) {
            let (out, part, kind) = text_event_key(evt, data);
            let index = self.text_part_index(out, part, kind)?;
            let id = &self.text_parts.as_slice()[index].text;
            self.lifecycle
                .as_mut()
                .ok_or(RestoreError::RestoreFailed)?
                .mark_text_released(id)?;
        }
        Ok(())
    }

    fn flush_text_tail(&mut self, out: u64, part: u64, kind: u8) -> Result<(), RestoreError> {
        let index = self.text_part_index(out, part, kind)?;
        let id = &self.text_parts.as_slice()[index].text;
        self.lifecycle
            .as_mut()
            .ok_or(RestoreError::RestoreFailed)?
            .flush_text_tail(id)
    }

    fn flush_ordinary_tail(&mut self, evt: &str, data: &Value) -> Result<(), RestoreError> {
        let (out, part, kind) = text_event_key(evt, data);
        self.flush_text_tail(out, part, kind)
    }

    fn flush_deferred_text(&mut self) -> Result<Vec<Bytes>, RestoreError> {
        let mut emitted = PayloadVec::new(self.budget.clone()).map_err(RestoreError::scan_error)?;
        let mut frames = Vec::new();
        for event in self.deferred_events.units() {
            let evt = event.event_name.as_str();
            let prepared = event.prepared.as_ref().ok_or(RestoreError::RestoreFailed)?;
            let data = prepared.as_value();
            if matches!(
                evt,
                "response.output_text.delta"
                    | "response.refusal.delta"
                    | "response.reasoning_summary_text.delta"
                    | "response.reasoning_text.delta"
            ) {
                let key = text_event_key(evt, data);
                let index = self
                    .text_parts
                    .as_slice()
                    .iter()
                    .position(|p| (p.output_index, p.part_index, p.kind) == key)
                    .ok_or(RestoreError::RestoreFailed)?;
                let track = self.text_state(index)?;
                let text = if emitted.as_slice().contains(&key) {
                    ""
                } else {
                    emitted.push(key).map_err(RestoreError::scan_error)?;
                    std::str::from_utf8(
                        track
                            .restored_deltas
                            .as_slice()
                            .get(track.released_deltas_len..)
                            .ok_or(RestoreError::RestoreFailed)?,
                    )
                    .map_err(|_| RestoreError::RestoreFailed)?
                };
                let replay = PayloadJson::try_with_replacements(
                    data,
                    &[JsonStringReplacement {
                        target: data.get("delta").ok_or(RestoreError::RestoreFailed)?,
                        text,
                    }],
                    self.budget.clone(),
                    self.budget.limit(),
                )
                .map_err(RestoreError::scan_error)?;
                frames.push(encode_sse_frame(evt, replay.as_value(), &self.budget)?);
            } else {
                frames.push(encode_sse_frame(evt, data, &self.budget)?);
            }
        }
        for key in emitted.as_slice() {
            let index = self.text_part_index(key.0, key.1, key.2)?;
            let id = &self.text_parts.as_slice()[index].text;
            self.lifecycle
                .as_mut()
                .ok_or(RestoreError::RestoreFailed)?
                .mark_text_released(id)?;
        }
        self.deferred_events.clear();
        Ok(frames)
    }

    fn record_raw_event_state(&mut self, evt: &str, data: &Value) -> Result<(), RestoreError> {
        let out_idx = data
            .get("output_index")
            .and_then(Value::as_u64)
            .unwrap_or(0);
        match evt {
            "response.output_text.delta"
            | "response.refusal.delta"
            | "response.reasoning_summary_text.delta"
            | "response.reasoning_text.delta"
            | "response.output_text.done"
            | "response.refusal.done"
            | "response.reasoning_summary_text.done"
            | "response.reasoning_text.done"
            | "response.content_part.done"
            | "response.reasoning_summary_part.done"
            | "response.content_part.added"
            | "response.reasoning_summary_part.added" => {
                let mut ignored =
                    PayloadVec::new(self.budget.clone()).map_err(RestoreError::capacity)?;
                self.handle_pre_barrier_event(evt, data, &mut ignored)?;
            }
            "response.function_call_arguments.delta" | "response.custom_tool_call_input.delta" => {
                let is_custom = evt == "response.custom_tool_call_input.delta";
                let delta = data
                    .get("delta")
                    .and_then(Value::as_str)
                    .ok_or(RestoreError::RestoreFailed)?;
                let track = self.get_or_create_tool_item(out_idx, is_custom)?;
                if track.raw_done.is_some() {
                    return Err(RestoreError::RestoreFailed);
                }
                track.saw_deltas = true;
                track
                    .raw_deltas
                    .append(delta.as_bytes())
                    .map_err(RestoreError::scan_error)?;
            }
            "response.function_call_arguments.done" | "response.custom_tool_call_input.done" => {
                let is_custom = evt == "response.custom_tool_call_input.done";
                let key = if is_custom { "input" } else { "arguments" };
                let text = data
                    .get(key)
                    .and_then(Value::as_str)
                    .ok_or(RestoreError::RestoreFailed)?;
                let budget = self.budget.clone();
                let track = self.get_or_create_tool_item(out_idx, is_custom)?;
                if track.raw_done.is_some() {
                    return Err(RestoreError::RestoreFailed);
                }
                track.raw_done = Some(
                    PayloadText::try_from_str(text, budget, MAX_ARGUMENT_BYTES)
                        .map_err(RestoreError::scan_error)?,
                );
            }
            "response.output_item.added" => {
                let item = data.get("item").ok_or(RestoreError::RestoreFailed)?;
                match item.get("type").and_then(Value::as_str) {
                    Some("message" | "reasoning") => {
                        let mut ignored =
                            PayloadVec::new(self.budget.clone()).map_err(RestoreError::capacity)?;
                        self.handle_pre_barrier_event(evt, data, &mut ignored)?;
                    }
                    Some("function_call") => {
                        let _ = self.get_or_create_tool_item(out_idx, false)?;
                    }
                    Some("custom_tool_call") => {
                        let _ = self.get_or_create_tool_item(out_idx, true)?;
                    }
                    _ => {}
                }
            }
            "response.output_item.done" => {
                let item = data.get("item").ok_or(RestoreError::RestoreFailed)?;
                match item.get("type").and_then(Value::as_str) {
                    Some("message" | "reasoning") => {
                        let mut ignored =
                            PayloadVec::new(self.budget.clone()).map_err(RestoreError::capacity)?;
                        self.handle_pre_barrier_event(evt, data, &mut ignored)?;
                    }
                    Some("function_call") => {
                        let args = item
                            .get("arguments")
                            .and_then(Value::as_str)
                            .ok_or(RestoreError::RestoreFailed)?;
                        let budget = self.budget.clone();
                        let track = self.get_or_create_tool_item(out_idx, false)?;
                        if track.raw_item_done.is_some() {
                            return Err(RestoreError::RestoreFailed);
                        }
                        track.raw_item_done = Some(
                            PayloadText::try_from_str(args, budget, MAX_ARGUMENT_BYTES)
                                .map_err(RestoreError::scan_error)?,
                        );
                    }
                    Some("custom_tool_call") => {
                        let input = item
                            .get("input")
                            .and_then(Value::as_str)
                            .ok_or(RestoreError::RestoreFailed)?;
                        let budget = self.budget.clone();
                        let track = self.get_or_create_tool_item(out_idx, true)?;
                        if track.raw_item_done.is_some() {
                            return Err(RestoreError::RestoreFailed);
                        }
                        if let Some(done) = &track.raw_done {
                            if done.as_str() != input {
                                return Err(RestoreError::RestoreFailed);
                            }
                        }
                        track.raw_item_done = Some(
                            PayloadText::try_from_str(input, budget, MAX_ARGUMENT_BYTES)
                                .map_err(RestoreError::scan_error)?,
                        );
                    }
                    _ => {}
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn handle_pre_barrier_event<'a>(
        &mut self,
        evt: &str,
        data: &'a Value,
        replacements: &mut TextReplacements<'a>,
    ) -> Result<(), RestoreError> {
        let out = data
            .get("output_index")
            .and_then(Value::as_u64)
            .unwrap_or(0);
        match evt {
            "response.output_text.delta"
            | "response.refusal.delta"
            | "response.reasoning_summary_text.delta"
            | "response.reasoning_text.delta"
            | "response.output_text.done"
            | "response.refusal.done"
            | "response.reasoning_summary_text.done"
            | "response.reasoning_text.done" => {
                let (_, part, kind) = text_event_key(evt, data);
                let field = if evt.ends_with(".delta") {
                    "delta"
                } else if kind == 2 {
                    "refusal"
                } else {
                    "text"
                };
                for (key, value) in data.as_object().ok_or(RestoreError::RestoreFailed)? {
                    check_no_tokens_in_str(key, &self.lease)?;
                    if key != field {
                        check_no_tokens_in_value(value, &self.lease)?;
                    }
                }
                let target = data.get(field).ok_or(RestoreError::RestoreFailed)?;
                let text = target.as_str().ok_or(RestoreError::RestoreFailed)?;
                let index = self.text_part_index(out, part, kind)?;
                if self.text_parts.as_slice()[index].text_done_seen {
                    return Err(RestoreError::RestoreFailed);
                }
                self.failure_stage = RestoreStage::TokenRestore;
                let restored = if evt.ends_with(".delta") {
                    let pending = self.text_state(index)?.pending_tail.len();
                    let id = &self.text_parts.as_slice()[index].text;
                    let decision = self
                        .lifecycle
                        .as_mut()
                        .ok_or(RestoreError::RestoreFailed)?
                        .delta(id, text);
                    let decision = decision.inspect_err(|error| {
                        if *error == RestoreError::FailedBecause(RestoreReason::TokenFormat) {
                            self.token_format_diagnostics.observe(
                                text,
                                pending,
                                text_carrier(kind),
                                Site::DeltaRestore,
                                false,
                            );
                        }
                    })?;
                    let pending_after = self.text_state(index)?.pending_tail.len();
                    self.token_format_diagnostics.record_delta_structure(
                        pending,
                        pending_after,
                        text.len(),
                    );
                    decision_text(decision, text, &self.budget, MAX_FRAME_BYTES)?
                } else {
                    self.flush_text_tail(out, part, kind)?;
                    let restored = self.snapshot_text(index, text).inspect_err(|error| {
                        if *error == RestoreError::FailedBecause(RestoreReason::TokenFormat) {
                            self.token_format_diagnostics.observe(
                                text,
                                0,
                                text_carrier(kind),
                                Site::DoneRestore,
                                false,
                            );
                        }
                    })?;
                    self.text_parts
                        .get_mut(index)
                        .ok_or(RestoreError::RestoreFailed)?
                        .text_done_seen = true;
                    restored
                };
                replacements
                    .push((target, restored))
                    .map_err(RestoreError::capacity)?;
            }
            "response.content_part.done"
            | "response.reasoning_summary_part.done"
            | "response.content_part.added"
            | "response.reasoning_summary_part.added" => {
                let part = data.get("part").ok_or(RestoreError::RestoreFailed)?;
                let (kind, field) = text_part_kind(part)?;
                if evt.contains("reasoning_summary") != (kind == 3) {
                    return Err(RestoreError::RestoreFailed);
                }
                let part_index = data
                    .get(if kind == 3 {
                        "summary_index"
                    } else {
                        "content_index"
                    })
                    .and_then(Value::as_u64)
                    .unwrap_or(0);
                let Some(target) = part.get(field) else {
                    if evt.ends_with(".added") {
                        return Ok(());
                    }
                    return Err(RestoreError::RestoreFailed);
                };
                let text = target.as_str().ok_or(RestoreError::RestoreFailed)?;
                if evt.ends_with(".added") && text.is_empty() {
                    return Ok(());
                }
                let index = self.text_part_index(out, part_index, kind)?;
                if evt.ends_with(".done") {
                    if self.text_parts.as_slice()[index].part_done_seen {
                        return Err(RestoreError::RestoreFailed);
                    }
                    self.flush_text_tail(out, part_index, kind)?;
                    self.text_parts
                        .get_mut(index)
                        .ok_or(RestoreError::RestoreFailed)?
                        .part_done_seen = true;
                }
                let restored = self.snapshot_text(index, text)?;
                replacements
                    .push((target, restored))
                    .map_err(RestoreError::capacity)?;
            }
            "response.output_item.done" | "response.output_item.added" => {
                let item = data.get("item").ok_or(RestoreError::RestoreFailed)?;
                let kind = item
                    .get("type")
                    .and_then(Value::as_str)
                    .ok_or(RestoreError::RestoreFailed)?;
                let fields: &[&str] = match kind {
                    "message" => &["content"],
                    "reasoning" => {
                        check_reasoning_item(item, &self.lease)?;
                        &["summary", "content"]
                    }
                    _ => return Err(RestoreError::RestoreFailed),
                };
                for field in fields {
                    let Some(parts) = item.get(*field) else {
                        if kind == "message" && evt.ends_with(".done") {
                            return Err(RestoreError::RestoreFailed);
                        }
                        continue;
                    };
                    for (part_index, part) in parts
                        .as_array()
                        .ok_or(RestoreError::RestoreFailed)?
                        .iter()
                        .enumerate()
                    {
                        let (part_kind, key) = text_part_kind(part)?;
                        if (kind == "reasoning") != matches!(part_kind, 3 | 4)
                            || (*field == "summary") != (part_kind == 3)
                        {
                            return Err(RestoreError::RestoreFailed);
                        }
                        let Some(target) = part.get(key) else {
                            if evt.ends_with(".added") {
                                continue;
                            }
                            return Err(RestoreError::RestoreFailed);
                        };
                        let text = target.as_str().ok_or(RestoreError::RestoreFailed)?;
                        if evt.ends_with(".added") && text.is_empty() {
                            continue;
                        }
                        let index = self.text_part_index(out, part_index as u64, part_kind)?;
                        if evt.ends_with(".done") {
                            if self.text_parts.as_slice()[index].item_done_seen {
                                return Err(RestoreError::RestoreFailed);
                            }
                            self.flush_text_tail(out, part_index as u64, part_kind)?;
                            self.text_parts
                                .get_mut(index)
                                .ok_or(RestoreError::RestoreFailed)?
                                .item_done_seen = true;
                        }
                        let restored = self.snapshot_text(index, text)?;
                        replacements
                            .push((target, restored))
                            .map_err(RestoreError::capacity)?;
                    }
                }
            }
            _ => check_no_tokens_in_value(data, &self.lease)?,
        }
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn finalize_and_flush(
        &mut self,
        completion_permitted: bool,
    ) -> Result<Vec<Bytes>, RestoreError> {
        if let Some(err) = self.failed {
            return Err(err);
        }
        // Permission must be checked before the successful-reentry shortcut.
        let res = if !completion_permitted {
            self.failure_stage = RestoreStage::CompletionEvidence;
            Err(RestoreError::RestoreFailed)
        } else if self.finalized {
            return Ok(Vec::new());
        } else {
            let permit = finalization::fixture_permit(&self.budget)?;
            self.finalize_and_flush_inner(&permit)
        };
        if let Err(err) = res {
            self.pending_delivered_history = None;
            self.failed = Some(err);
            if err == RestoreError::FailedBecause(RestoreReason::TokenFormat) {
                // Missing carrier context stays unknown; never inspect unrelated fields.
                self.token_format_diagnostics
                    .observe("", 0, Carrier::Other, Site::Unknown, false);
                self.token_format_diagnostics.emit_once();
            }
            return Err(err);
        }
        self.finalized = true;
        if let Some(pending) = self.pending_publication.take() {
            finalization::activate_fixture(pending)?;
        }
        res
    }

    fn finalize_and_flush_inner(
        &mut self,
        permit: &finalization::FinalizationPermit,
    ) -> Result<Vec<Bytes>, RestoreError> {
        self.failure_stage = RestoreStage::CompletionEvidence;
        if !self.completed_seen {
            return Err(RestoreError::RestoreFailed);
        }

        self.failure_stage = RestoreStage::SseSnapshot;
        // Pending text is reconciled with the raw completed snapshot below.
        // Literal tails never stand in for completion evidence.

        // Locate the final `response.completed` event in `barrier_events`.
        let completed_pos = self
            .held_events()
            .iter()
            .position(|e| e.event_name.as_str() == "response.completed")
            .ok_or(RestoreError::RestoreFailed)?;
        if completed_pos + 1 != self.held_events().len() {
            return Err(RestoreError::RestoreFailed);
        }

        // Validate all tracked text parts and tool items against `response.completed`.
        let response_owner = PayloadJson::try_from_value(
            self.held_events()[completed_pos]
                .data
                .as_value()
                .get("response")
                .ok_or(RestoreError::RestoreFailed)?,
            self.budget.clone(),
            self.budget.limit(),
        )
        .map_err(RestoreError::scan_error)?;
        let response_val = response_owner.as_value();
        let output_arr = response_val
            .get("output")
            .and_then(Value::as_array)
            .ok_or(RestoreError::RestoreFailed)?;

        for raw in self.raw_snapshots.as_slice() {
            validate_raw_snapshot(raw.as_value(), output_arr, self.norm_mode, &self.budget)?;
        }
        for event in self.held_events() {
            let data = event.data.as_value();
            let evt = event.event_name.as_str();
            if is_raw_snapshot_event(evt) {
                validate_raw_snapshot(data, output_arr, self.norm_mode, &self.budget)?;
            }
            if matches!(
                evt,
                "response.output_item.added" | "response.output_item.done"
            ) {
                let index = data
                    .get("output_index")
                    .and_then(Value::as_u64)
                    .ok_or(RestoreError::RestoreFailed)?;
                let raw = data.get("item").ok_or(RestoreError::RestoreFailed)?;
                let final_item = usize::try_from(index)
                    .ok()
                    .and_then(|i| output_arr.get(i))
                    .ok_or(RestoreError::RestoreFailed)?;
                for key in ["id", "call_id", "name", "type", "namespace"] {
                    if raw.get(key) != final_item.get(key) {
                        return Err(RestoreError::RestoreFailed);
                    }
                }
            } else if matches!(
                evt,
                "response.function_call_arguments.delta"
                    | "response.function_call_arguments.done"
                    | "response.custom_tool_call_input.delta"
                    | "response.custom_tool_call_input.done"
            ) {
                let index = data
                    .get("output_index")
                    .and_then(Value::as_u64)
                    .ok_or(RestoreError::RestoreFailed)?;
                let final_item = usize::try_from(index)
                    .ok()
                    .and_then(|i| output_arr.get(i))
                    .ok_or(RestoreError::RestoreFailed)?;
                let expected = if evt.starts_with("response.custom_tool_call_input.") {
                    "custom_tool_call"
                } else {
                    "function_call"
                };
                if final_item.get("type").and_then(Value::as_str) != Some(expected)
                    || data
                        .get("item_id")
                        .is_some_and(|id| Some(id) != final_item.get("id"))
                {
                    return Err(RestoreError::RestoreFailed);
                }
            }
        }

        for (out_idx, item) in output_arr.iter().enumerate() {
            let out_idx = out_idx as u64;
            match item.get("type").and_then(Value::as_str) {
                Some("message" | "reasoning") => {
                    let reasoning = item.get("type").and_then(Value::as_str) == Some("reasoning");
                    if reasoning {
                        check_reasoning_item(item, &self.lease)?;
                    }
                    let fields: &[&str] = if reasoning {
                        &["summary", "content"]
                    } else {
                        &["content"]
                    };
                    for field in fields {
                        let Some(parts) = item.get(*field) else {
                            if !reasoning {
                                return Err(RestoreError::RestoreFailed);
                            }
                            continue;
                        };
                        for (part_index, part) in parts
                            .as_array()
                            .ok_or(RestoreError::RestoreFailed)?
                            .iter()
                            .enumerate()
                        {
                            let (kind, key) = text_part_kind(part)?;
                            if reasoning != matches!(kind, 3 | 4)
                                || (*field == "summary") != (kind == 3)
                            {
                                return Err(RestoreError::RestoreFailed);
                            }
                            let text = part
                                .get(key)
                                .and_then(Value::as_str)
                                .ok_or(RestoreError::RestoreFailed)?;
                            let index = self.text_part_index(out_idx, part_index as u64, kind)?;
                            self.failure_stage = RestoreStage::TokenRestore;
                            let id = &self.text_parts.as_slice()[index].text;
                            drop(
                                self.lifecycle
                                    .as_mut()
                                    .ok_or(RestoreError::RestoreFailed)?
                                    .done(id, text)?,
                            );
                        }
                    }
                }
                Some("function_call") => {
                    let final_raw = item
                        .get("arguments")
                        .and_then(Value::as_str)
                        .ok_or(RestoreError::RestoreFailed)?;
                    let budget = self.budget.clone();
                    let norm_mode = self.norm_mode;
                    {
                        let track = self.get_or_create_tool_item(out_idx, false)?;
                        if let Some(done) = &track.raw_done {
                            if !arguments_match_mode(done.as_str(), final_raw, norm_mode, &budget)?
                            {
                                return Err(RestoreError::RestoreFailed);
                            }
                        }
                        if let Some(item_done) = &track.raw_item_done {
                            if !arguments_match_mode(
                                item_done.as_str(),
                                final_raw,
                                norm_mode,
                                &budget,
                            )? {
                                return Err(RestoreError::RestoreFailed);
                            }
                        }
                        if track.saw_deltas {
                            let raw_deltas_str = std::str::from_utf8(track.raw_deltas.as_slice())
                                .map_err(|_| RestoreError::RestoreFailed)?;
                            if !arguments_match_mode(raw_deltas_str, final_raw, norm_mode, &budget)?
                            {
                                return Err(RestoreError::RestoreFailed);
                            }
                        }
                    }
                    let before = self.restored_occurrences();
                    self.failure_stage = RestoreStage::TokenRestore;
                    let restored = self.owner_mut()?.arguments(final_raw).inspect_err(|err| {
                        if *err == RestoreError::FailedBecause(RestoreReason::TokenFormat) {
                            // Do not rescan encoded JSON or guess which decoded leaf failed.
                            self.token_format_diagnostics.observe(
                                "",
                                0,
                                Carrier::FunctionArguments,
                                Site::FinalToolRestore,
                                false,
                            );
                        }
                    })?;
                    self.failure_stage = RestoreStage::SseSnapshot;
                    let replaced = self.restored_occurrences() != before;
                    let track = self.get_or_create_tool_item(out_idx, false)?;
                    track.payload_replaced = replaced;
                    track.restored_payload = Some(restored);
                }
                Some("custom_tool_call") => {
                    let final_raw = item
                        .get("input")
                        .and_then(Value::as_str)
                        .ok_or(RestoreError::RestoreFailed)?;
                    let budget = self.budget.clone();
                    {
                        let track = self.get_or_create_tool_item(out_idx, true)?;
                        if let Some(done) = &track.raw_done {
                            if done.as_str() != final_raw {
                                return Err(RestoreError::RestoreFailed);
                            }
                        }
                        if let Some(item_done) = &track.raw_item_done {
                            if item_done.as_str() != final_raw {
                                return Err(RestoreError::RestoreFailed);
                            }
                        }
                        if track.saw_deltas && track.raw_deltas.as_slice() != final_raw.as_bytes() {
                            return Err(RestoreError::RestoreFailed);
                        }
                    }
                    let before = self.restored_occurrences();
                    self.failure_stage = RestoreStage::TokenRestore;
                    let id = self.owner_mut()?.open_text(TextPolicy::ToolArguments)?;
                    let decision = self.owner_mut()?.done(&id, final_raw)?;
                    let restored = decision_text(decision, final_raw, &budget, MAX_ARGUMENT_BYTES)
                        .inspect_err(|err| {
                            if *err == RestoreError::FailedBecause(RestoreReason::TokenFormat) {
                                self.token_format_diagnostics.observe(
                                    final_raw,
                                    0,
                                    Carrier::CustomInput,
                                    Site::FinalToolRestore,
                                    false,
                                );
                            }
                        })?;
                    self.failure_stage = RestoreStage::SseSnapshot;
                    let replaced = self.restored_occurrences() != before;
                    let track = self.get_or_create_tool_item(out_idx, true)?;
                    track.payload_replaced = replaced;
                    track.restored_payload = Some(restored);
                }
                _ => return Err(RestoreError::RestoreFailed),
            }
        }

        // Every tracked text part and tool item must have been matched in `response.completed`.
        for part in self.text_parts.as_slice() {
            let item = usize::try_from(part.output_index)
                .ok()
                .and_then(|n| output_arr.get(n))
                .ok_or(RestoreError::RestoreFailed)?;
            let (kind, field) = match part.kind {
                3 => ("reasoning", "summary"),
                4 => ("reasoning", "content"),
                _ => ("message", "content"),
            };
            let final_part = usize::try_from(part.part_index)
                .ok()
                .and_then(|n| {
                    item.get(field)
                        .and_then(Value::as_array)
                        .and_then(|a| a.get(n))
                })
                .ok_or(RestoreError::RestoreFailed)?;
            let expected_part = match part.kind {
                1 => "output_text",
                2 => "refusal",
                3 => "summary_text",
                4 => "reasoning_text",
                _ => return Err(RestoreError::RestoreFailed),
            };
            if item.get("type").and_then(Value::as_str) != Some(kind)
                || part
                    .item_id
                    .as_ref()
                    .is_some_and(|id| item.get("id").and_then(Value::as_str) != Some(id.as_str()))
                || final_part.get("type").and_then(Value::as_str) != Some(expected_part)
            {
                return Err(RestoreError::RestoreFailed);
            }
            let state = self
                .lifecycle
                .as_ref()
                .ok_or(RestoreError::RestoreFailed)?
                .text_state(&part.text)?;
            let Some(restored_done) = &state.restored_done else {
                return Err(RestoreError::RestoreFailed);
            };
            if state.saw_deltas
                && !restored_done
                    .as_str()
                    .as_bytes()
                    .starts_with(state.restored_deltas.as_slice())
            {
                return Err(RestoreError::RestoreFailed);
            }
        }
        for tool in self.tool_items.as_slice() {
            if tool.restored_payload.is_none() {
                return Err(RestoreError::RestoreFailed);
            }
        }

        self.failure_stage = RestoreStage::ToolCommit;
        // Finish mechanics before any successful terminal frame or publication is prepared.
        let inspection = self
            .lifecycle
            .take()
            .ok_or(RestoreError::RestoreFailed)?
            .finish()?;
        self.finished_inspection = Some(inspection);
        let inspection = self
            .finished_inspection
            .as_ref()
            .ok_or(RestoreError::RestoreFailed)?;
        let restored_response = restore_completed_snapshot(
            response_val,
            &self.lease,
            &self.budget,
            self.text_parts.as_slice(),
            self.tool_items.as_slice(),
            inspection,
        )?;
        let mut emitted_tool_delta =
            PayloadVec::new(self.budget.clone()).map_err(RestoreError::scan_error)?;
        let mut emitted_text_delta =
            PayloadVec::new(self.budget.clone()).map_err(RestoreError::scan_error)?;
        let metadata = self
            .held_events()
            .len()
            .checked_mul(std::mem::size_of::<Bytes>())
            .ok_or(RestoreError::RestoreFailed)?;
        self.frame_metadata = Some(
            self.budget
                .reserve(metadata)
                .map_err(RestoreError::capacity)?,
        );
        let mut out_frames = Vec::new();
        out_frames
            .try_reserve_exact(self.held_events().len())
            .map_err(|_| RestoreError::RestoreFailed)?;
        if out_frames.capacity() > self.held_events().len() {
            return Err(RestoreError::RestoreFailed);
        }
        for event in self.held_events() {
            let evt = event.event_name.as_str();
            let data = event.data.as_value();
            let out_idx = data
                .get("output_index")
                .and_then(Value::as_u64)
                .unwrap_or(0);
            let mut edits =
                PayloadVec::new(self.budget.clone()).map_err(RestoreError::scan_error)?;
            match evt {
                "response.completed" => {
                    copy_string_edits(
                        data.get("response").ok_or(RestoreError::RestoreFailed)?,
                        restored_response.as_value(),
                        &mut edits,
                    )?;
                }
                "response.function_call_arguments.delta"
                | "response.custom_tool_call_input.delta"
                | "response.function_call_arguments.done"
                | "response.custom_tool_call_input.done" => {
                    let custom = evt.starts_with("response.custom_tool_call_input.");
                    let track = self
                        .tool_items
                        .as_slice()
                        .iter()
                        .find(|t| t.output_index == out_idx && t.is_custom == custom)
                        .ok_or(RestoreError::RestoreFailed)?;
                    let restored = track
                        .restored_payload
                        .as_ref()
                        .ok_or(RestoreError::RestoreFailed)?;
                    // Zero replacements preserve each validated raw carrier, including delta boundaries.
                    if track.payload_replaced {
                        let key = if evt.ends_with(".delta") {
                            "delta"
                        } else if custom {
                            "input"
                        } else {
                            "arguments"
                        };
                        let text = if key == "delta" {
                            if emitted_tool_delta.as_slice().contains(&out_idx) {
                                ""
                            } else {
                                emitted_tool_delta
                                    .push(out_idx)
                                    .map_err(RestoreError::scan_error)?;
                                restored.as_str()
                            }
                        } else {
                            restored.as_str()
                        };
                        edits
                            .push(JsonStringReplacement {
                                target: data.get(key).ok_or(RestoreError::RestoreFailed)?,
                                text,
                            })
                            .map_err(RestoreError::scan_error)?;
                    }
                }
                "response.output_text.delta"
                | "response.refusal.delta"
                | "response.reasoning_summary_text.delta"
                | "response.reasoning_text.delta"
                | "response.output_text.done"
                | "response.refusal.done"
                | "response.reasoning_summary_text.done"
                | "response.reasoning_text.done"
                | "response.content_part.done"
                | "response.reasoning_summary_part.done"
                | "response.content_part.added"
                | "response.reasoning_summary_part.added" => {
                    let (part_key, kind, field) = if evt.contains("reasoning_summary") {
                        ("summary_index", 3, "text")
                    } else if matches!(
                        evt,
                        "response.reasoning_text.delta" | "response.reasoning_text.done"
                    ) || data.pointer("/part/type").and_then(Value::as_str)
                        == Some("reasoning_text")
                    {
                        ("content_index", 4, "text")
                    } else if evt.contains("refusal")
                        || data.pointer("/part/type").and_then(Value::as_str) == Some("refusal")
                    {
                        ("content_index", 2, "refusal")
                    } else {
                        ("content_index", 1, "text")
                    };
                    if evt.ends_with(".added")
                        && data.get("part").and_then(|p| p.get(field)).is_none()
                    {
                        // Missing added text makes no content claim and stays missing on replay.
                        check_no_tokens_in_value(data, &self.lease)?;
                        out_frames.push(encode_sse_frame(evt, data, &self.budget)?);
                        continue;
                    }
                    let part_idx = data.get(part_key).and_then(Value::as_u64).unwrap_or(0);
                    let index = self
                        .text_parts
                        .as_slice()
                        .iter()
                        .position(|part| {
                            part.output_index == out_idx
                                && part.part_index == part_idx
                                && part.kind == kind
                        })
                        .ok_or(RestoreError::RestoreFailed)?;
                    let track = self.text_state(index)?;
                    let restored = track
                        .restored_done
                        .as_ref()
                        .ok_or(RestoreError::RestoreFailed)?;
                    let key_tuple = (out_idx, part_idx, kind);
                    let text = if evt.ends_with(".delta") {
                        if emitted_text_delta.as_slice().contains(&key_tuple) {
                            ""
                        } else {
                            emitted_text_delta
                                .push(key_tuple)
                                .map_err(RestoreError::scan_error)?;
                            restored
                                .as_str()
                                .get(track.released_deltas_len..)
                                .ok_or(RestoreError::RestoreFailed)?
                        }
                    } else {
                        restored.as_str()
                    };
                    let target = if evt.ends_with(".delta") {
                        data.get("delta")
                    } else if evt.contains("_part.") {
                        data.get("part").and_then(|p| p.get(field))
                    } else {
                        data.get(field)
                    }
                    .ok_or(RestoreError::RestoreFailed)?;
                    if !(evt.ends_with(".added") && target.as_str() == Some("")) {
                        edits
                            .push(JsonStringReplacement { target, text })
                            .map_err(RestoreError::scan_error)?;
                    }
                }
                "response.output_item.done" | "response.output_item.added" => {
                    let raw = data.get("item").ok_or(RestoreError::RestoreFailed)?;
                    let restored = restored_response
                        .as_value()
                        .get("output")
                        .and_then(Value::as_array)
                        .and_then(|a| a.get(out_idx as usize))
                        .ok_or(RestoreError::RestoreFailed)?;
                    let preserve_payload = self
                        .tool_items
                        .as_slice()
                        .iter()
                        .find(|track| track.output_index == out_idx)
                        .is_some_and(|track| !track.payload_replaced);
                    item_string_edits(
                        raw,
                        restored,
                        evt.ends_with(".added"),
                        preserve_payload,
                        &mut edits,
                    )?;
                }
                _ => {}
            }
            let replay = PayloadJson::try_with_replacements(
                data,
                edits.as_slice(),
                self.budget.clone(),
                self.budget.limit(),
            )
            .map_err(RestoreError::scan_error)?;
            check_restored_event(evt, replay.as_value(), &self.lease)?;
            out_frames.push(encode_sse_frame(evt, replay.as_value(), &self.budget)?);
        }
        self.failure_stage = RestoreStage::ContextPublish;
        self.pending_publication = finalization::prepare_snapshot(
            response_val,
            DeliveredToolSnapshots {
                response: restored_response.as_value(),
                tracks: self.tool_items.as_slice(),
                events: self.held_events(),
            },
            self.finished_inspection
                .as_ref()
                .ok_or(RestoreError::RestoreFailed)?
                .dependency_tokens(),
            &self.lease,
            &self.binding,
            permit,
            &self.budget,
        )?;
        self.pending_delivered_history = self
            .lease
            .capture_delivered_history(&self.binding, restored_response.as_value());
        Ok(out_frames)
    }
}

/// Private groundwork only: never compiled into the production response path.
#[cfg(test)]
pub(super) const TEXT_PROJECTION_NOTICE: &str =
    "Some response text was omitted because it could not be safely restored.";

#[cfg(test)]
pub(super) fn prepare_text_projection(
    response: &Value,
    lease: &DlpRequestLease,
    budget: &PayloadBudget,
) -> Result<PayloadJson, RestoreError> {
    use super::{
        completion_json::{JsonCompletion, JsonCompletionProtocol},
        tokenization::validate_response_references,
    };
    lease
        .validate_response_liveness()
        .map_err(|_| RestoreError::FailedBecause(RestoreReason::TokenContext))?;
    let encoded = serialize_json_with_budget(response, budget, budget.limit())?;
    let raw = std::str::from_utf8(encoded.as_slice()).map_err(|_| RestoreError::RestoreFailed)?;
    let mut evidence = JsonCompletion::new(JsonCompletionProtocol::NativeResponses);
    evidence.observe_raw_json(raw, budget);
    if !evidence.permits_completion() {
        return Err(RestoreError::RestoreFailed);
    }
    let obj = response.as_object().ok_or(RestoreError::RestoreFailed)?;
    if obj.get("object").and_then(Value::as_str) != Some("response") {
        return Err(RestoreError::RestoreFailed);
    }
    for (key, value) in obj {
        check_no_tokens_in_str(key, lease)?;
        if key != "output" {
            check_no_tokens_in_value(value, lease)?;
        }
    }
    let output = obj
        .get("output")
        .and_then(Value::as_array)
        .ok_or(RestoreError::RestoreFailed)?;
    if output.len() > MAX_RESPONSE_ITEMS {
        return Err(RestoreError::RestoreFailed);
    }
    let mut replacements = PayloadVec::new(budget.clone()).map_err(RestoreError::capacity)?;
    let mut tokens = PayloadVec::new(budget.clone()).map_err(RestoreError::capacity)?;
    let mut count = 0;
    let mut damaged = false;
    for item in output {
        let obj = item.as_object().ok_or(RestoreError::RestoreFailed)?;
        if item.get("type").and_then(Value::as_str) != Some("message") {
            return Err(RestoreError::RestoreUnsupported);
        }
        for (key, value) in obj {
            check_no_tokens_in_str(key, lease)?;
            if key != "content" {
                check_no_tokens_in_value(value, lease)?;
            }
        }
        let content = item
            .get("content")
            .and_then(Value::as_array)
            .ok_or(RestoreError::RestoreFailed)?;
        if content.len() > MAX_RESPONSE_ITEMS {
            return Err(RestoreError::RestoreFailed);
        }
        for part in content {
            let obj = part.as_object().ok_or(RestoreError::RestoreFailed)?;
            if part.get("type").and_then(Value::as_str) != Some("output_text") {
                return Err(RestoreError::RestoreUnsupported);
            }
            for (key, value) in obj {
                check_no_tokens_in_str(key, lease)?;
                if key != "text" {
                    check_no_tokens_in_value(value, lease)?;
                }
            }
            let target = part.get("text").ok_or(RestoreError::RestoreFailed)?;
            let text = target.as_str().ok_or(RestoreError::RestoreFailed)?;
            if text.len() > MAX_FRAME_BYTES {
                return Err(RestoreError::FailedBecause(RestoreReason::RestoreCapacity));
            }
            let has_format_error =
                validate_response_references(text, lease).map_err(|error| match error {
                    TokenError::CapacityExceeded | TokenError::Unavailable => {
                        RestoreError::capacity(error)
                    }
                    _ => RestoreError::FailedBecause(RestoreReason::TokenContext),
                })?;
            let replacement = if has_format_error {
                damaged = true;
                PayloadText::try_from_str(TEXT_PROJECTION_NOTICE, budget.clone(), MAX_FRAME_BYTES)
                    .map_err(RestoreError::capacity)?
            } else {
                restore_text_with_lease(text, lease, budget, &mut tokens, &mut count)?
            };
            replacements
                .push((target, replacement))
                .map_err(RestoreError::capacity)?;
        }
    }
    if !damaged {
        return Err(RestoreError::RestoreFailed);
    }
    let prepared = apply_text_replacements(response, &replacements, budget)?;
    check_no_tokens_in_value(prepared.as_value(), lease)?;
    lease
        .validate_response_liveness()
        .map_err(|_| RestoreError::FailedBecause(RestoreReason::TokenContext))?;
    // No binding/witness/publication capability is available to this builder.
    Ok(prepared)
}

/// Synthetic-only transaction. It cannot be selected by an HTTP request or runtime policy.
#[cfg(test)]
pub(super) struct TextProjectionStream {
    pending: Option<(DlpRequestLease, PayloadBudget)>,
    initialized: Option<InitializedTextProjectionStream>,
    finished: bool,
}

#[cfg(test)]
impl TextProjectionStream {
    pub(super) fn new(lease: DlpRequestLease, budget: PayloadBudget) -> Result<Self, RestoreError> {
        Ok(Self {
            pending: Some((lease, budget)),
            initialized: None,
            finished: false,
        })
    }

    /// HTTP fixtures must initialize before polling the upstream body.
    pub(super) fn initialize(&mut self) -> Result<(), RestoreError> {
        if self.finished {
            return Err(RestoreError::RestoreFailed);
        }
        if self.initialized.is_none() {
            let (lease, budget) = self.pending.take().ok_or(RestoreError::RestoreFailed)?;
            match InitializedTextProjectionStream::new(lease, budget) {
                Ok(initialized) => self.initialized = Some(initialized),
                Err(error) => {
                    self.finished = true;
                    return Err(error);
                }
            }
        }
        Ok(())
    }

    pub(super) fn push_frame(&mut self, block: &str) -> Result<(), RestoreError> {
        self.initialize()?;
        let result = self
            .initialized
            .as_mut()
            .ok_or(RestoreError::RestoreFailed)?
            .push_frame(block);
        if result.is_err() {
            self.finished = true;
        }
        result
    }

    pub(super) fn finish(&mut self, reached_eof: bool) -> Result<PayloadVec<Bytes>, RestoreError> {
        if self.finished {
            return Err(RestoreError::RestoreFailed);
        }
        self.finished = true;
        self.pending.take();
        // Empty/aborted transactions cannot create mechanics or completion evidence.
        self.initialized
            .as_mut()
            .ok_or(RestoreError::RestoreFailed)?
            .finish(reached_eof)
    }
}

#[cfg(test)]
struct InitializedTextProjectionStream {
    raw: SseRestorationState,
    evidence: super::completion_json::NativeSseCompletion,
    bytes: usize,
    frames: usize,
    finished: bool,
}

#[cfg(test)]
impl InitializedTextProjectionStream {
    pub(super) fn new(lease: DlpRequestLease, budget: PayloadBudget) -> Result<Self, RestoreError> {
        // The binding is test-local; projection never uses it to publish anything.
        let binding = lease
            .bind_actual_attempt(
                "projection-fixture",
                "openai_responses",
                "http://127.0.0.1:1/responses",
                None,
            )
            .map_err(RestoreError::scan_error)?;
        let mut raw =
            SseRestorationState::new(lease, binding, budget, ArgumentNormalizationMode::Exact)?;
        raw.owner_mut()?.hold_all()?;
        Ok(Self {
            raw,
            evidence: super::completion_json::NativeSseCompletion::new(),
            bytes: 0,
            frames: 0,
            finished: false,
        })
    }

    pub(super) fn push_frame(&mut self, block: &str) -> Result<(), RestoreError> {
        if self.finished {
            return Err(RestoreError::RestoreFailed);
        }
        let result = self.push_inner(block);
        if let Err(error) = result {
            self.raw.failed.get_or_insert(error);
            self.finished = true;
        }
        result
    }

    fn push_inner(&mut self, block: &str) -> Result<(), RestoreError> {
        use super::completion_evidence::CompletionObservation;
        self.bytes = self
            .bytes
            .checked_add(block.len())
            .filter(|n| *n <= MAX_BARRIER_BYTES)
            .ok_or(RestoreError::RestoreFailed)?;
        self.frames = self
            .frames
            .checked_add(1)
            .filter(|n| *n <= MAX_RESPONSE_ITEMS)
            .ok_or(RestoreError::RestoreFailed)?;
        if !(block.ends_with("\n\n") || block.ends_with("\r\n\r\n")) {
            return Err(RestoreError::RestoreFailed);
        }
        // The inherited strict parser makes a temporary event-kind String. Reserve its
        // worst-case decoded length for this test-only transaction before parsing/copying.
        // Keep production parsing and its fixed diagnostics unchanged.
        let _kind_copy_charge = self
            .raw
            .budget
            .reserve(block.len())
            .map_err(RestoreError::capacity)?;
        self.evidence
            .observe(&CompletionObservation::RawFrame(block), &self.raw.budget);
        let before = self.raw.held_events().len();
        let released = self.raw.process_sse_block(block)?;
        if !released.is_empty() {
            return Err(RestoreError::RestoreFailed);
        }
        if self.raw.held_events().len() != before {
            let event = &self.raw.held_events()[before];
            let evt = event.event_name.as_str();
            if !matches!(
                evt,
                "response.created"
                    | "response.in_progress"
                    | "response.output_item.added"
                    | "response.output_item.done"
                    | "response.content_part.added"
                    | "response.content_part.done"
                    | "response.output_text.delta"
                    | "response.output_text.done"
                    | "response.completed"
            ) {
                return Err(RestoreError::RestoreUnsupported);
            }
            let event_name = block
                .lines()
                .find_map(|line| line.strip_prefix("event:"))
                .map(str::trim)
                .unwrap_or("");
            self.evidence.observe(
                &CompletionObservation::NativeEvent {
                    event_name,
                    data: event.data.as_value(),
                },
                &self.raw.budget,
            );
        }
        Ok(())
    }

    pub(super) fn finish(&mut self, reached_eof: bool) -> Result<PayloadVec<Bytes>, RestoreError> {
        if self.finished {
            return Err(RestoreError::RestoreFailed);
        }
        self.finished = true;
        let result = self.finish_inner(reached_eof);
        if let Err(error) = &result {
            self.raw.failed.get_or_insert(*error);
        }
        result
    }

    fn finish_inner(&mut self, reached_eof: bool) -> Result<PayloadVec<Bytes>, RestoreError> {
        use super::completion_evidence::CompletionObservation;
        self.evidence.observe(
            &CompletionObservation::EntityEnd {
                reached_eof,
                unfinished_frame: false,
                unfinished_utf8: false,
            },
            &self.raw.budget,
        );
        if !reached_eof || !self.evidence.permits_completion() || !self.raw.completed_seen {
            return Err(RestoreError::RestoreFailed);
        }
        let events = if let Some(owner) = &self.raw.lifecycle {
            owner.held_units()
        } else if let Some(inspection) = &self.raw.finished_inspection {
            inspection.held_units()
        } else {
            &[]
        };
        let last = events.last().ok_or(RestoreError::RestoreFailed)?;
        if last.event_name.as_str() != "response.completed" {
            return Err(RestoreError::RestoreFailed);
        }
        let response = last
            .data
            .as_value()
            .get("response")
            .ok_or(RestoreError::RestoreFailed)?;
        let output = response
            .get("output")
            .and_then(Value::as_array)
            .ok_or(RestoreError::RestoreFailed)?;
        let mut closed_items =
            PayloadVec::new(self.raw.budget.clone()).map_err(RestoreError::capacity)?;
        let mut closed_parts =
            PayloadVec::new(self.raw.budget.clone()).map_err(RestoreError::capacity)?;
        // Validate original events, including those after the damaged text, before any substitution.
        for event in events {
            let data = event.data.as_value();
            let evt = event.event_name.as_str();
            if is_raw_snapshot_event(evt) {
                validate_raw_snapshot(
                    data,
                    output,
                    ArgumentNormalizationMode::Exact,
                    &self.raw.budget,
                )?;
            }
            for (key, value) in data.as_object().ok_or(RestoreError::RestoreFailed)? {
                check_no_tokens_in_str(key, &self.raw.lease)?;
                let carrier = match evt {
                    "response.output_text.delta" => "delta",
                    "response.output_text.done" => "text",
                    "response.content_part.added" | "response.content_part.done" => "part",
                    "response.output_item.added" | "response.output_item.done" => "item",
                    "response.created" | "response.in_progress" | "response.completed" => {
                        "response"
                    }
                    _ => return Err(RestoreError::RestoreUnsupported),
                };
                if key != carrier {
                    check_no_tokens_in_value(value, &self.raw.lease)?;
                }
            }
            for key in ["output_index", "content_index"] {
                if data.get(key).is_some_and(|value| value.as_u64().is_none()) {
                    return Err(RestoreError::RestoreFailed);
                }
            }
            if is_raw_snapshot_event(evt) {
                check_snapshot_carriers(data, &self.raw.lease)?;
            }
            if matches!(evt, "response.created" | "response.in_progress") {
                check_no_tokens_in_value(data, &self.raw.lease)?;
                let prefix = data.get("response").ok_or(RestoreError::RestoreFailed)?;
                if !prefix.is_object()
                    || prefix.get("id") != response.get("id")
                    || prefix.get("object").and_then(Value::as_str) != Some("response")
                    || prefix.get("status").and_then(Value::as_str) != Some("in_progress")
                    || !prefix
                        .get("output")
                        .and_then(Value::as_array)
                        .is_some_and(Vec::is_empty)
                {
                    return Err(RestoreError::RestoreFailed);
                }
            } else if evt != "response.completed" {
                let index = data
                    .get("output_index")
                    .and_then(Value::as_u64)
                    .unwrap_or(0);
                if closed_items.as_slice().contains(&index) {
                    return Err(RestoreError::RestoreFailed);
                }
                if evt == "response.output_item.done" {
                    closed_items.push(index).map_err(RestoreError::capacity)?;
                } else if !evt.starts_with("response.output_item.") {
                    let part_index = data
                        .get("content_index")
                        .and_then(Value::as_u64)
                        .unwrap_or(0);
                    let identity = (index, part_index);
                    if closed_parts.as_slice().contains(&identity) {
                        return Err(RestoreError::RestoreFailed);
                    }
                    if evt == "response.content_part.done" {
                        closed_parts
                            .push(identity)
                            .map_err(RestoreError::capacity)?;
                    }
                }
                let item = usize::try_from(index)
                    .ok()
                    .and_then(|i| output.get(i))
                    .ok_or(RestoreError::RestoreFailed)?;
                if item.get("type").and_then(Value::as_str) != Some("message") {
                    return Err(RestoreError::RestoreUnsupported);
                }
                if data
                    .get("item_id")
                    .is_some_and(|id| Some(id) != item.get("id"))
                {
                    return Err(RestoreError::RestoreFailed);
                }
                if let Some(raw_item) = data.get("item") {
                    for key in ["id", "type", "role", "call_id", "name", "namespace"] {
                        if raw_item.get(key) != item.get(key) {
                            return Err(RestoreError::RestoreFailed);
                        }
                    }
                }
                if evt.starts_with("response.output_text.") {
                    let part_index = data
                        .get("content_index")
                        .and_then(Value::as_u64)
                        .unwrap_or(0);
                    let part = usize::try_from(part_index)
                        .ok()
                        .and_then(|i| item.get("content")?.as_array()?.get(i))
                        .ok_or(RestoreError::RestoreFailed)?;
                    if part.get("type").and_then(Value::as_str) != Some("output_text") {
                        return Err(RestoreError::RestoreFailed);
                    }
                }
            }
        }
        for (index, track) in self.raw.text_parts.as_slice().iter().enumerate() {
            let state = self.raw.text_state(index)?;
            let part = usize::try_from(track.output_index)
                .ok()
                .and_then(|i| output.get(i))
                .and_then(|item| item.get("content"))
                .and_then(Value::as_array)
                .and_then(|parts| {
                    usize::try_from(track.part_index)
                        .ok()
                        .and_then(|i| parts.get(i))
                })
                .ok_or(RestoreError::RestoreFailed)?;
            let final_text = part
                .get("text")
                .and_then(Value::as_str)
                .ok_or(RestoreError::RestoreFailed)?;
            if track.kind != 1
                || (state.saw_deltas && state.raw_deltas.as_slice() != final_text.as_bytes())
                || state
                    .raw_done
                    .as_ref()
                    .is_some_and(|text| text.as_str() != final_text)
            {
                return Err(RestoreError::RestoreFailed);
            }
        }
        let prepared = prepare_text_projection(response, &self.raw.lease, &self.raw.budget)?;
        // Restoration stays failed even when a separate local projection can be prepared.
        self.raw.failed = Some(RestoreError::FailedBecause(RestoreReason::TokenFormat));
        let mut emitted =
            PayloadVec::new(self.raw.budget.clone()).map_err(RestoreError::capacity)?;
        let mut tails: PayloadVec<((u64, u64), PayloadBuffer, usize)> =
            PayloadVec::new(self.raw.budget.clone()).map_err(RestoreError::capacity)?;
        let mut tokens =
            PayloadVec::new(self.raw.budget.clone()).map_err(RestoreError::capacity)?;
        let mut occurrences = 0;
        let mut frames =
            PayloadVec::new(self.raw.budget.clone()).map_err(RestoreError::capacity)?;
        for event in events {
            let data = event.data.as_value();
            let evt = event.event_name.as_str();
            // Keep the charged per-delta replacement alive through immutable replay encoding.
            let delta_text: PayloadText;
            let mut edits =
                PayloadVec::new(self.raw.budget.clone()).map_err(RestoreError::capacity)?;
            if evt == "response.completed" {
                copy_string_edits(response, prepared.as_value(), &mut edits)?;
            } else if !matches!(evt, "response.created" | "response.in_progress") {
                let index = data
                    .get("output_index")
                    .and_then(Value::as_u64)
                    .unwrap_or(0);
                let projected_item = usize::try_from(index)
                    .ok()
                    .and_then(|i| prepared.as_value().get("output")?.as_array()?.get(i))
                    .ok_or(RestoreError::RestoreFailed)?;
                if matches!(
                    evt,
                    "response.output_item.added" | "response.output_item.done"
                ) {
                    let item = data.get("item").ok_or(RestoreError::RestoreFailed)?;
                    item_string_edits(
                        item,
                        projected_item,
                        evt.ends_with(".added"),
                        false,
                        &mut edits,
                    )?;
                } else {
                    let part_index = data
                        .get("content_index")
                        .and_then(Value::as_u64)
                        .unwrap_or(0);
                    let projected_part = usize::try_from(part_index)
                        .ok()
                        .and_then(|i| projected_item.get("content")?.as_array()?.get(i))
                        .ok_or(RestoreError::RestoreFailed)?;
                    let text = projected_part
                        .get("text")
                        .and_then(Value::as_str)
                        .ok_or(RestoreError::RestoreFailed)?;
                    let target = match evt {
                        "response.content_part.added" | "response.content_part.done" => {
                            data.get("part").and_then(|part| part.get("text"))
                        }
                        "response.output_text.delta" => data.get("delta"),
                        "response.output_text.done" => data.get("text"),
                        _ => return Err(RestoreError::RestoreUnsupported),
                    };
                    if let Some(target) = target {
                        let text = if evt == "response.output_text.delta" {
                            let identity = (index, part_index);
                            let original = usize::try_from(index)
                                .ok()
                                .and_then(|i| output.get(i))
                                .and_then(|item| item.get("content")?.as_array())
                                .and_then(|parts| {
                                    usize::try_from(part_index).ok().and_then(|i| parts.get(i))
                                })
                                .and_then(|part| part.get("text"))
                                .and_then(Value::as_str)
                                .ok_or(RestoreError::RestoreFailed)?;
                            let damaged = super::tokenization::validate_response_references(
                                original,
                                &self.raw.lease,
                            )
                            .map_err(|_| {
                                RestoreError::FailedBecause(RestoreReason::TokenContext)
                            })?;
                            if damaged {
                                if emitted.as_slice().contains(&identity) {
                                    ""
                                } else {
                                    emitted.push(identity).map_err(RestoreError::capacity)?;
                                    text
                                }
                            } else {
                                let slot = match tails
                                    .as_slice()
                                    .iter()
                                    .position(|(key, _, _)| *key == identity)
                                {
                                    Some(slot) => slot,
                                    None => {
                                        tails
                                            .push((
                                                identity,
                                                PayloadBuffer::with_capacity(
                                                    0,
                                                    self.raw.budget.clone(),
                                                    MAX_FRAME_BYTES,
                                                )
                                                .map_err(RestoreError::capacity)?,
                                                0,
                                            ))
                                            .map_err(RestoreError::capacity)?;
                                        tails.len() - 1
                                    }
                                };
                                let (_, tail, received) =
                                    tails.get_mut(slot).ok_or(RestoreError::RestoreFailed)?;
                                let delta = target.as_str().ok_or(RestoreError::RestoreFailed)?;
                                *received = received
                                    .checked_add(delta.len())
                                    .ok_or(RestoreError::RestoreFailed)?;
                                let mut combined = PayloadBuffer::with_capacity(
                                    tail.len()
                                        .checked_add(delta.len())
                                        .ok_or(RestoreError::RestoreFailed)?,
                                    self.raw.budget.clone(),
                                    MAX_FRAME_BYTES,
                                )
                                .map_err(RestoreError::capacity)?;
                                combined
                                    .append(tail.as_slice())
                                    .and_then(|_| combined.append(delta.as_bytes()))
                                    .map_err(RestoreError::capacity)?;
                                let combined_text = std::str::from_utf8(combined.as_slice())
                                    .map_err(|_| RestoreError::RestoreFailed)?;
                                let (safe, pending) = if *received == original.len() {
                                    // The original final field is already fully validated; flush its last safe tail.
                                    (combined_text, "")
                                } else {
                                    split_safe_prefix_and_tail(combined_text)?
                                };
                                let mut next_tail = PayloadBuffer::with_capacity(
                                    pending.len(),
                                    self.raw.budget.clone(),
                                    MAX_FRAME_BYTES,
                                )
                                .map_err(RestoreError::capacity)?;
                                next_tail
                                    .append(pending.as_bytes())
                                    .map_err(RestoreError::capacity)?;
                                delta_text = restore_text_with_lease(
                                    safe,
                                    &self.raw.lease,
                                    &self.raw.budget,
                                    &mut tokens,
                                    &mut occurrences,
                                )?;
                                *tail = next_tail;
                                delta_text.as_str()
                            }
                        } else if evt.ends_with(".added") && target.as_str() == Some("") {
                            ""
                        } else {
                            text
                        };
                        edits
                            .push(JsonStringReplacement { target, text })
                            .map_err(RestoreError::capacity)?;
                    }
                }
            }
            let replay = PayloadJson::try_with_replacements(
                data,
                edits.as_slice(),
                self.raw.budget.clone(),
                self.raw.budget.limit(),
            )
            .map_err(RestoreError::capacity)?;
            check_no_tokens_in_value(replay.as_value(), &self.raw.lease)?;
            frames
                .push(encode_sse_frame(evt, replay.as_value(), &self.raw.budget)?)
                .map_err(RestoreError::capacity)?;
        }
        self.raw
            .lease
            .validate_response_liveness()
            .map_err(|_| RestoreError::FailedBecause(RestoreReason::TokenContext))?;
        Ok(frames)
    }
}

#[cfg(test)]
#[path = "tests/c3_converted_sse_owner_tests.rs"]
mod converted_sse_owner_tests;
