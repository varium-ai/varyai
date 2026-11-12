//! Native SSE, complete JSON, compact JSON and normalization completion checks.
//!
//! All strict checks use `PayloadJson` / `PayloadBuffer` / `PayloadText` with
//! precharged budgets; no lossy repair or synthetic completion can authorize
//! completed-context publication.
use super::{
    completion_evidence::{
        AnthropicCompletionReason, ChatCompletionReason, CompletionObservation,
        GeminiCompletionReason, NativeCompletionEvent,
    },
    payload_budget::PayloadBudget,
    payload_buffer::PayloadBuffer,
    payload_json::PayloadJson,
    payload_text::PayloadText,
    tokenization::TokenError,
};
use serde_json::{Number, Value};

const FRAME_LIMIT: usize = 1024 * 1024;
const ARGUMENT_LIMIT: usize = 1024 * 1024;

pub(crate) struct NativeSseCompletion {
    legacy_completed: bool,
    completed: bool,
    invalid: bool,
    pending_raw: Option<PayloadJson>,
    pending_event_name: Option<PayloadText>,
    pending_done: bool,
}

impl Default for NativeSseCompletion {
    fn default() -> Self {
        Self::new()
    }
}

impl NativeSseCompletion {
    pub(crate) fn new() -> Self {
        Self {
            legacy_completed: false,
            completed: false,
            invalid: false,
            pending_raw: None,
            pending_event_name: None,
            pending_done: false,
        }
    }

    pub(crate) fn legacy_completed(&self) -> bool {
        self.legacy_completed
    }

    pub(crate) fn finish_legacy(&mut self) {
        self.legacy_completed = true;
    }

    pub(super) fn observe(&mut self, fact: &CompletionObservation<'_>, budget: &PayloadBudget) {
        match fact {
            CompletionObservation::RawFrame(raw) => {
                if self.raw_frame(raw, budget).is_err() {
                    self.invalid = true;
                }
            }
            CompletionObservation::NativeEvent { event_name, data } => {
                if NativeCompletionEvent::new(event_name, data).legacy_is_completed() {
                    self.legacy_completed = true;
                }
                if self.native_event(event_name, data, budget).is_err() {
                    self.invalid = true;
                }
            }
            CompletionObservation::BufferedBodyEnd { unfinished_frame } => {
                self.invalid |= *unfinished_frame || self.pending_raw.is_some();
            }
            CompletionObservation::EntityEnd {
                reached_eof,
                unfinished_frame,
                unfinished_utf8,
            } => {
                self.invalid |= !*reached_eof
                    || *unfinished_frame
                    || *unfinished_utf8
                    || self.pending_raw.is_some();
            }
            CompletionObservation::InvalidJson
            | CompletionObservation::InvalidUtf8
            | CompletionObservation::IoFailure
            | CompletionObservation::RawJson(_)
            | CompletionObservation::ChatChunk(_)
            | CompletionObservation::ChatDone
            | CompletionObservation::GeminiChunk(_)
            | CompletionObservation::AnthropicEvent { .. }
            | CompletionObservation::AnthropicJson(_) => {
                self.invalid = true;
            }
        }
    }

    fn raw_frame(&mut self, raw: &str, budget: &PayloadBudget) -> Result<(), TokenError> {
        if raw.len() > FRAME_LIMIT || self.pending_raw.is_some() || self.pending_done {
            return Err(TokenError::RedactionFailed);
        }
        let mut event_name: Option<&str> = None;
        let mut data_bytes = 0usize;
        let mut data_count = 0usize;
        for line in raw.lines() {
            if line.is_empty() || line.starts_with(':') {
                continue;
            }
            let (name, value) = line.split_once(':').unwrap_or((line, ""));
            let value = value.strip_prefix(' ').unwrap_or(value);
            match name {
                "event" if event_name.is_none() => event_name = Some(value.trim()),
                "data" => {
                    data_bytes = data_bytes
                        .checked_add(value.len())
                        .and_then(|n| n.checked_add(usize::from(data_count != 0)))
                        .ok_or(TokenError::CapacityExceeded)?;
                    data_count += 1;
                }
                "id" | "retry" => {}
                _ => return Err(TokenError::RedactionFailed),
            }
        }
        if data_count == 0 {
            return if event_name.is_none() {
                Ok(())
            } else {
                Err(TokenError::RedactionFailed)
            };
        }
        let mut data = PayloadBuffer::with_capacity(data_bytes, budget.clone(), FRAME_LIMIT)?;
        let mut first = true;
        for line in raw.lines() {
            let trimmed = line.trim_start();
            if let Some(value) = trimmed
                .strip_prefix("data:")
                .or_else(|| (trimmed == "data").then_some(""))
            {
                if !first {
                    data.append(b"\n")?;
                }
                first = false;
                data.append(value.strip_prefix(' ').unwrap_or(value).as_bytes())?;
            }
        }
        let encoded =
            std::str::from_utf8(data.as_slice()).map_err(|_| TokenError::RedactionFailed)?;
        if encoded.trim() == "[DONE]" {
            self.pending_done = false;
            return Ok(());
        }
        if self.completed {
            return Err(TokenError::RedactionFailed);
        }
        self.pending_raw = Some(PayloadJson::try_from_str(
            encoded,
            budget.clone(),
            budget.limit(),
        )?);
        self.pending_event_name = Some(PayloadText::try_from_str(
            event_name.unwrap_or(""),
            budget.clone(),
            256,
        )?);
        Ok(())
    }

