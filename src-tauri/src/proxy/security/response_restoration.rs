//! Post-consumer Codex response restoration, delta/snapshot consistency, and tool commit barrier.
//!
//! No `unwrap()`/`expect()` is permitted here. All payload copies and event buffers
//! reserve capacity from `PayloadBudget` before growth, and completed-context
//! publication requires a verified `CompletionWitness` and `ActualAttemptBinding`.
use std::{
    borrow::Cow,
    io::{self, Write},
};

use bytes::Bytes;
use serde_json::{json, Value};

use super::{
    completed_context::{
        valid_external_id, ActualAttemptBinding, CompletedCallSpec, CompletionWitness,
    },
    completion_json::permits_chat_argument_normalization,
    payload_budget::PayloadBudget,
    payload_buffer::PayloadBuffer,
    payload_json::{JsonStringReplacement, PayloadJson},
    payload_metadata::PayloadVec,
    payload_text::PayloadText,
    restore_diagnostics::{RestoreReason, RestoreStage},
    token_format_diagnostics::{Carrier, Site, TokenFormatDiagnosticState},
    tokenization::{visit_token_references, DlpRequestLease, TokenError},
};

const MAX_FRAME_BYTES: usize = 1024 * 1024;
const MAX_ARGUMENT_BYTES: usize = 1024 * 1024;
const MAX_BARRIER_BYTES: usize = 8 * 1024 * 1024;
const MAX_BARRIER_TOOLS: usize = 128;
const MAX_RESPONSE_ITEMS: usize = 4096;
use super::token_format;

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

    fn capacity(_: TokenError) -> Self {
        Self::FailedBecause(RestoreReason::RestoreCapacity)
    }

    fn scan_error(error: TokenError) -> Self {
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
    let mut found_authorized = false;
    let mut found_unauthorized = false;
    visit_token_references(text, |range| {
        if lease.resolve(&text[range]).is_ok() {
            found_authorized = true;
        } else {
            found_unauthorized = true;
        }
        Ok(())
    })
    .map_err(RestoreError::scan_error)?;
    if found_unauthorized {
        return Err(RestoreError::FailedBecause(RestoreReason::TokenContext));
    }
    if found_authorized {
        return Err(RestoreError::RestoreUnsupported);
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

// Only protocol-identified ordinary output-text leaves use exact recognition.
// Keys, metadata, tools, refusal and unknown carriers remain strict.
fn check_no_complete_output_references(
    text: &str,
    lease: &DlpRequestLease,
) -> Result<(), RestoreError> {
    let mut authorized = false;
    let mut unauthorized = false;
    super::output_text_matching::visit_complete_references(text, |range| {
        if lease.resolve(&text[range]).is_ok() {
            authorized = true;
        } else {
            unauthorized = true;
        }
        Ok(())
    })
    .map_err(RestoreError::scan_error)?;
    if unauthorized {
        return Err(RestoreError::FailedBecause(RestoreReason::TokenContext));
    }
    if authorized {
        return Err(RestoreError::RestoreUnsupported);
    }
    Ok(())
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

fn copy_reasoning_text(
    text: &str,
    _lease: &DlpRequestLease,
    budget: &PayloadBudget,
    _output_tokens: &mut PayloadVec<PayloadText>,
    _restored_occurrences: &mut usize,
) -> Result<PayloadText, RestoreError> {
    PayloadText::try_from_str(text, budget.clone(), MAX_FRAME_BYTES).map_err(RestoreError::capacity)
}

type RestoreTextPart = fn(
    &str,
    &DlpRequestLease,
    &PayloadBudget,
    &mut PayloadVec<PayloadText>,
    &mut usize,
) -> Result<PayloadText, RestoreError>;

fn restore_text_part(kind: u8) -> RestoreTextPart {
    match kind {
        1 => restore_output_text_with_lease,
        3 | 4 => copy_reasoning_text,
        _ => restore_text_with_lease,
    }
}

fn restore_output_text_with_lease(
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
        true,
    )
}

fn restore_text_with_matching(
    text: &str,
    lease: &DlpRequestLease,
    budget: &PayloadBudget,
    output_tokens: &mut PayloadVec<PayloadText>,
    restored_occurrences: &mut usize,
    output_text: bool,
) -> Result<PayloadText, RestoreError> {
    let mut ranges = PayloadVec::new(budget.clone()).map_err(RestoreError::capacity)?;
    if output_text {
        super::output_text_matching::visit_complete_references(text, |range| ranges.push(range))
    } else {
        visit_token_references(text, |range| ranges.push(range))
    }
    .map_err(RestoreError::scan_error)?;
    let mut out_len = text.len();
    for range in ranges.as_slice() {
        let token = &text[range.clone()];
        let plain = lease
            .resolve(token)
            .map_err(|_| RestoreError::FailedBecause(RestoreReason::TokenContext))?;
        push_unique_text(
            output_tokens,
            token,
            token_format::MAX_REFERENCE_BYTES,
            budget,
        )?;
        out_len = out_len
            .checked_sub(range.len())
            .and_then(|n| n.checked_add(plain.len()))
            .ok_or(RestoreError::FailedBecause(RestoreReason::RestoreCapacity))?;
        *restored_occurrences = restored_occurrences
            .checked_add(1)
            .ok_or(RestoreError::FailedBecause(RestoreReason::RestoreCapacity))?;
    }
    let mut out = PayloadText::with_capacity(out_len, budget.clone(), budget.limit())
        .map_err(RestoreError::capacity)?;
    let mut cursor = 0usize;
    for range in ranges.as_slice() {
        out.append(&text[cursor..range.start])
            .map_err(RestoreError::capacity)?;
        let plain = lease
            .resolve(&text[range.clone()])
            .map_err(|_| RestoreError::FailedBecause(RestoreReason::TokenContext))?;
        out.append(plain).map_err(RestoreError::capacity)?;
        cursor = range.end;
    }
    out.append(&text[cursor..])
        .map_err(RestoreError::capacity)?;
    Ok(out)
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
    let mut edits = PayloadVec::new(budget.clone()).map_err(|_| RestoreError::RestoreFailed)?;
    for (target, text) in replacements.as_slice() {
        edits
            .push(JsonStringReplacement {
                target,
                text: text.as_str(),
            })
            .map_err(|_| RestoreError::RestoreFailed)?;
    }
    PayloadJson::try_with_replacements(source, edits.as_slice(), budget.clone(), budget.limit())
        .map_err(|_| RestoreError::RestoreFailed)
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
        .map_err(|_| RestoreError::RestoreFailed)?;
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
    let mut replacements = PayloadVec::new(budget.clone()).map_err(RestoreError::capacity)?;
    restore_json_leaves(
        parsed.as_value(),
        lease,
        budget,
        output_tokens,
        restored_occurrences,
        &mut replacements,
    )?;
    let restored = apply_text_replacements(parsed.as_value(), &replacements, budget)
        .map_err(|error| error.with_fallback_reason(RestoreReason::ArgumentsRebuild))?;
    let encoded = serialize_json_with_budget(restored.as_value(), budget, MAX_ARGUMENT_BYTES)
        .map_err(|error| error.with_fallback_reason(RestoreReason::ArgumentsSerialize))?;
    let text = std::str::from_utf8(encoded.as_slice())
        .map_err(|_| RestoreError::FailedBecause(RestoreReason::ArgumentsCopy))?;
    PayloadText::try_from_str(text, budget.clone(), MAX_ARGUMENT_BYTES)
        .map_err(|error| RestoreError::arguments_error(error, RestoreReason::ArgumentsCopy))
}

fn restore_output_item<'a>(
    item: &'a Value,
    is_compact: bool,
    lease: &DlpRequestLease,
    budget: &PayloadBudget,
    output_tokens: &mut PayloadVec<PayloadText>,
    restored_occurrences: &mut usize,
    replacements: &mut TextReplacements<'a>,
) -> Result<(), RestoreError> {
    let obj = item.as_object().ok_or(RestoreError::RestoreFailed)?;
    let kind = obj
        .get("type")
        .and_then(Value::as_str)
        .ok_or(RestoreError::RestoreFailed)?;
    match kind {
        "reasoning" => check_reasoning_item(item, lease)?,
        "message" => {
            let parts_field = "content";
            for (key, value) in obj {
                check_no_tokens_in_str(key, lease)?;
                if key != parts_field {
                    check_no_tokens_in_value(value, lease)?;
                }
            }
            let parts = match obj.get(parts_field) {
                Some(value) => value.as_array().ok_or(RestoreError::RestoreFailed)?,
                None if kind == "reasoning" => return Ok(()),
                None => return Err(RestoreError::RestoreFailed),
            };
            for part in parts {
                let part_obj = part.as_object().ok_or(RestoreError::RestoreFailed)?;
                let field = match (kind, part.get("type").and_then(Value::as_str)) {
                    ("message", Some("output_text")) | ("reasoning", Some("summary_text")) => {
                        "text"
                    }
                    ("message", Some("refusal")) => "refusal",
                    _ => {
                        check_no_tokens_in_value(part, lease)?;
                        return Err(RestoreError::RestoreFailed);
                    }
                };
                for (key, value) in part_obj {
                    check_no_tokens_in_str(key, lease)?;
                    if key != field {
                        check_no_tokens_in_value(value, lease)?;
                    }
                }
                let target = part.get(field).ok_or(RestoreError::RestoreFailed)?;
                let text = target.as_str().ok_or(RestoreError::RestoreFailed)?;
                let restore = if kind == "message"
                    && part.get("type").and_then(Value::as_str) == Some("output_text")
                {
                    restore_output_text_with_lease
                } else {
                    restore_text_with_lease
                };
                let restored = restore(text, lease, budget, output_tokens, restored_occurrences)?;
                replacements
                    .push((target, restored))
                    .map_err(|_| RestoreError::RestoreFailed)?;
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
                restore_json_arguments_string(
                    text,
                    lease,
                    budget,
                    output_tokens,
                    restored_occurrences,
                )?
            } else {
                restore_text_with_lease(text, lease, budget, output_tokens, restored_occurrences)?
            };
            replacements
                .push((target, restored))
                .map_err(|_| RestoreError::RestoreFailed)?;
            if obj
                .get("reasoning_content")
                .is_some_and(|v| !v.is_null() && !v.is_string())
            {
                return Err(RestoreError::RestoreFailed);
            }
            if obj
                .get("encrypted_reasoning")
                .is_some_and(|value| !value.is_null() && !value.is_string())
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
    restored: RestoredJsonResponse,
    source: &'a Value,
    output_tokens: PayloadVec<PayloadText>,
}
impl StagedJsonResponse<'_> {
    pub(super) fn as_value(&self) -> &Value {
        self.restored.as_value()
    }
    pub(super) fn publish(
        self,
        lease: &DlpRequestLease,
        binding: &ActualAttemptBinding,
        witness: &CompletionWitness,
        budget: &PayloadBudget,
    ) -> Result<RestoredJsonResponse, RestoreError> {
        publish_completed_snapshot(
            self.source,
            &self.output_tokens,
            lease,
            binding,
            witness,
            budget,
        )?;
        Ok(self.restored)
    }
}

pub(super) fn stage_json_response<'a>(
    response: &'a Value,
    is_compact: bool,
    lease: &DlpRequestLease,
    budget: &PayloadBudget,
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
    let mut replacements =
        PayloadVec::new(budget.clone()).map_err(|_| RestoreError::RestoreFailed)?;
    let mut output_tokens =
        PayloadVec::new(budget.clone()).map_err(|_| RestoreError::RestoreFailed)?;
    let mut restored_occurrences = 0;
    for item in output {
        restore_output_item(
            item,
            is_compact,
            lease,
            budget,
            &mut output_tokens,
            &mut restored_occurrences,
            &mut replacements,
        )?;
    }
    let value = apply_text_replacements(response, &replacements, budget)?;
    Ok(StagedJsonResponse {
        restored: RestoredJsonResponse {
            value,
            restored_occurrences,
        },
        source: response,
        output_tokens,
    })
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
                .map_err(|_| RestoreError::RestoreFailed)?;
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
        if !supported {
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
    if !part.get(field).is_some_and(Value::is_string) {
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
                        check_snapshot_part(part, container, lease)?;
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
        check_snapshot_part(part, container, lease)?;
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
    output_tokens: &mut PayloadVec<PayloadText>,
) -> Result<PayloadJson, RestoreError> {
    let output = response
        .get("output")
        .and_then(Value::as_array)
        .ok_or(RestoreError::RestoreFailed)?;
    let mut replacements =
        PayloadVec::new(budget.clone()).map_err(|_| RestoreError::RestoreFailed)?;
    for item in output {
        let kind = item
            .get("type")
            .and_then(Value::as_str)
            .ok_or(RestoreError::RestoreFailed)?;
        let mut targets =
            PayloadVec::new(budget.clone()).map_err(|_| RestoreError::RestoreFailed)?;
        match kind {
            "reasoning" => {
                check_reasoning_item(item, lease)?;
            }
            "message" => {
                let parts = if kind == "message" {
                    Some(
                        item.get("content")
                            .and_then(Value::as_array)
                            .ok_or(RestoreError::RestoreFailed)?,
                    )
                } else {
                    item.get("summary").and_then(Value::as_array)
                };
                if let Some(parts) = parts {
                    for part in parts {
                        let field = match part.get("type").and_then(Value::as_str) {
                            Some("output_text" | "summary_text") => "text",
                            Some("refusal") => "refusal",
                            _ => return Err(RestoreError::RestoreFailed),
                        };
                        targets
                            .push(part.get(field).ok_or(RestoreError::RestoreFailed)?)
                            .map_err(|_| RestoreError::RestoreFailed)?;
                    }
                }
            }
            "function_call" | "custom_tool_call" => {
                let field = if kind == "function_call" {
                    "arguments"
                } else {
                    "input"
                };
                targets
                    .push(item.get(field).ok_or(RestoreError::RestoreFailed)?)
                    .map_err(|_| RestoreError::RestoreFailed)?;
            }
            _ => return Err(RestoreError::RestoreFailed),
        }
        for target in targets.as_slice() {
            let text = target.as_str().ok_or(RestoreError::RestoreFailed)?;
            let mut ignored = 0;
            let restored = if kind == "function_call" && std::ptr::eq(*target, &item["arguments"]) {
                restore_json_arguments_string(text, lease, budget, output_tokens, &mut ignored)?
            } else {
                let ordinary_output_text = kind == "message"
                    && item
                        .get("content")
                        .and_then(Value::as_array)
                        .is_some_and(|parts| {
                            parts.iter().any(|part| {
                                part.get("type").and_then(Value::as_str) == Some("output_text")
                                    && part
                                        .get("text")
                                        .is_some_and(|value| std::ptr::eq(value, *target))
                            })
                        });
                let restore = if ordinary_output_text {
                    restore_output_text_with_lease
                } else {
                    restore_text_with_lease
                };
                restore(text, lease, budget, output_tokens, &mut ignored)?
            };
            replacements
                .push((*target, restored))
                .map_err(|_| RestoreError::RestoreFailed)?;
        }
    }
    let mut edits = PayloadVec::new(budget.clone()).map_err(|_| RestoreError::RestoreFailed)?;
    for (target, text) in replacements.as_slice() {
        edits
            .push(JsonStringReplacement {
                target,
                text: text.as_str(),
            })
            .map_err(|_| RestoreError::RestoreFailed)?;
    }
    let restored = PayloadJson::try_with_replacements(
        response,
        edits.as_slice(),
        budget.clone(),
        budget.limit(),
    )
    .map_err(|_| RestoreError::RestoreFailed)?;
    check_restored_response(restored.as_value(), lease)?;
    Ok(restored)
}

fn publish_completed_snapshot(
    response: &Value,
    output_tokens: &PayloadVec<PayloadText>,
    lease: &DlpRequestLease,
    binding: &ActualAttemptBinding,
    witness: &CompletionWitness,
    budget: &PayloadBudget,
) -> Result<(), RestoreError> {
    if !binding.reusable_principal {
        return Ok(());
    }
    let response_id = response
        .get("id")
        .and_then(Value::as_str)
        .filter(|id| valid_external_id(id))
        .ok_or(RestoreError::RestoreFailed)?;
    let mut tokens = PayloadVec::new(budget.clone()).map_err(|_| RestoreError::RestoreFailed)?;
    for token in output_tokens.as_slice() {
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
            let required = |key| {
                item.get(key)
                    .and_then(Value::as_str)
                    .ok_or(RestoreError::RestoreFailed)
            };
            calls
                .push(CompletedCallSpec {
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
    lease
        .publish_completed_context(
            binding,
            witness,
            response_id,
            tokens.as_slice(),
            calls.as_slice(),
            opaques.as_slice(),
        )
        .map_err(|_| RestoreError::RestoreFailed)?;
    Ok(())
}

pub(super) fn split_safe_prefix_and_tail(combined: &str) -> Result<(&str, &str), RestoreError> {
    let bytes = combined.as_bytes();
    let mut offset = 0;
    while let Some((start, prefix_len)) = token_format::next_candidate(combined, offset) {
        offset = start + prefix_len;
        let rem = &bytes[start..];
        let Some(layout) = token_format::header(rem) else {
            continue;
        };
        if rem.len() >= layout.len {
            if !layout.complete(rem) {
                return Err(RestoreError::FailedBecause(RestoreReason::TokenFormat));
            }
            offset = start + layout.len;
        } else if token_format::is_prefix(rem) {
            return Ok((&combined[..start], &combined[start..]));
        } else {
            return Err(RestoreError::FailedBecause(RestoreReason::TokenFormat));
        }
    }
    for start in bytes
        .len()
        .saturating_sub(token_format::MAX_REFERENCE_BYTES - 1)..bytes.len()
    {
        if combined.is_char_boundary(start) && token_format::is_prefix(&bytes[start..]) {
            return Ok((&combined[..start], &combined[start..]));
        }
    }
    Ok((combined, ""))
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
    .map_err(|_| RestoreError::RestoreFailed)
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
        .map_err(|_| RestoreError::RestoreFailed)?;
    let final_value = PayloadJson::try_from_str(snapshot, budget.clone(), budget.limit())
        .map_err(|_| RestoreError::RestoreFailed)?;
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
        .map_err(|_| RestoreError::RestoreFailed),
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
            tracks: PayloadVec::new(budget.clone()).map_err(|_| RestoreError::RestoreFailed)?,
            snapshots: PayloadVec::new(budget.clone()).map_err(|_| RestoreError::RestoreFailed)?,
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
                .map_err(|_| RestoreError::RestoreFailed)?,
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
                    .map_err(|_| RestoreError::RestoreFailed)?;
            self.snapshots
                .push(snapshot)
                .map_err(|_| RestoreError::RestoreFailed)?;
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
                        .map_err(|_| RestoreError::RestoreFailed)?,
                    saw_deltas: false,
                    done_seen: false,
                };
                self.tracks
                    .push(track)
                    .map_err(|_| RestoreError::RestoreFailed)?;
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
                        .map_err(|_| RestoreError::RestoreFailed)?,
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
                .map_err(|_| RestoreError::RestoreFailed)?;
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
    saw_deltas: bool,
    raw_deltas: PayloadBuffer,
    restored_deltas: PayloadBuffer,
    pending_tail: PayloadBuffer,
    raw_done: Option<PayloadText>,
    restored_done: Option<PayloadText>,
    part_done_seen: bool,
    item_done_seen: bool,
    released_deltas_len: usize,
}

struct ToolItemTrack {
    output_index: u64,
    is_custom: bool,
    saw_deltas: bool,
    raw_deltas: PayloadBuffer,
    raw_done: Option<PayloadText>,
    raw_item_done: Option<PayloadText>,
    restored_payload: Option<PayloadText>,
}

struct BufferedSseEvent {
    event_name: PayloadText,
    data: PayloadJson,
    prepared: Option<PayloadJson>,
}

pub(crate) struct SseRestorationState {
    lease: DlpRequestLease,
    binding: ActualAttemptBinding,
    budget: PayloadBudget,
    norm_mode: ArgumentNormalizationMode,
    barrier_active: bool,
    barrier_bytes: usize,
    barrier_events: PayloadVec<BufferedSseEvent>,
    deferred_events: PayloadVec<BufferedSseEvent>,
    deferred_bytes: usize,
    raw_snapshots: PayloadVec<PayloadJson>,
    text_parts: PayloadVec<TextPartTrack>,
    tool_items: PayloadVec<ToolItemTrack>,
    output_tokens: PayloadVec<PayloadText>,
    restored_occurrences: usize,
    completed_seen: bool,
    finalized: bool,
    failed: Option<RestoreError>,
    failure_stage: RestoreStage,
    token_format_diagnostics: TokenFormatDiagnosticState,
    pending_delivered_history: Option<super::history_provenance::PendingDeliveredHistory>,
}

impl SseRestorationState {
    pub(super) fn new(
        lease: DlpRequestLease,
        binding: ActualAttemptBinding,
        budget: PayloadBudget,
        norm_mode: ArgumentNormalizationMode,
    ) -> Result<Self, RestoreError> {
        Ok(Self {
            lease,
            binding,
            budget: budget.clone(),
            norm_mode,
            barrier_active: false,
            barrier_bytes: 0,
            barrier_events: PayloadVec::new(budget.clone())
                .map_err(|_| RestoreError::RestoreFailed)?,
            deferred_events: PayloadVec::new(budget.clone())
                .map_err(|_| RestoreError::RestoreFailed)?,
            deferred_bytes: 0,
            raw_snapshots: PayloadVec::new(budget.clone())
                .map_err(|_| RestoreError::RestoreFailed)?,
            text_parts: PayloadVec::new(budget.clone()).map_err(|_| RestoreError::RestoreFailed)?,
            tool_items: PayloadVec::new(budget.clone()).map_err(|_| RestoreError::RestoreFailed)?,
            output_tokens: PayloadVec::new(budget).map_err(|_| RestoreError::RestoreFailed)?,
            restored_occurrences: 0,
            completed_seen: false,
            finalized: false,
            failed: None,
            failure_stage: RestoreStage::Unknown,
            token_format_diagnostics: TokenFormatDiagnosticState::default(),
            pending_delivered_history: None,
        })
    }

    /// Only the final wrapper may call this after every terminal frame was released.
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
            .barrier_events
            .as_slice()
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
        self.restored_occurrences
    }

    fn get_or_create_text_part(
        &mut self,
        output_index: u64,
        part_index: u64,
        kind: u8,
    ) -> Result<&mut TextPartTrack, RestoreError> {
        let pos = self.text_parts.as_slice().iter().position(|p| {
            p.output_index == output_index && p.part_index == part_index && p.kind == kind
        });
        if let Some(idx) = pos {
            return self
                .text_parts
                .get_mut(idx)
                .ok_or(RestoreError::RestoreFailed);
        }
        if self.text_parts.len() >= MAX_RESPONSE_ITEMS {
            return Err(RestoreError::RestoreFailed);
        }
        let idx = self.text_parts.len();
        self.text_parts
            .push(TextPartTrack {
                output_index,
                item_id: None,
                part_index,
                kind,
                saw_deltas: false,
                raw_deltas: PayloadBuffer::new(self.budget.clone(), MAX_FRAME_BYTES)
                    .map_err(|_| RestoreError::RestoreFailed)?,
                restored_deltas: PayloadBuffer::new(self.budget.clone(), MAX_FRAME_BYTES)
                    .map_err(|_| RestoreError::RestoreFailed)?,
                pending_tail: PayloadBuffer::new(self.budget.clone(), 64)
                    .map_err(|_| RestoreError::RestoreFailed)?,
                raw_done: None,
                restored_done: None,
                part_done_seen: false,
                item_done_seen: false,
                released_deltas_len: 0,
            })
            .map_err(|_| RestoreError::RestoreFailed)?;
        self.text_parts
            .get_mut(idx)
            .ok_or(RestoreError::RestoreFailed)
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
                    .map_err(|_| RestoreError::RestoreFailed)?,
                raw_done: None,
                raw_item_done: None,
                restored_payload: None,
            })
            .map_err(|_| RestoreError::RestoreFailed)?;
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
        self.barrier_active = true;
        self.barrier_bytes = self
            .barrier_bytes
            .checked_add(raw_bytes_len)
            .filter(|&n| n <= MAX_BARRIER_BYTES)
            .ok_or(RestoreError::RestoreFailed)?;
        if self.barrier_events.len() >= MAX_RESPONSE_ITEMS {
            return Err(RestoreError::RestoreFailed);
        }
        let name = PayloadText::try_from_str(event_name, self.budget.clone(), 256)
            .map_err(|_| RestoreError::RestoreFailed)?;
        self.barrier_events
            .push(BufferedSseEvent {
                event_name: name,
                data,
                prepared: None,
            })
            .map_err(|_| RestoreError::RestoreFailed)
    }

    pub(crate) fn process_sse_block(&mut self, block: &str) -> Result<Vec<Bytes>, RestoreError> {
        if let Some(err) = self.failed {
            return Err(err);
        }
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
                .map_err(|_| RestoreError::RestoreFailed)?;
        let mut first = true;
        for line in block.lines() {
            let trimmed = line.trim_start();
            if let Some(val) = trimmed
                .strip_prefix("data:")
                .or_else(|| (trimmed == "data").then_some(""))
            {
                if !first {
                    data_buf
                        .append(b"\n")
                        .map_err(|_| RestoreError::RestoreFailed)?;
                }
                first = false;
                data_buf
                    .append(val.strip_prefix(' ').unwrap_or(val).as_bytes())
                    .map_err(|_| RestoreError::RestoreFailed)?;
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
            .map_err(|_| RestoreError::RestoreFailed)?;
        self.failure_stage = RestoreStage::SseEvent;
        let data = parsed.as_value();
        let kind = data
            .get("type")
            .and_then(Value::as_str)
            .ok_or(RestoreError::RestoreFailed)?
            .to_owned();
        let evt = event_name.unwrap_or(kind.as_str());
        self.token_format_diagnostics
            .record_event(evt, self.barrier_active, block.len());
        if evt != kind {
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
                                .map_err(|_| RestoreError::RestoreFailed)?,
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

        if is_tool_event || evt == "response.completed" || self.barrier_active {
            if self.deferred_events.len() != 0 {
                // Deferred text has already been tracked; transfer its raw snapshots in order.
                self.barrier_bytes = self.deferred_bytes;
                self.deferred_bytes = 0;
                std::mem::swap(&mut self.barrier_events, &mut self.deferred_events);
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
                        .map_err(|_| RestoreError::RestoreFailed)?,
                )
                .map_err(|_| RestoreError::RestoreFailed)?;
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
            PayloadVec::new(self.budget.clone()).map_err(|_| RestoreError::RestoreFailed)?;
        self.failure_stage = RestoreStage::SseEvent;
        self.handle_pre_barrier_event(evt, data, &mut replacements)?;
        let rewritten = apply_text_replacements(data, &replacements, &self.budget)?;
        if matches!(
            evt,
            "response.content_part.done" | "response.reasoning_summary_part.done"
        ) {
            check_restored_event(evt, rewritten.as_value(), &self.lease)?;
        }
        let has_short_tail = self.text_parts.as_slice().iter().any(|p| {
            !p.pending_tail.as_slice().is_empty()
                && (p.kind == 1 || token_format::header(p.pending_tail.as_slice()).is_none())
        });
        if self.deferred_events.len() != 0 || has_short_tail {
            self.deferred_bytes = self
                .deferred_bytes
                .checked_add(block.len())
                .filter(|n| *n <= MAX_BARRIER_BYTES)
                .ok_or(RestoreError::RestoreFailed)?;
            if self.deferred_events.len() >= MAX_RESPONSE_ITEMS {
                return Err(RestoreError::RestoreFailed);
            }
            let mut edits =
                PayloadVec::new(self.budget.clone()).map_err(|_| RestoreError::RestoreFailed)?;
            copy_string_edits(data, rewritten.as_value(), &mut edits)?;
            let restored = PayloadJson::try_with_replacements(
                data,
                edits.as_slice(),
                self.budget.clone(),
                self.budget.limit(),
            )
            .map_err(|_| RestoreError::RestoreFailed)?;
            // Keep both versions charged: raw events are needed if a tool starts before resolution.
            self.deferred_events
                .push(BufferedSseEvent {
                    event_name: PayloadText::try_from_str(evt, self.budget.clone(), 256)
                        .map_err(|_| RestoreError::RestoreFailed)?,
                    data: parsed,
                    prepared: Some(restored),
                })
                .map_err(|_| RestoreError::RestoreFailed)?;
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
        .map_err(|_| RestoreError::RestoreFailed)?;
    frame
        .append(b"event: ")
        .and_then(|_| frame.append(event_name.as_bytes()))
        .and_then(|_| frame.append(b"\ndata: "))
        .and_then(|_| frame.append(json_buf.as_slice()))
        .and_then(|_| frame.append(b"\n\n"))
        .map_err(|_| RestoreError::RestoreFailed)?;
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
            "id": "resp_dlp_failed",
            "object": "response",
            "status": "failed",
            "error": {
                "message": err.message_at(stage),
                "type": "server_error",
                "code": err.code(),
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
            let track = self.get_or_create_text_part(out, part, kind)?;
            track.released_deltas_len = track.restored_deltas.len();
        }
        Ok(())
    }

    fn flush_output_text_tail(&mut self, out: u64, part: u64) -> Result<(), RestoreError> {
        let track = self.get_or_create_text_part(out, part, 1)?;
        if !track.pending_tail.as_slice().is_empty() {
            track
                .restored_deltas
                .append(track.pending_tail.as_slice())
                .map_err(RestoreError::capacity)?;
            track.pending_tail.clear();
        }
        Ok(())
    }

    fn flush_ordinary_tail(&mut self, evt: &str, data: &Value) -> Result<(), RestoreError> {
        let (out, part, kind) = text_event_key(evt, data);
        let track = self.get_or_create_text_part(out, part, kind)?;
        if kind != 1 && token_format::header(track.pending_tail.as_slice()).is_some() {
            return Err(RestoreError::RestoreFailed);
        }
        if !track.pending_tail.as_slice().is_empty() {
            track
                .restored_deltas
                .append(track.pending_tail.as_slice())
                .map_err(|_| RestoreError::RestoreFailed)?;
            track.pending_tail.clear();
        }
        Ok(())
    }

    fn flush_deferred_text(&mut self) -> Result<Vec<Bytes>, RestoreError> {
        let mut emitted =
            PayloadVec::new(self.budget.clone()).map_err(|_| RestoreError::RestoreFailed)?;
        let mut frames = Vec::new();
        for event in self.deferred_events.as_slice() {
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
                let track = self
                    .text_parts
                    .as_slice()
                    .iter()
                    .find(|p| (p.output_index, p.part_index, p.kind) == key)
                    .ok_or(RestoreError::RestoreFailed)?;
                let text = if emitted.as_slice().contains(&key) {
                    ""
                } else {
                    emitted.push(key).map_err(|_| RestoreError::RestoreFailed)?;
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
                .map_err(|_| RestoreError::RestoreFailed)?;
                frames.push(encode_sse_frame(evt, replay.as_value(), &self.budget)?);
            } else {
                frames.push(encode_sse_frame(evt, data, &self.budget)?);
            }
        }
        for key in emitted.as_slice() {
            let track = self.get_or_create_text_part(key.0, key.1, key.2)?;
            track.released_deltas_len = track.restored_deltas.len();
        }
        self.deferred_events =
            PayloadVec::new(self.budget.clone()).map_err(|_| RestoreError::RestoreFailed)?;
        self.deferred_bytes = 0;
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
            | "response.reasoning_text.delta" => {
                let (part_key, kind) = match evt {
                    "response.output_text.delta" => ("content_index", 1u8),
                    "response.refusal.delta" => ("content_index", 2u8),
                    "response.reasoning_text.delta" => ("content_index", 4u8),
                    _ => ("summary_index", 3u8),
                };
                let part_idx = data.get(part_key).and_then(Value::as_u64).unwrap_or(0);
                let delta = data
                    .get("delta")
                    .and_then(Value::as_str)
                    .ok_or(RestoreError::RestoreFailed)?;
                let track = self.get_or_create_text_part(out_idx, part_idx, kind)?;
                if track.raw_done.is_some() {
                    return Err(RestoreError::RestoreFailed);
                }
                track.saw_deltas = true;
                track
                    .raw_deltas
                    .append(delta.as_bytes())
                    .map_err(|_| RestoreError::RestoreFailed)?;
            }
            "response.output_text.done"
            | "response.refusal.done"
            | "response.reasoning_summary_text.done"
            | "response.reasoning_text.done" => {
                let (part_key, field_key, kind) = match evt {
                    "response.output_text.done" => ("content_index", "text", 1u8),
                    "response.refusal.done" => ("content_index", "refusal", 2u8),
                    "response.reasoning_text.done" => ("content_index", "text", 4u8),
                    _ => ("summary_index", "text", 3u8),
                };
                let part_idx = data.get(part_key).and_then(Value::as_u64).unwrap_or(0);
                let text = data
                    .get(field_key)
                    .and_then(Value::as_str)
                    .ok_or(RestoreError::RestoreFailed)?;
                let budget = self.budget.clone();
                let track = self.get_or_create_text_part(out_idx, part_idx, kind)?;
                if track.raw_done.is_some() {
                    return Err(RestoreError::RestoreFailed);
                }
                if track.saw_deltas && track.raw_deltas.as_slice() != text.as_bytes() {
                    return Err(RestoreError::RestoreFailed);
                }
                track.raw_done = Some(
                    PayloadText::try_from_str(text, budget, MAX_FRAME_BYTES)
                        .map_err(|_| RestoreError::RestoreFailed)?,
                );
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
                    .map_err(|_| RestoreError::RestoreFailed)?;
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
                        .map_err(|_| RestoreError::RestoreFailed)?,
                );
            }
            "response.output_item.added" => {
                let item = data.get("item").ok_or(RestoreError::RestoreFailed)?;
                match item.get("type").and_then(Value::as_str) {
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
                                .map_err(|_| RestoreError::RestoreFailed)?,
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
                                .map_err(|_| RestoreError::RestoreFailed)?,
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
        let out_idx = data
            .get("output_index")
            .and_then(Value::as_u64)
            .unwrap_or(0);
        match evt {
            "response.output_text.delta"
            | "response.refusal.delta"
            | "response.reasoning_summary_text.delta"
            | "response.reasoning_text.delta" => {
                let (part_key, kind) = match evt {
                    "response.output_text.delta" => ("content_index", 1u8),
                    "response.refusal.delta" => ("content_index", 2u8),
                    "response.reasoning_text.delta" => ("content_index", 4u8),
                    _ => ("summary_index", 3u8),
                };
                let part_idx = data.get(part_key).and_then(Value::as_u64).unwrap_or(0);
                let delta = data
                    .get("delta")
                    .and_then(Value::as_str)
                    .ok_or(RestoreError::RestoreFailed)?;
                let obj = data.as_object().ok_or(RestoreError::RestoreFailed)?;
                for (k, v) in obj.iter() {
                    check_no_tokens_in_str(k, &self.lease)?;
                    if k != "delta" {
                        check_no_tokens_in_value(v, &self.lease)?;
                    }
                }
                let budget = self.budget.clone();
                let (combined_buf, pending_before) = {
                    let track = self.get_or_create_text_part(out_idx, part_idx, kind)?;
                    if track.raw_done.is_some() {
                        return Err(RestoreError::RestoreFailed);
                    }
                    track.saw_deltas = true;
                    track
                        .raw_deltas
                        .append(delta.as_bytes())
                        .map_err(|_| RestoreError::RestoreFailed)?;
                    let combined_len = track
                        .pending_tail
                        .len()
                        .checked_add(delta.len())
                        .ok_or(RestoreError::RestoreFailed)?;
                    let mut buf =
                        PayloadBuffer::with_capacity(combined_len, budget.clone(), MAX_FRAME_BYTES)
                            .map_err(|_| RestoreError::RestoreFailed)?;
                    buf.append(track.pending_tail.as_slice())
                        .and_then(|_| buf.append(delta.as_bytes()))
                        .map_err(|_| RestoreError::RestoreFailed)?;
                    (buf, track.pending_tail.len())
                };
                self.token_format_diagnostics.record_delta_structure(
                    pending_before,
                    pending_before,
                    delta.len(),
                );
                let combined_str = std::str::from_utf8(combined_buf.as_slice())
                    .map_err(|_| RestoreError::RestoreFailed)?;
                self.failure_stage = RestoreStage::TokenRestore;
                let carrier = match kind {
                    1 => Carrier::OutputText,
                    2 => Carrier::Refusal,
                    _ => Carrier::Reasoning,
                };
                let split = if kind == 1 {
                    Ok(super::output_text_matching::split_prefix_and_tail(
                        combined_str,
                    ))
                } else if matches!(kind, 3 | 4) {
                    Ok((combined_str, ""))
                } else {
                    split_safe_prefix_and_tail(combined_str)
                };
                let (safe_prefix, new_tail) = split.inspect_err(|err| {
                    if *err == RestoreError::FailedBecause(RestoreReason::TokenFormat) {
                        self.token_format_diagnostics.observe(
                            combined_str,
                            pending_before,
                            carrier,
                            Site::DeltaSplit,
                            true,
                        );
                    }
                })?;
                let restore = restore_text_part(kind);
                let restored_chunk = restore(
                    safe_prefix,
                    &self.lease,
                    &budget,
                    &mut self.output_tokens,
                    &mut self.restored_occurrences,
                )
                .inspect_err(|err| {
                    if *err == RestoreError::FailedBecause(RestoreReason::TokenFormat) {
                        self.token_format_diagnostics.observe(
                            safe_prefix,
                            pending_before,
                            carrier,
                            Site::DeltaRestore,
                            false,
                        );
                    }
                })?;
                self.token_format_diagnostics.record_delta_structure(
                    pending_before,
                    new_tail.len(),
                    delta.len(),
                );
                self.failure_stage = RestoreStage::SseEvent;
                let track = self.get_or_create_text_part(out_idx, part_idx, kind)?;
                let mut next_tail = PayloadBuffer::with_capacity(new_tail.len(), budget, 64)
                    .map_err(|_| RestoreError::RestoreFailed)?;
                next_tail
                    .append(new_tail.as_bytes())
                    .map_err(|_| RestoreError::RestoreFailed)?;
                track.pending_tail = next_tail;
                track
                    .restored_deltas
                    .append(restored_chunk.as_str().as_bytes())
                    .map_err(|_| RestoreError::RestoreFailed)?;
                replacements
                    .push((
                        data.get("delta").ok_or(RestoreError::RestoreFailed)?,
                        restored_chunk,
                    ))
                    .map_err(|_| RestoreError::RestoreFailed)?;
            }
            "response.output_text.done"
            | "response.refusal.done"
            | "response.reasoning_summary_text.done"
            | "response.reasoning_text.done" => {
                let (part_key, field_key, kind) = match evt {
                    "response.output_text.done" => ("content_index", "text", 1u8),
                    "response.refusal.done" => ("content_index", "refusal", 2u8),
                    "response.reasoning_text.done" => ("content_index", "text", 4u8),
                    _ => ("summary_index", "text", 3u8),
                };
                let part_idx = data.get(part_key).and_then(Value::as_u64).unwrap_or(0);
                let raw_text = data
                    .get(field_key)
                    .and_then(Value::as_str)
                    .ok_or(RestoreError::RestoreFailed)?;
                let obj = data.as_object().ok_or(RestoreError::RestoreFailed)?;
                for (k, v) in obj.iter() {
                    check_no_tokens_in_str(k, &self.lease)?;
                    if k != field_key {
                        check_no_tokens_in_value(v, &self.lease)?;
                    }
                }
                self.failure_stage = RestoreStage::SseSnapshot;
                let budget = self.budget.clone();
                {
                    let track = self.get_or_create_text_part(out_idx, part_idx, kind)?;
                    if track.raw_done.is_some() || !track.pending_tail.as_slice().is_empty() {
                        return Err(RestoreError::RestoreFailed);
                    }
                    if track.saw_deltas && track.raw_deltas.as_slice() != raw_text.as_bytes() {
                        return Err(RestoreError::RestoreFailed);
                    }
                }
                self.failure_stage = RestoreStage::TokenRestore;
                let mut ignored_count = 0usize;
                let restore = restore_text_part(kind);
                let restored_done = restore(
                    raw_text,
                    &self.lease,
                    &budget,
                    &mut self.output_tokens,
                    &mut ignored_count,
                )
                .inspect_err(|err| {
                    if *err == RestoreError::FailedBecause(RestoreReason::TokenFormat) {
                        let carrier = match kind {
                            1 => Carrier::OutputText,
                            2 => Carrier::Refusal,
                            _ => Carrier::Reasoning,
                        };
                        self.token_format_diagnostics.observe(
                            raw_text,
                            0,
                            carrier,
                            Site::DoneRestore,
                            false,
                        );
                    }
                })?;
                self.failure_stage = RestoreStage::SseSnapshot;
                let saw_deltas = {
                    let track = self.get_or_create_text_part(out_idx, part_idx, kind)?;
                    if track.saw_deltas
                        && track.restored_deltas.as_slice() != restored_done.as_str().as_bytes()
                    {
                        return Err(RestoreError::RestoreFailed);
                    }
                    track.raw_done = Some(
                        PayloadText::try_from_str(raw_text, budget.clone(), MAX_FRAME_BYTES)
                            .map_err(|_| RestoreError::RestoreFailed)?,
                    );
                    track.restored_done = Some(
                        PayloadText::try_from_str(restored_done.as_str(), budget, MAX_FRAME_BYTES)
                            .map_err(|_| RestoreError::RestoreFailed)?,
                    );
                    track.saw_deltas
                };
                if !saw_deltas {
                    self.restored_occurrences = self
                        .restored_occurrences
                        .checked_add(ignored_count)
                        .ok_or(RestoreError::RestoreFailed)?;
                }
                replacements
                    .push((
                        data.get(field_key).ok_or(RestoreError::RestoreFailed)?,
                        restored_done,
                    ))
                    .map_err(|_| RestoreError::RestoreFailed)?;
            }
            "response.content_part.done" | "response.reasoning_summary_part.done" => {
                let (part_key, default_kind) = if evt == "response.content_part.done" {
                    ("content_index", 1u8)
                } else {
                    ("summary_index", 3u8)
                };
                let part_idx = data.get(part_key).and_then(Value::as_u64).unwrap_or(0);
                let part = data
                    .get("part")
                    .and_then(Value::as_object)
                    .ok_or(RestoreError::RestoreFailed)?;
                let part_type = part
                    .get("type")
                    .and_then(Value::as_str)
                    .ok_or(RestoreError::RestoreFailed)?;
                let (kind, field_key) = match part_type {
                    "output_text" => (1u8, "text"),
                    "refusal" => (2u8, "refusal"),
                    "summary_text" => (3u8, "text"),
                    "reasoning_text" => (4u8, "text"),
                    _ => {
                        check_no_tokens_in_value(data, &self.lease)?;
                        return Err(RestoreError::RestoreFailed);
                    }
                };
                if (default_kind == 3) != (kind == 3) {
                    return Err(RestoreError::RestoreFailed);
                }
                let raw_text = part
                    .get(field_key)
                    .and_then(Value::as_str)
                    .ok_or(RestoreError::RestoreFailed)?;
                let budget = self.budget.clone();
                if kind == 1 {
                    self.flush_output_text_tail(out_idx, part_idx)?;
                }
                let restored_str = {
                    let track = self.get_or_create_text_part(out_idx, part_idx, kind)?;
                    if track.part_done_seen || !track.pending_tail.as_slice().is_empty() {
                        return Err(RestoreError::RestoreFailed);
                    }
                    if track.saw_deltas && track.raw_deltas.as_slice() != raw_text.as_bytes() {
                        return Err(RestoreError::RestoreFailed);
                    }
                    if let Some(done) = &track.raw_done {
                        if done.as_str() != raw_text {
                            return Err(RestoreError::RestoreFailed);
                        }
                    }
                    track.part_done_seen = true;
                    track
                        .restored_done
                        .as_ref()
                        .map(|t| {
                            PayloadText::try_from_str(t.as_str(), budget.clone(), MAX_FRAME_BYTES)
                        })
                        .transpose()
                        .map_err(|_| RestoreError::RestoreFailed)?
                };
                let restored = match restored_str {
                    Some(s) => s,
                    None => {
                        let mut dummy = 0usize;
                        let restore = restore_text_part(kind);
                        restore(
                            raw_text,
                            &self.lease,
                            &self.budget,
                            &mut self.output_tokens,
                            &mut dummy,
                        )
                        .inspect_err(|err| {
                            if *err == RestoreError::FailedBecause(RestoreReason::TokenFormat) {
                                let carrier = match kind {
                                    1 => Carrier::OutputText,
                                    2 => Carrier::Refusal,
                                    _ => Carrier::Reasoning,
                                };
                                self.token_format_diagnostics.observe(
                                    raw_text,
                                    0,
                                    carrier,
                                    Site::SnapshotRestore,
                                    false,
                                );
                            }
                        })?
                    }
                };
                replacements
                    .push((
                        part.get(field_key).ok_or(RestoreError::RestoreFailed)?,
                        restored,
                    ))
                    .map_err(|_| RestoreError::RestoreFailed)?;
            }
            "response.output_item.done" => {
                let item = data.get("item").ok_or(RestoreError::RestoreFailed)?;
                let item_type = item
                    .get("type")
                    .and_then(Value::as_str)
                    .ok_or(RestoreError::RestoreFailed)?;
                match item_type {
                    "message" => {
                        let content = item
                            .get("content")
                            .and_then(Value::as_array)
                            .ok_or(RestoreError::RestoreFailed)?;
                        for (idx, part) in content.iter().enumerate() {
                            let part_idx = idx as u64;
                            let (kind, field_key) = match part.get("type").and_then(Value::as_str) {
                                Some("output_text") => (1u8, "text"),
                                Some("refusal") => (2u8, "refusal"),
                                _ => return Err(RestoreError::RestoreFailed),
                            };
                            let raw_text = part
                                .get(field_key)
                                .and_then(Value::as_str)
                                .ok_or(RestoreError::RestoreFailed)?;
                            if kind == 1 {
                                self.flush_output_text_tail(out_idx, part_idx)?;
                            }
                            let track = self.get_or_create_text_part(out_idx, part_idx, kind)?;
                            if track.item_done_seen || !track.pending_tail.as_slice().is_empty() {
                                return Err(RestoreError::RestoreFailed);
                            }
                            if track.saw_deltas
                                && track.raw_deltas.as_slice() != raw_text.as_bytes()
                            {
                                return Err(RestoreError::RestoreFailed);
                            }
                            if let Some(done) = &track.raw_done {
                                if done.as_str() != raw_text {
                                    return Err(RestoreError::RestoreFailed);
                                }
                            }
                            track.item_done_seen = true;
                        }
                        let mut dummy_count = 0usize;
                        let item = data.get("item").ok_or(RestoreError::RestoreFailed)?;
                        restore_output_item(
                            item,
                            false,
                            &self.lease,
                            &self.budget,
                            &mut self.output_tokens,
                            &mut dummy_count,
                            replacements,
                        )
                        .inspect_err(|err| {
                            if *err == RestoreError::FailedBecause(RestoreReason::TokenFormat) {
                                // The nested helper does not identify the failing leaf.
                                self.token_format_diagnostics.observe(
                                    "",
                                    0,
                                    Carrier::Other,
                                    Site::SnapshotRestore,
                                    false,
                                );
                            }
                        })?;
                    }
                    "reasoning" => {
                        for (field, kind) in [("summary", 3), ("content", 4)] {
                            if let Some(parts) = item.get(field) {
                                for (idx, part) in parts
                                    .as_array()
                                    .ok_or(RestoreError::RestoreFailed)?
                                    .iter()
                                    .enumerate()
                                {
                                    let raw_text = part
                                        .get("text")
                                        .and_then(Value::as_str)
                                        .ok_or(RestoreError::RestoreFailed)?;
                                    let track =
                                        self.get_or_create_text_part(out_idx, idx as u64, kind)?;
                                    if track.item_done_seen
                                        || !track.pending_tail.as_slice().is_empty()
                                    {
                                        return Err(RestoreError::RestoreFailed);
                                    }
                                    if track.saw_deltas
                                        && track.raw_deltas.as_slice() != raw_text.as_bytes()
                                    {
                                        return Err(RestoreError::RestoreFailed);
                                    }
                                    if track
                                        .raw_done
                                        .as_ref()
                                        .is_some_and(|done| done.as_str() != raw_text)
                                    {
                                        return Err(RestoreError::RestoreFailed);
                                    }
                                    track.item_done_seen = true;
                                }
                            }
                        }
                        let mut dummy_count = 0usize;
                        let item = data.get("item").ok_or(RestoreError::RestoreFailed)?;
                        restore_output_item(
                            item,
                            false,
                            &self.lease,
                            &self.budget,
                            &mut self.output_tokens,
                            &mut dummy_count,
                            replacements,
                        )
                        .inspect_err(|err| {
                            if *err == RestoreError::FailedBecause(RestoreReason::TokenFormat) {
                                // The nested helper does not identify the failing leaf.
                                self.token_format_diagnostics.observe(
                                    "",
                                    0,
                                    Carrier::Other,
                                    Site::SnapshotRestore,
                                    false,
                                );
                            }
                        })?;
                    }
                    _ => return Err(RestoreError::RestoreFailed),
                }
            }
            "response.output_item.added" => {
                let item = data.get("item").ok_or(RestoreError::RestoreFailed)?;
                let kind = item.get("type").and_then(Value::as_str);
                if kind == Some("reasoning") {
                    check_reasoning_item(item, &self.lease)?;
                    return Ok(());
                }
                if kind != Some("message") {
                    return Err(RestoreError::RestoreFailed);
                }
                let field = "content";
                if let Some(parts) = item.get(field) {
                    for part in parts.as_array().ok_or(RestoreError::RestoreFailed)? {
                        let key = if part.get("type").and_then(Value::as_str) == Some("refusal") {
                            "refusal"
                        } else {
                            "text"
                        };
                        if let Some(target) = part.get(key) {
                            let text = target.as_str().ok_or(RestoreError::RestoreFailed)?;
                            let restore = if field == "content"
                                && part.get("type").and_then(Value::as_str) == Some("output_text")
                            {
                                restore_output_text_with_lease
                            } else if field == "summary" {
                                copy_reasoning_text
                            } else {
                                restore_text_with_lease
                            };
                            let restored = restore(
                                text,
                                &self.lease,
                                &self.budget,
                                &mut self.output_tokens,
                                &mut self.restored_occurrences,
                            )
                            .inspect_err(|err| {
                                if *err == RestoreError::FailedBecause(RestoreReason::TokenFormat) {
                                    let carrier = match part.get("type").and_then(Value::as_str) {
                                        Some("output_text") => Carrier::OutputText,
                                        Some("refusal") => Carrier::Refusal,
                                        Some("summary_text") => Carrier::Reasoning,
                                        _ => Carrier::Other,
                                    };
                                    self.token_format_diagnostics.observe(
                                        text,
                                        0,
                                        carrier,
                                        Site::SnapshotRestore,
                                        false,
                                    );
                                }
                            })?;
                            replacements
                                .push((target, restored))
                                .map_err(|_| RestoreError::RestoreFailed)?;
                        }
                    }
                }
            }
            "response.content_part.added" | "response.reasoning_summary_part.added" => {
                let part = data.get("part").ok_or(RestoreError::RestoreFailed)?;
                let field = match part.get("type").and_then(Value::as_str) {
                    Some("output_text" | "summary_text" | "reasoning_text") => "text",
                    Some("refusal") => "refusal",
                    _ => return Err(RestoreError::RestoreFailed),
                };
                if let Some(target) = part.get(field) {
                    let text = target.as_str().ok_or(RestoreError::RestoreFailed)?;
                    let restore = if evt == "response.content_part.added"
                        && part.get("type").and_then(Value::as_str) == Some("output_text")
                    {
                        restore_output_text_with_lease
                    } else if matches!(
                        part.get("type").and_then(Value::as_str),
                        Some("summary_text" | "reasoning_text")
                    ) {
                        copy_reasoning_text
                    } else {
                        restore_text_with_lease
                    };
                    let restored = restore(
                        text,
                        &self.lease,
                        &self.budget,
                        &mut self.output_tokens,
                        &mut self.restored_occurrences,
                    )
                    .inspect_err(|err| {
                        if *err == RestoreError::FailedBecause(RestoreReason::TokenFormat) {
                            let carrier = match part.get("type").and_then(Value::as_str) {
                                Some("output_text") => Carrier::OutputText,
                                Some("refusal") => Carrier::Refusal,
                                _ => Carrier::Reasoning,
                            };
                            self.token_format_diagnostics.observe(
                                text,
                                0,
                                carrier,
                                Site::SnapshotRestore,
                                false,
                            );
                        }
                    })?;
                    replacements
                        .push((target, restored))
                        .map_err(|_| RestoreError::RestoreFailed)?;
                }
            }
            _ => {
                check_no_tokens_in_value(data, &self.lease)?;
            }
        }
        Ok(())
    }

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
            self.finalize_and_flush_inner(completion_permitted)
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
        res
    }

    fn finalize_and_flush_inner(
        &mut self,
        completion_permitted: bool,
    ) -> Result<Vec<Bytes>, RestoreError> {
        self.failure_stage = RestoreStage::CompletionEvidence;
        if !self.completed_seen {
            return Err(RestoreError::RestoreFailed);
        }
        let witness = CompletionWitness::from_verified_completion(completion_permitted)
            .map_err(|_| RestoreError::RestoreFailed)?;

        self.failure_stage = RestoreStage::SseSnapshot;
        // Ordinary text tails remain undecided until the complete raw field is
        // restored below: post-barrier deltas may complete a pending reference.
        // Strict carriers retain their existing unfinished-tail rejection.
        for part in self.text_parts.as_slice() {
            if part.kind != 1 && token_format::header(part.pending_tail.as_slice()).is_some() {
                return Err(RestoreError::RestoreFailed);
            }
        }

        // Locate the final `response.completed` event in `barrier_events`.
        let completed_pos = self
            .barrier_events
            .as_slice()
            .iter()
            .position(|e| e.event_name.as_str() == "response.completed")
            .ok_or(RestoreError::RestoreFailed)?;
        if completed_pos + 1 != self.barrier_events.len() {
            return Err(RestoreError::RestoreFailed);
        }

        // Validate all tracked text parts and tool items against `response.completed`.
        let response_owner = PayloadJson::try_from_value(
            self.barrier_events.as_slice()[completed_pos]
                .data
                .as_value()
                .get("response")
                .ok_or(RestoreError::RestoreFailed)?,
            self.budget.clone(),
            self.budget.limit(),
        )
        .map_err(|_| RestoreError::RestoreFailed)?;
        let response_val = response_owner.as_value();
        let output_arr = response_val
            .get("output")
            .and_then(Value::as_array)
            .ok_or(RestoreError::RestoreFailed)?;

        for raw in self.raw_snapshots.as_slice() {
            validate_raw_snapshot(raw.as_value(), output_arr, self.norm_mode, &self.budget)?;
        }
        for event in self.barrier_events.as_slice() {
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
                Some("message") => {
                    let content = item
                        .get("content")
                        .and_then(Value::as_array)
                        .ok_or(RestoreError::RestoreFailed)?;
                    for (part_idx, part) in content.iter().enumerate() {
                        let (kind, key) = match part.get("type").and_then(Value::as_str) {
                            Some("output_text") => (1u8, "text"),
                            Some("refusal") => (2u8, "refusal"),
                            _ => return Err(RestoreError::RestoreFailed),
                        };
                        let final_raw = part
                            .get(key)
                            .and_then(Value::as_str)
                            .ok_or(RestoreError::RestoreFailed)?;
                        let budget = self.budget.clone();
                        let track = self.get_or_create_text_part(out_idx, part_idx as u64, kind)?;
                        if track.saw_deltas && track.raw_deltas.as_slice() != final_raw.as_bytes() {
                            return Err(RestoreError::RestoreFailed);
                        }
                        if let Some(done) = &track.raw_done {
                            if done.as_str() != final_raw {
                                return Err(RestoreError::RestoreFailed);
                            }
                        }
                        if track.restored_done.is_none() {
                            let mut count = 0usize;
                            self.failure_stage = RestoreStage::TokenRestore;
                            let restore = if kind == 1 {
                                restore_output_text_with_lease
                            } else {
                                restore_text_with_lease
                            };
                            let restored = restore(
                                final_raw,
                                &self.lease,
                                &budget,
                                &mut self.output_tokens,
                                &mut count,
                            )
                            .inspect_err(|err| {
                                if *err == RestoreError::FailedBecause(RestoreReason::TokenFormat) {
                                    let carrier = if kind == 1 {
                                        Carrier::OutputText
                                    } else {
                                        Carrier::Refusal
                                    };
                                    self.token_format_diagnostics.observe(
                                        final_raw,
                                        0,
                                        carrier,
                                        Site::FinalTextRestore,
                                        false,
                                    );
                                }
                            })?;
                            self.failure_stage = RestoreStage::SseSnapshot;
                            self.restored_occurrences = self
                                .restored_occurrences
                                .checked_add(count)
                                .ok_or(RestoreError::RestoreFailed)?;
                            let track =
                                self.get_or_create_text_part(out_idx, part_idx as u64, kind)?;
                            track.restored_done = Some(restored);
                        }
                    }
                }
                Some("reasoning") => {
                    check_reasoning_item(item, &self.lease)?;
                    for (field, kind) in [("summary", 3), ("content", 4)] {
                        if let Some(parts) = item.get(field) {
                            for (part_idx, part) in parts
                                .as_array()
                                .ok_or(RestoreError::RestoreFailed)?
                                .iter()
                                .enumerate()
                            {
                                let final_raw = part
                                    .get("text")
                                    .and_then(Value::as_str)
                                    .ok_or(RestoreError::RestoreFailed)?;
                                let budget = self.budget.clone();
                                let track =
                                    self.get_or_create_text_part(out_idx, part_idx as u64, kind)?;
                                if track.saw_deltas
                                    && track.raw_deltas.as_slice() != final_raw.as_bytes()
                                {
                                    return Err(RestoreError::RestoreFailed);
                                }
                                if track
                                    .raw_done
                                    .as_ref()
                                    .is_some_and(|done| done.as_str() != final_raw)
                                {
                                    return Err(RestoreError::RestoreFailed);
                                }
                                if track.restored_done.is_none() {
                                    track.restored_done = Some(
                                        PayloadText::try_from_str(
                                            final_raw,
                                            budget,
                                            MAX_FRAME_BYTES,
                                        )
                                        .map_err(RestoreError::capacity)?,
                                    );
                                }
                            }
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
                    let mut count = 0usize;
                    self.failure_stage = RestoreStage::TokenRestore;
                    let restored = restore_json_arguments_string(
                        final_raw,
                        &self.lease,
                        &budget,
                        &mut self.output_tokens,
                        &mut count,
                    )
                    .inspect_err(|err| {
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
                    self.restored_occurrences = self
                        .restored_occurrences
                        .checked_add(count)
                        .ok_or(RestoreError::RestoreFailed)?;
                    let track = self.get_or_create_tool_item(out_idx, false)?;
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
                    let mut count = 0usize;
                    self.failure_stage = RestoreStage::TokenRestore;
                    let restored = restore_text_with_lease(
                        final_raw,
                        &self.lease,
                        &budget,
                        &mut self.output_tokens,
                        &mut count,
                    )
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
                    self.restored_occurrences = self
                        .restored_occurrences
                        .checked_add(count)
                        .ok_or(RestoreError::RestoreFailed)?;
                    let track = self.get_or_create_tool_item(out_idx, true)?;
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
            let Some(restored_done) = &part.restored_done else {
                return Err(RestoreError::RestoreFailed);
            };
            if part.saw_deltas
                && !restored_done
                    .as_str()
                    .as_bytes()
                    .starts_with(part.restored_deltas.as_slice())
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
        // Prepare all immutable snapshots and frames before publication can authorize continuity.
        let restored_response = restore_completed_snapshot(
            response_val,
            &self.lease,
            &self.budget,
            &mut self.output_tokens,
        )?;
        let mut emitted_tool_delta =
            PayloadVec::new(self.budget.clone()).map_err(|_| RestoreError::RestoreFailed)?;
        let mut emitted_text_delta =
            PayloadVec::new(self.budget.clone()).map_err(|_| RestoreError::RestoreFailed)?;
        let mut out_frames = Vec::with_capacity(self.barrier_events.len());
        for event in self.barrier_events.as_slice() {
            let evt = event.event_name.as_str();
            let data = event.data.as_value();
            let out_idx = data
                .get("output_index")
                .and_then(Value::as_u64)
                .unwrap_or(0);
            let mut edits =
                PayloadVec::new(self.budget.clone()).map_err(|_| RestoreError::RestoreFailed)?;
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
                    let restored = self
                        .tool_items
                        .as_slice()
                        .iter()
                        .find(|t| t.output_index == out_idx && t.is_custom == custom)
                        .and_then(|t| t.restored_payload.as_ref())
                        .ok_or(RestoreError::RestoreFailed)?;
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
                                .map_err(|_| RestoreError::RestoreFailed)?;
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
                        .map_err(|_| RestoreError::RestoreFailed)?;
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
                    let part_idx = data.get(part_key).and_then(Value::as_u64).unwrap_or(0);
                    let restored = self
                        .text_parts
                        .as_slice()
                        .iter()
                        .find(|p| {
                            p.output_index == out_idx && p.part_index == part_idx && p.kind == kind
                        })
                        .and_then(|p| p.restored_done.as_ref())
                        .ok_or(RestoreError::RestoreFailed)?;
                    let key_tuple = (out_idx, part_idx, kind);
                    let track = self
                        .text_parts
                        .as_slice()
                        .iter()
                        .find(|p| {
                            p.output_index == out_idx && p.part_index == part_idx && p.kind == kind
                        })
                        .ok_or(RestoreError::RestoreFailed)?;
                    let text = if evt.ends_with(".delta") {
                        if emitted_text_delta.as_slice().contains(&key_tuple) {
                            ""
                        } else {
                            emitted_text_delta
                                .push(key_tuple)
                                .map_err(|_| RestoreError::RestoreFailed)?;
                            restored
                                .as_str()
                                .get(track.released_deltas_len..)
                                .ok_or(RestoreError::RestoreFailed)?
                        }
                    } else {
                        restored.as_str()
                    };
                    if evt.ends_with(".added")
                        && data.get("part").and_then(|p| p.get(field)).is_none()
                    {
                        // Missing added text makes no content claim and stays missing on replay.
                        check_no_tokens_in_value(data, &self.lease)?;
                        out_frames.push(encode_sse_frame(evt, data, &self.budget)?);
                        continue;
                    }
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
                            .map_err(|_| RestoreError::RestoreFailed)?;
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
                    item_string_edits(raw, restored, evt.ends_with(".added"), &mut edits)?;
                }
                _ => {}
            }
            let replay = PayloadJson::try_with_replacements(
                data,
                edits.as_slice(),
                self.budget.clone(),
                self.budget.limit(),
            )
            .map_err(|_| RestoreError::RestoreFailed)?;
            check_restored_event(evt, replay.as_value(), &self.lease)?;
            out_frames.push(encode_sse_frame(evt, replay.as_value(), &self.budget)?);
        }
        self.failure_stage = RestoreStage::ContextPublish;
        publish_completed_snapshot(
            response_val,
            &self.output_tokens,
            &self.lease,
            &self.binding,
            &witness,
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
    raw: SseRestorationState,
    evidence: super::completion_json::NativeSseCompletion,
    bytes: usize,
    frames: usize,
    finished: bool,
}

#[cfg(test)]
impl TextProjectionStream {
    pub(super) fn new(lease: DlpRequestLease, budget: PayloadBudget) -> Result<Self, RestoreError> {
        // The binding is test-local; projection never uses it to publish anything.
        let binding = lease
            .bind_actual_attempt(
                "projection-fixture",
                "openai_responses",
                "http://127.0.0.1:1/responses",
                None,
            )
            .map_err(|_| RestoreError::RestoreFailed)?;
        let mut raw =
            SseRestorationState::new(lease, binding, budget, ArgumentNormalizationMode::Exact)?;
        raw.barrier_active = true;
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
        let before = self.raw.barrier_events.len();
        let released = self.raw.process_sse_block(block)?;
        if !released.is_empty() {
            return Err(RestoreError::RestoreFailed);
        }
        if self.raw.barrier_events.len() != before {
            let event = &self.raw.barrier_events.as_slice()[before];
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
        let events = self.raw.barrier_events.as_slice();
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
        for track in self.raw.text_parts.as_slice() {
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
                || (track.saw_deltas && track.raw_deltas.as_slice() != final_text.as_bytes())
                || track
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
                    item_string_edits(item, projected_item, evt.ends_with(".added"), &mut edits)?;
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