    fn native_event(
        &mut self,
        event_name: &str,
        data: &Value,
        budget: &PayloadBudget,
    ) -> Result<(), TokenError> {
        let Some(raw_event_name) = self.pending_event_name.take() else {
            return Err(TokenError::RedactionFailed);
        };
        let Some(raw) = self.pending_raw.take() else {
            return Err(TokenError::RedactionFailed);
        };
        if raw_event_name.as_str() != event_name
            || !super::request_carriers::same_snapshot(raw.as_value(), data)
        {
            return Err(TokenError::RedactionFailed);
        }
        if let Some(kind) = data.get("type").and_then(Value::as_str) {
            if !event_name.is_empty() && event_name != kind {
                return Err(TokenError::RedactionFailed);
            }
        }
        // Pair the raw header first; data-only SSE uses its validated protocol type.
        let event_name = if event_name.is_empty() {
            data.get("type")
                .and_then(Value::as_str)
                .ok_or(TokenError::RedactionFailed)?
        } else {
            event_name
        };
        if event_name == "response.failed"
            || event_name == "error"
            || data.get("error").is_some_and(|v| !v.is_null())
        {
            return Err(TokenError::RedactionFailed);
        }
        if event_name == "response.output_item.done" {
            let item = data.get("item").ok_or(TokenError::RedactionFailed)?;
            validate_native_output_item(item, budget)?;
        } else if event_name == "response.completed" {
            if self.completed || !NativeCompletionEvent::new(event_name, data).permits_completion()
            {
                return Err(TokenError::RedactionFailed);
            }
            let response = data.get("response").ok_or(TokenError::RedactionFailed)?;
            validate_native_response_object(response, budget)?;
            self.completed = true;
        }
        Ok(())
    }

    pub(crate) fn permits_completion(&self) -> bool {
        self.completed
            && !self.invalid
            && self.pending_raw.is_none()
            && self.pending_event_name.is_none()
    }
}

#[derive(Clone, Copy)]
pub(crate) enum JsonCompletionProtocol {
    NativeResponses,
    ChatCompletions,
    AnthropicMessages,
    ResponsesCompact,
}

pub(crate) struct JsonCompletion {
    protocol: JsonCompletionProtocol,
    native_chat: bool,
    native_gemini: bool,
    seen: bool,
    permitted: bool,
    unsupported_carrier: bool,
}

impl JsonCompletion {
    pub(crate) fn new(protocol: JsonCompletionProtocol) -> Self {
        Self {
            protocol,
            native_chat: false,
            native_gemini: false,
            seen: false,
            permitted: false,
            unsupported_carrier: false,
        }
    }

    /// Native Chat preserves independently indexed choices; conversion stays single-choice.
    pub(super) fn native_chat() -> Self {
        Self {
            native_chat: true,
            ..Self::new(JsonCompletionProtocol::ChatCompletions)
        }
    }

    /// Private native selection avoids widening integration-owned protocol enums.
    pub(super) fn native_gemini() -> Self {
        Self {
            native_gemini: true,
            ..Self::new(JsonCompletionProtocol::ChatCompletions)
        }
    }

    pub(super) fn has_unsupported_carrier(&self) -> bool {
        self.unsupported_carrier
    }

    pub(super) fn observe_raw_json(&mut self, raw: &str, budget: &PayloadBudget) {
        drop(self.observe_raw_json_scoped(raw, budget));
    }

    /// The caller may inspect the exact strict parse before conversion, without retaining a copy.
    pub(super) fn observe_raw_json_scoped(
        &mut self,
        raw: &str,
        budget: &PayloadBudget,
    ) -> Option<PayloadJson> {
        if self.seen {
            self.permitted = false;
            return None;
        }
        self.seen = true;
        let Ok(parsed) = PayloadJson::try_from_str(raw, budget.clone(), budget.limit()) else {
            self.permitted = false;
            return None;
        };
        let value = parsed.as_value();
        self.permitted = if self.native_gemini {
            validate_gemini_json_response(value, &mut self.unsupported_carrier).is_ok()
        } else {
            match self.protocol {
                JsonCompletionProtocol::NativeResponses => {
                    validate_native_response_object(value, budget).is_ok()
                }
                JsonCompletionProtocol::ChatCompletions => validate_chat_json_response(
                    value,
                    budget,
                    &mut self.unsupported_carrier,
                    self.native_chat,
                )
                .is_ok(),
                JsonCompletionProtocol::AnthropicMessages => {
                    validate_anthropic_json_response(value, budget, &mut self.unsupported_carrier)
                        .is_ok()
                }
                JsonCompletionProtocol::ResponsesCompact => {
                    validate_compact_json_response(value).is_ok()
                }
            }
        };
        self.permitted.then_some(parsed)
    }

    pub(crate) fn permits_completion(&self) -> bool {
        self.seen && self.permitted
    }
}

fn valid_external_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 256 && !id.chars().any(|c| c.is_whitespace() || c.is_control())
}

fn validate_native_response_object(
    response: &Value,
    budget: &PayloadBudget,
) -> Result<(), TokenError> {
    let obj = response.as_object().ok_or(TokenError::RedactionFailed)?;
    let id = obj
        .get("id")
        .and_then(Value::as_str)
        .filter(|id| valid_external_id(id))
        .ok_or(TokenError::RedactionFailed)?;
    let _ = id;
    if obj.get("status").and_then(Value::as_str) != Some("completed")
        || obj.get("error").is_some_and(|v| !v.is_null())
        || obj.get("incomplete_details").is_some_and(|v| !v.is_null())
    {
        return Err(TokenError::RedactionFailed);
    }
    let output = obj
        .get("output")
        .and_then(Value::as_array)
        .ok_or(TokenError::RedactionFailed)?;
    for item in output {
        validate_native_output_item(item, budget)?;
    }
    Ok(())
}

fn validate_native_output_item(item: &Value, budget: &PayloadBudget) -> Result<(), TokenError> {
    let obj = item.as_object().ok_or(TokenError::RedactionFailed)?;
    let kind = obj
        .get("type")
        .and_then(Value::as_str)
        .ok_or(TokenError::RedactionFailed)?;
    if let Some(status) = obj.get("status").and_then(Value::as_str) {
        if status != "completed" {
            return Err(TokenError::RedactionFailed);
        }
    }
    match kind {
        "message" => {
            if !obj
                .get("id")
                .and_then(Value::as_str)
                .is_some_and(valid_external_id)
                || obj.get("role").and_then(Value::as_str) != Some("assistant")
            {
                return Err(TokenError::RedactionFailed);
            }
            let content = obj
                .get("content")
                .and_then(Value::as_array)
                .ok_or(TokenError::RedactionFailed)?;
            for part in content {
                match part.get("type").and_then(Value::as_str) {
                    Some("output_text") => {
                        if !part.get("text").is_some_and(Value::is_string) {
                            return Err(TokenError::RedactionFailed);
                        }
                    }
                    Some("refusal") => {
                        if !part.get("refusal").is_some_and(Value::is_string) {
                            return Err(TokenError::RedactionFailed);
                        }
                    }
                    _ => return Err(TokenError::RedactionFailed),
                }
            }
        }
        "reasoning" => {
            if !obj
                .get("id")
                .and_then(Value::as_str)
                .is_some_and(valid_external_id)
            {
                return Err(TokenError::RedactionFailed);
            }
            for (field, expected_kind) in
                [("summary", "summary_text"), ("content", "reasoning_text")]
            {
                if let Some(parts) = obj.get(field) {
                    for part in parts.as_array().ok_or(TokenError::RedactionFailed)? {
                        if part.get("type").and_then(Value::as_str) != Some(expected_kind)
                            || !part.get("text").is_some_and(Value::is_string)
                        {
                            return Err(TokenError::RedactionFailed);
                        }
                    }
                }
            }
            if let Some(enc) = obj.get("encrypted_content") {
                if !enc.is_null() && !enc.as_str().is_some_and(|s| !s.is_empty()) {
                    return Err(TokenError::RedactionFailed);
                }
            }
        }
        "function_call" => {
            if !obj
                .get("call_id")
                .and_then(Value::as_str)
                .is_some_and(valid_external_id)
                || !obj
                    .get("name")
                    .and_then(Value::as_str)
                    .is_some_and(valid_external_id)
            {
                return Err(TokenError::RedactionFailed);
            }
            let args = obj
                .get("arguments")
                .and_then(Value::as_str)
                .filter(|s| !s.trim().is_empty())
                .ok_or(TokenError::RedactionFailed)?;
            let parsed = PayloadJson::try_from_str(args, budget.clone(), budget.limit())?;
            if !parsed.as_value().is_object() {
                return Err(TokenError::RedactionFailed);
            }
        }
        "custom_tool_call" => {
            if !obj
                .get("call_id")
                .and_then(Value::as_str)
                .is_some_and(valid_external_id)
                || !obj
                    .get("name")
                    .and_then(Value::as_str)
                    .is_some_and(valid_external_id)
                || !obj.get("input").is_some_and(Value::is_string)
            {
                return Err(TokenError::RedactionFailed);
            }
        }
        _ => return Err(TokenError::RedactionFailed),
    }
    Ok(())
}

// Return whether the converter selects nonempty text; None means data would be ignored.
fn chat_reasoning_object_supported(value: &Value, details: bool, depth: usize) -> Option<bool> {
    if depth > 32 {
        return None;
    }
    let object = value.as_object()?;
    let mut selected = 0usize;
    let mut parts_selected = false;
    for (key, value) in object {
        match key.as_str() {
            "text" | "content" | "summary" => match value {
                Value::Null => {}
                Value::String(text) => selected += usize::from(!text.is_empty()),
                _ => return None,
            },
            "parts" if details => {
                let parts = value.as_array()?;
                for part in parts {
                    parts_selected |= chat_reasoning_object_supported(part, true, depth + 1)?;
                }
            }
            "type" if details && value.as_str() == Some("reasoning_text") => {}
            _ => return None,
        }
    }
    if selected > 1 || selected != 0 && parts_selected {
        return None;
    }
    Some(selected != 0 || parts_selected)
}

fn chat_reasoning_carriers_supported(message: &serde_json::Map<String, Value>) -> bool {
    let mut selected = 0usize;
    for field in ["reasoning_content", "reasoning", "reasoning_details"] {
        let Some(value) = message.get(field) else {
            continue;
        };
        let active = match value {
            Value::Null => Some(false),
            Value::String(text) => Some(!text.is_empty()),
            Value::Object(_) if field != "reasoning_content" => {
                chat_reasoning_object_supported(value, field == "reasoning_details", 0)
            }
            Value::Array(parts) if field == "reasoning_details" => {
                let mut active = false;
                for part in parts {
                    let Some(part_active) = chat_reasoning_object_supported(part, true, 0) else {
                        return false;
                    };
                    active |= part_active;
                }
                Some(active)
            }
            _ => None,
        };
        let Some(active) = active else {
            return false;
        };
        selected += usize::from(active);
    }
    selected <= 1
}

/// Match readable Chat carriers to the JSON/SSE converters before they can drop data.
/// Validate reasoning containers before priority extraction can discard their siblings.
pub(super) fn chat_carriers_supported(
    message: &serde_json::Map<String, Value>,
    streaming: bool,
) -> bool {
    if !chat_reasoning_carriers_supported(message)
        || message.iter().any(|(key, value)| {
            !value.is_null()
                && !matches!(
                    key.as_str(),
                    "role"
                        | "content"
                        | "refusal"
                        | "reasoning_content"
                        | "reasoning"
                        | "reasoning_details"
                        | "tool_calls"
                )
        })
    {
        return false;
    }
    if message
        .get("refusal")
        .is_some_and(|value| !value.is_null() && (streaming || !value.is_string()))
        || message.get("tool_calls").is_some_and(|value| {
            !value.is_null()
                && !super::completion_tools::chat_tool_carriers_supported(value, streaming)
        })
    {
        return false;
    }
    match message.get("content") {
        None | Some(Value::Null | Value::String(_)) => true,
        Some(Value::Array(parts)) if !streaming => parts.iter().all(|part| {
            let Some(object) = part.as_object() else {
                return false;
            };
            let field = match object.get("type").and_then(Value::as_str) {
                Some("text" | "output_text") => "text",
                Some("refusal") => "refusal",
                _ => return false,
            };
            object.get(field).is_some_and(Value::is_string)
                && object
                    .iter()
                    .all(|(key, value)| value.is_null() || key == "type" || key == field)
        }),
        _ => false,
    }
}

/// Native carriers retain opaque siblings instead of passing through a lossy converter.
/// Known readable fields still require their protocol types; tool structure is checked
/// by the same strict completion validator that owns tool identities and arguments.
pub(super) fn native_chat_carriers_supported(
    message: &serde_json::Map<String, Value>,
    streaming: bool,
) -> bool {
    for field in ["refusal", "reasoning_content"] {
        if message
            .get(field)
            .is_some_and(|value| !value.is_null() && !value.is_string())
        {
            return false;
        }
    }
    match message.get("content") {
        None | Some(Value::Null | Value::String(_)) => true,
        Some(Value::Array(parts)) if !streaming => parts.iter().all(|part| {
            let Some(object) = part.as_object() else {
                return false;
            };
            match object.get("type").and_then(Value::as_str) {
                Some("text" | "output_text") => object.get("text").is_some_and(Value::is_string),
                Some("refusal") => object.get("refusal").is_some_and(Value::is_string),
                Some(_) => true,
                None => false,
            }
        }),
        _ => false,
    }
}

fn validate_chat_json_response(
    body: &Value,
    budget: &PayloadBudget,
    unsupported_carrier: &mut bool,
    native_chat: bool,
) -> Result<(), TokenError> {
    let obj = body.as_object().ok_or(TokenError::RedactionFailed)?;
    if obj.get("error").is_some_and(|v| !v.is_null())
        || !obj
            .get("id")
            .and_then(Value::as_str)
            .is_some_and(valid_external_id)
    {
        return Err(TokenError::RedactionFailed);
    }
    let choices = obj
        .get("choices")
        .and_then(Value::as_array)
        .ok_or(TokenError::RedactionFailed)?;
    if native_chat {
        if choices.is_empty() || choices.len() > 128 {
            return Err(TokenError::RedactionFailed);
        }
        for (position, choice) in choices.iter().enumerate() {
            let index = choice
                .get("index")
                .and_then(Value::as_u64)
                .filter(|index| *index < 128)
                .ok_or(TokenError::RedactionFailed)?;
            if choices[..position]
                .iter()
                .any(|prior| prior.get("index").and_then(Value::as_u64) == Some(index))
            {
                return Err(TokenError::RedactionFailed);
            }
            validate_chat_json_choice(choice, budget, unsupported_carrier, true)?;
        }
        Ok(())
    } else {
        if choices.len() != 1 {
            return Err(TokenError::RedactionFailed);
        }
        validate_chat_json_choice(&choices[0], budget, unsupported_carrier, false)
    }
}

fn validate_chat_json_choice(
    choice: &Value,
    budget: &PayloadBudget,
    unsupported_carrier: &mut bool,
    native_chat: bool,
) -> Result<(), TokenError> {
    let choice = choice.as_object().ok_or(TokenError::RedactionFailed)?;
    let reason = choice.get("finish_reason").and_then(Value::as_str);
    if !ChatCompletionReason::new(reason).permits_completion() {
        return Err(TokenError::RedactionFailed);
    }
    let message = choice.get("message").ok_or(TokenError::RedactionFailed)?;
    validate_chat_message(message, reason, budget, unsupported_carrier, native_chat)
}

/// Structural validation only; no transport, release or completion authority.
/// Shared by complete native JSON and independently composed Chat SSE snapshots.
pub(super) fn validate_chat_message(
    message: &Value,
    reason: Option<&str>,
    budget: &PayloadBudget,
    unsupported_carrier: &mut bool,
    native_chat: bool,
) -> Result<(), TokenError> {
    let message = message.as_object().ok_or(TokenError::RedactionFailed)?;
    if message
        .get("role")
        .filter(|v| !v.is_null())
        .is_some_and(|r| r.as_str() != Some("assistant"))
        || message.get("function_call").is_some_and(|v| !v.is_null())
    {
        return Err(TokenError::RedactionFailed);
    }
    let supported = if native_chat {
        native_chat_carriers_supported(message, false)
    } else {
        chat_carriers_supported(message, false)
    };
    if !supported {
        *unsupported_carrier = true;
        return Err(TokenError::RedactionFailed);
    }
    let tool_reason = matches!(reason, Some("tool_calls" | "function_call"));
    if let Some(tools) = message.get("tool_calls").filter(|v| !v.is_null()) {
        let tools = tools.as_array().ok_or(TokenError::RedactionFailed)?;
        if tools.is_empty() || tools.len() > 128 || !tool_reason {
            return Err(TokenError::RedactionFailed);
        }
        for (idx, tool) in tools.iter().enumerate() {
            if tool.get("type").and_then(Value::as_str) != Some("function") {
                return Err(TokenError::RedactionFailed);
            }
            let id = tool
                .get("id")
                .and_then(Value::as_str)
                .filter(|id| valid_external_id(id))
                .ok_or(TokenError::RedactionFailed)?;
            if tools[..idx]
                .iter()
                .any(|prior| prior.get("id").and_then(Value::as_str) == Some(id))
            {
                return Err(TokenError::RedactionFailed);
            }
            let func = tool
                .get("function")
                .and_then(Value::as_object)
                .ok_or(TokenError::RedactionFailed)?;
            if !func
                .get("name")
                .and_then(Value::as_str)
                .is_some_and(valid_external_id)
            {
                return Err(TokenError::RedactionFailed);
            }
            let args = func
                .get("arguments")
                .and_then(Value::as_str)
                .filter(|s| !s.trim().is_empty() && s.len() <= ARGUMENT_LIMIT)
                .ok_or(TokenError::RedactionFailed)?;
            let parsed = PayloadJson::try_from_str(args, budget.clone(), budget.limit())?;
            if !parsed.as_value().is_object() {
                return Err(TokenError::RedactionFailed);
            }
        }
    } else if tool_reason {
        return Err(TokenError::RedactionFailed);
    }
    Ok(())
}

// Only carrier keys are closed-world; tool inputs and opaque values stay untouched.
pub(super) fn anthropic_carrier_keys_supported(carrier: &Value, keys: &[&str]) -> bool {
    carrier
        .as_object()
        .is_none_or(|object| object.keys().all(|key| keys.contains(&key.as_str())))
}

pub(super) fn anthropic_block_carrier_supported(block: &Value) -> bool {
    let keys: &[&str] = match block.get("type").and_then(Value::as_str) {
        Some("text") => &["type", "text"],
        Some("thinking") => &["type", "thinking", "signature"],
        Some("redacted_thinking") => &["type", "data"],
        Some("tool_use") => &["type", "id", "name", "input"],
        Some(_) => return false,
        // Missing or malformed known fields remain ordinary completion failures.
        None => return true,
    };
    anthropic_carrier_keys_supported(block, keys)
}

pub(super) fn anthropic_delta_carrier_supported(delta: &Value) -> bool {
    let keys: &[&str] = match delta.get("type").and_then(Value::as_str) {
        Some("text_delta") => &["type", "text"],
        Some("thinking_delta") => &["type", "thinking"],
        Some("signature_delta") => &["type", "signature"],
        Some("input_json_delta") => &["type", "partial_json"],
        Some(_) => return false,
        None => return true,
    };
    anthropic_carrier_keys_supported(delta, keys)
}

fn validate_anthropic_json_response(
    body: &Value,
    _budget: &PayloadBudget,
    unsupported_carrier: &mut bool,
) -> Result<(), TokenError> {
    let obj = body.as_object().ok_or(TokenError::RedactionFailed)?;
    if obj.get("type").and_then(Value::as_str) == Some("error")
        || obj.get("error").is_some()
        || !obj
            .get("id")
            .and_then(Value::as_str)
            .is_some_and(valid_external_id)
        || obj.get("role").and_then(Value::as_str) != Some("assistant")
    {
        return Err(TokenError::RedactionFailed);
    }
    let reason = obj.get("stop_reason").and_then(Value::as_str);
    if !AnthropicCompletionReason::new(reason).permits_completion() {
        return Err(TokenError::RedactionFailed);
    }
    let blocks = obj
        .get("content")
        .and_then(Value::as_array)
        .ok_or(TokenError::RedactionFailed)?;
    if blocks
        .iter()
        .any(|block| !anthropic_block_carrier_supported(block))
    {
        *unsupported_carrier = true;
        return Err(TokenError::RedactionFailed);
    }
    let mut tool_count = 0usize;
    for (idx, block) in blocks.iter().enumerate() {
        match block.get("type").and_then(Value::as_str) {
            Some("text") => {
                if !block.get("text").is_some_and(Value::is_string) {
                    return Err(TokenError::RedactionFailed);
                }
            }
            Some("thinking") => {
                if !block.get("thinking").is_some_and(Value::is_string) {
                    return Err(TokenError::RedactionFailed);
                }
            }
            Some("redacted_thinking") => {
                if !block
                    .get("data")
                    .and_then(Value::as_str)
                    .is_some_and(|s| !s.is_empty())
                {
                    return Err(TokenError::RedactionFailed);
                }
            }
            Some("tool_use") => {
                tool_count += 1;
                if tool_count > 128 {
                    return Err(TokenError::RedactionFailed);
                }
                let id = block
                    .get("id")
                    .and_then(Value::as_str)
                    .filter(|id| valid_external_id(id))
                    .ok_or(TokenError::RedactionFailed)?;
                if blocks[..idx].iter().any(|prior| {
                    prior.get("type").and_then(Value::as_str) == Some("tool_use")
                        && prior.get("id").and_then(Value::as_str) == Some(id)
                }) {
                    return Err(TokenError::RedactionFailed);
                }
                let name = block
                    .get("name")
                    .and_then(Value::as_str)
                    .filter(|name| valid_external_id(name))
                    .ok_or(TokenError::RedactionFailed)?;
                let input = block
                    .get("input")
                    .and_then(Value::as_object)
                    .ok_or(TokenError::RedactionFailed)?;
                if name == "Read" && input.get("pages").and_then(Value::as_str) == Some("") {
                    return Err(TokenError::RedactionFailed);
                }
            }
            _ => return Err(TokenError::RedactionFailed),
        }
    }
    if (reason == Some("tool_use")) != (tool_count != 0) {
        return Err(TokenError::RedactionFailed);
    }
    Ok(())
}

fn validate_gemini_json_response(
    body: &Value,
    unsupported_carrier: &mut bool,
) -> Result<(), TokenError> {
    let obj = body.as_object().ok_or(TokenError::RedactionFailed)?;
    if obj.get("error").is_some_and(|error| !error.is_null())
        || body
            .pointer("/promptFeedback/blockReason")
            .is_some_and(|reason| !reason.is_null())
    {
        return Err(TokenError::RedactionFailed);
    }
    let candidates = obj
        .get("candidates")
        .and_then(Value::as_array)
        .filter(|candidates| !candidates.is_empty() && candidates.len() <= 128)
        .ok_or(TokenError::RedactionFailed)?;
    for (candidate_index, candidate) in candidates.iter().enumerate() {
        let candidate = candidate.as_object().ok_or(TokenError::RedactionFailed)?;
        let reason = candidate.get("finishReason").and_then(Value::as_str);
        if !GeminiCompletionReason::new(reason).permits_completion() {
            return Err(TokenError::RedactionFailed);
        }
        if let Some(index) = candidate.get("index") {
            let index = index.as_u64().ok_or(TokenError::RedactionFailed)?;
            if candidates[..candidate_index]
                .iter()
                .any(|prior| prior.get("index").and_then(Value::as_u64) == Some(index))
            {
                return Err(TokenError::RedactionFailed);
            }
        }
        let content = candidate
            .get("content")
            .ok_or(TokenError::RedactionFailed)?;
        validate_gemini_content(content, unsupported_carrier)?;
    }
    Ok(())
}

/// Structural validation for one exact streamed or buffered native Content.
/// This grants no cross-frame identity, completion or publication authority.
pub(super) fn validate_gemini_content(
    content: &Value,
    unsupported_carrier: &mut bool,
) -> Result<(), TokenError> {
    let content = content.as_object().ok_or(TokenError::RedactionFailed)?;
    if content.get("role").and_then(Value::as_str) != Some("model") {
        return Err(TokenError::RedactionFailed);
    }
    let parts = content
        .get("parts")
        .and_then(Value::as_array)
        .filter(|parts| parts.len() <= 128)
        .ok_or(TokenError::RedactionFailed)?;
    for part in parts {
        let part = part.as_object().ok_or(TokenError::RedactionFailed)?;
        if part.keys().any(|key| {
            !matches!(
                key.as_str(),
                "text" | "functionCall" | "thought" | "thoughtSignature"
            )
        }) {
            *unsupported_carrier = true;
            return Err(TokenError::RedactionFailed);
        }
        if part
            .get("thought")
            .is_some_and(|thought| !thought.is_boolean())
            || part.get("thoughtSignature").is_some_and(|signature| {
                !signature
                    .as_str()
                    .is_some_and(|signature| !signature.is_empty())
            })
        {
            return Err(TokenError::RedactionFailed);
        }
        let payloads =
            usize::from(part.contains_key("text")) + usize::from(part.contains_key("functionCall"));
        if payloads > 1 || (payloads == 0 && !part.contains_key("thoughtSignature")) {
            return Err(TokenError::RedactionFailed);
        }
        if let Some(text) = part.get("text") {
            if !text.is_string() {
                return Err(TokenError::RedactionFailed);
            }
        }
        if let Some(call) = part.get("functionCall") {
            let call = call.as_object().ok_or(TokenError::RedactionFailed)?;
            if call
                .keys()
                .any(|key| !matches!(key.as_str(), "id" | "name" | "args"))
                || !call
                    .get("name")
                    .and_then(Value::as_str)
                    .is_some_and(valid_external_id)
                || call
                    .get("id")
                    .is_some_and(|id| !id.as_str().is_some_and(valid_external_id))
                || !call.get("args").is_some_and(Value::is_object)
            {
                return Err(TokenError::RedactionFailed);
            }
        }
    }
    Ok(())
}

fn validate_compact_json_response(body: &Value) -> Result<(), TokenError> {
    let obj = body.as_object().ok_or(TokenError::RedactionFailed)?;
    if obj.get("object").and_then(Value::as_str) != Some("response.compaction")
        || !obj
            .get("id")
            .and_then(Value::as_str)
            .is_some_and(valid_external_id)
        || obj.get("created_at").and_then(Value::as_u64).is_none()
        || obj.get("error").is_some()
    {
        return Err(TokenError::RedactionFailed);
    }
    if let Some(status) = obj.get("status").and_then(Value::as_str) {
        if status != "completed" {
            return Err(TokenError::RedactionFailed);
        }
    }
    let output = obj
        .get("output")
        .and_then(Value::as_array)
        .filter(|out| !out.is_empty())
        .ok_or(TokenError::RedactionFailed)?;
    let mut compaction_count = 0usize;
    for item in output {
        let item_obj = item.as_object().ok_or(TokenError::RedactionFailed)?;
        if !item_obj
            .get("id")
            .and_then(Value::as_str)
            .is_some_and(valid_external_id)
        {
            return Err(TokenError::RedactionFailed);
        }
        match item_obj.get("type").and_then(Value::as_str) {
            Some("compaction") => {
                if !item_obj
                    .get("encrypted_content")
                    .and_then(Value::as_str)
                    .is_some_and(|s| !s.is_empty() && s.len() <= FRAME_LIMIT)
                {
                    return Err(TokenError::RedactionFailed);
                }
                compaction_count += 1;
            }
            Some("message") => {
                if item_obj.get("role").and_then(Value::as_str) != Some("assistant")
                    || item_obj
                        .get("status")
                        .and_then(Value::as_str)
                        .is_some_and(|s| s != "completed")
                {
                    return Err(TokenError::RedactionFailed);
                }
                let content = item_obj
                    .get("content")
                    .and_then(Value::as_array)
                    .filter(|c| !c.is_empty())
                    .ok_or(TokenError::RedactionFailed)?;
                for part in content {
                    match part.get("type").and_then(Value::as_str) {
                        Some("output_text") => {
                            if !part.get("text").is_some_and(Value::is_string) {
                                return Err(TokenError::RedactionFailed);
                            }
                        }
                        Some("refusal") => {
                            if !part.get("refusal").is_some_and(Value::is_string) {
                                return Err(TokenError::RedactionFailed);
                            }
                        }
                        _ => return Err(TokenError::RedactionFailed),
                    }
                }
            }
            Some("reasoning") => {
                if let Some(summary) = item_obj.get("summary") {
                    let summary = summary.as_array().ok_or(TokenError::RedactionFailed)?;
                    for part in summary {
                        if part.get("type").and_then(Value::as_str) != Some("summary_text")
                            || !part.get("text").is_some_and(Value::is_string)
                        {
                            return Err(TokenError::RedactionFailed);
                        }
                    }
                }
                if let Some(enc) = item_obj.get("encrypted_content") {
                    if !enc.is_null() && !enc.as_str().is_some_and(|s| !s.is_empty()) {
                        return Err(TokenError::RedactionFailed);
                    }
                }
            }
            _ => return Err(TokenError::RedactionFailed),
        }
    }
    if compaction_count == 0 {
        return Err(TokenError::RedactionFailed);
    }
    Ok(())
}

pub(super) fn permits_chat_argument_normalization(
    original: Option<&Value>,
    normalized: &str,
    budget: &PayloadBudget,
) -> Result<bool, TokenError> {
    let Some(raw) = original.and_then(Value::as_str) else {
        return Ok(false);
    };
    if raw.trim().is_empty()
        || normalized.trim().is_empty()
        || raw.len() > ARGUMENT_LIMIT
        || normalized.len() > ARGUMENT_LIMIT
    {
        return Ok(false);
    }
    let Ok(orig_json) = PayloadJson::try_from_str(raw, budget.clone(), budget.limit()) else {
        return Ok(false);
    };
    let Ok(norm_json) = PayloadJson::try_from_str(normalized, budget.clone(), budget.limit())
    else {
        return Ok(false);
    };
    if !orig_json.as_value().is_object() || !norm_json.as_value().is_object() {
        return Ok(false);
    }
    Ok(same_canonical_object_semantics(
        orig_json.as_value(),
        norm_json.as_value(),
    ))
}

fn same_canonical_object_semantics(left: &Value, right: &Value) -> bool {
    match (left, right) {
        (Value::Array(l), Value::Array(r)) => {
            l.len() == r.len()
                && l.iter()
                    .zip(r)
                    .all(|(lv, rv)| same_canonical_object_semantics(lv, rv))
        }
        (Value::Object(l), Value::Object(r)) => {
            if l.len() != r.len() {
                return false;
            }
            let mut prev: Option<&str> = None;
            for (rk, rv) in r.iter() {
                if prev.is_some_and(|p| p > rk.as_str()) {
                    return false;
                }
                prev = Some(rk.as_str());
                let Some(lv) = l.get(rk) else {
                    return false;
                };
                if !same_canonical_object_semantics(lv, rv) {
                    return false;
                }
            }
            true
        }
        (Value::Number(l), Value::Number(r)) => same_exact_number(l, r),
        _ => left == right,
    }
}

fn same_exact_number(left: &Number, right: &Number) -> bool {
    if left.is_i64() || left.is_u64() || right.is_i64() || right.is_u64() {
        return (left.is_i64() && right.is_i64() && left.as_i64() == right.as_i64())
            || (left.is_u64() && right.is_u64() && left.as_u64() == right.as_u64());
    }
    match (left.as_f64(), right.as_f64()) {
        (Some(lf), Some(rf)) => lf.to_bits() == rf.to_bits(),
        _ => false,
    }
}

pub(super) fn permits_xai_argument_normalization(
    original: &str,
    normalized: &str,
    xai_applied: bool,
    budget: &PayloadBudget,
) -> Result<bool, TokenError> {
    if original.trim().is_empty()
        || normalized.trim().is_empty()
        || original.len() > ARGUMENT_LIMIT
        || normalized.len() > ARGUMENT_LIMIT
    {
        return Ok(false);
    }
    let Ok(orig_json) = PayloadJson::try_from_str(original, budget.clone(), budget.limit()) else {
        return Ok(false);
    };
    let Ok(norm_json) = PayloadJson::try_from_str(normalized, budget.clone(), budget.limit())
    else {
        return Ok(false);
    };
    if !orig_json.as_value().is_object() || !norm_json.as_value().is_object() {
        return Ok(false);
    }
    if !xai_applied {
        return Ok(super::request_carriers::same_snapshot(
            orig_json.as_value(),
            norm_json.as_value(),
        ));
    }
    Ok(same_xai_normalized_value(
        orig_json.as_value(),
        norm_json.as_value(),
    ))
}

fn same_xai_normalized_value(original: &Value, normalized: &Value) -> bool {
    match (original, normalized) {
        (Value::Array(l), Value::Array(r)) => {
            l.len() == r.len()
                && l.iter()
                    .zip(r)
                    .all(|(lv, rv)| same_xai_normalized_value(lv, rv))
        }
        (Value::Object(l), Value::Object(r)) => {
            l.len() == r.len()
                && l.iter()
                    .zip(r)
                    .all(|((lk, lv), (rk, rv))| lk == rk && same_xai_normalized_value(lv, rv))
        }
        (Value::Number(orig_num), Value::Number(norm_num)) => {
            if let Some(expected_int) = xai_whole_float_to_int(orig_num) {
                same_exact_number(&expected_int, norm_num)
            } else {
                same_exact_number(orig_num, norm_num)
            }
        }
        _ => original == normalized,
    }
}

fn xai_whole_float_to_int(number: &Number) -> Option<Number> {
    if number.is_i64() || number.is_u64() {
        return None;
    }
    let float = number.as_f64()?;
    if !float.is_finite() || float.fract() != 0.0 || float.to_bits() == (-0.0f64).to_bits() {
        return None;
    }
    if float >= 0.0 {
        if float >= u64::MAX as f64 {
            return None;
        }
        let int = float as u64;
        if int as f64 != float {
            return None;
        }
        Some(Number::from(int))
    } else {
        if float < i64::MIN as f64 {
            return None;
        }
        let int = float as i64;
        if int as f64 != float {
            return None;
        }
        Some(Number::from(int))
    }
}
