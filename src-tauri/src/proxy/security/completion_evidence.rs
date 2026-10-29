//! Original-protocol observations. These facts are not restoration authority.
//!
//! Borrowed payloads are inspected before converter normalization and never retained
//! here. No observation, terminal or callback can issue a completed-context lease.
use serde_json::Value;

pub(crate) enum CompletionObservation<'a> {
    RawFrame(&'a str),
    RawJson(&'a str),
    ChatChunk(&'a Value),
    ChatDone,
    AnthropicEvent {
        event_name: Option<&'a str>,
        data: &'a Value,
    },
    AnthropicJson(&'a Value),
    NativeEvent {
        event_name: &'a str,
        data: &'a Value,
    },
    BufferedBodyEnd {
        unfinished_frame: bool,
    },
    InvalidJson,
    InvalidUtf8,
    IoFailure,
    EntityEnd {
        reached_eof: bool,
        unfinished_frame: bool,
        unfinished_utf8: bool,
    },
}

/// Borrowed terminal classification; neither a reason nor its legacy projection
/// proves that an HTTP entity or any tool call completed.
pub(crate) struct ChatCompletionReason<'a>(Option<&'a str>);

impl<'a> ChatCompletionReason<'a> {
    pub(crate) fn new(reason: Option<&'a str>) -> Self {
        Self(reason)
    }

    pub(crate) fn permits_completion(&self) -> bool {
        matches!(self.0, Some("stop" | "tool_calls" | "function_call"))
    }

    pub(crate) fn legacy_status(&self) -> &'static str {
        if self.0 == Some("length") {
            "incomplete"
        } else {
            "completed"
        }
    }
}

/// Transport facts only. Protocol, snapshot and tool validation remain separate.
#[derive(Default)]
pub(crate) struct CompletionTransport {
    aborted: bool,
    reached_end: bool,
    invalid: bool,
}

impl CompletionTransport {
    pub(crate) fn abort(&mut self) {
        self.aborted = true;
    }

    pub(crate) fn legacy_failed(&self) -> bool {
        self.aborted
    }

    pub(crate) fn observe(&mut self, fact: &CompletionObservation<'_>) {
        // Completion is a final transport fact, not a resettable success flag.
        if self.reached_end {
            self.invalid = true;
        }
        match fact {
            CompletionObservation::IoFailure => {
                self.abort();
                self.invalid = true;
            }
            CompletionObservation::InvalidJson | CompletionObservation::InvalidUtf8 => {
                self.invalid = true;
            }
            CompletionObservation::EntityEnd {
                reached_eof,
                unfinished_frame,
                unfinished_utf8,
            } => {
                self.invalid |= !*reached_eof || *unfinished_frame || *unfinished_utf8;
                self.reached_end = true;
            }
            _ => {}
        }
    }

    /// The caller has fully read an original JSON HTTP entity, not an SSE stream.
    pub(crate) fn observe_fully_read_json(&mut self) {
        if self.reached_end {
            self.invalid = true;
        }
        self.reached_end = true;
    }

    pub(crate) fn permits_completion(&self) -> bool {
        self.reached_end && !self.aborted && !self.invalid
    }
}

/// The legacy projection remains intentionally more permissive than restoration.
pub(crate) struct AnthropicCompletionReason<'a>(Option<&'a str>);

impl<'a> AnthropicCompletionReason<'a> {
    pub(crate) fn new(reason: Option<&'a str>) -> Self {
        Self(reason)
    }

    pub(crate) fn legacy_status(&self) -> (&'static str, Option<&'static str>) {
        match self.0 {
            Some("max_tokens" | "model_context_window_exceeded") => {
                ("incomplete", Some("max_output_tokens"))
            }
            Some("refusal") => ("incomplete", Some("content_filter")),
            _ => ("completed", None),
        }
    }

    pub(crate) fn permits_completion(&self) -> bool {
        matches!(self.0, Some("end_turn" | "tool_use" | "stop_sequence"))
    }
}

/// Classifies a borrowed original Responses event, never a converted event.
pub(crate) struct NativeCompletionEvent<'a> {
    event_name: &'a str,
    data: &'a Value,
}

impl<'a> NativeCompletionEvent<'a> {
    pub(crate) fn new(event_name: &'a str, data: &'a Value) -> Self {
        Self { event_name, data }
    }

    pub(crate) fn legacy_is_completed(&self) -> bool {
        self.event_name == "response.completed"
    }

    pub(crate) fn permits_completion(&self) -> bool {
        self.event_name == "response.completed"
            && self.data.get("type").and_then(Value::as_str) == Some("response.completed")
            && self
                .data
                .pointer("/response/status")
                .and_then(Value::as_str)
                == Some("completed")
            && self.data.get("error").is_none_or(Value::is_null)
            && self
                .data
                .pointer("/response/error")
                .is_none_or(Value::is_null)
            && self
                .data
                .pointer("/response/incomplete_details")
                .is_none_or(Value::is_null)
    }
}

#[derive(Clone, Copy)]
enum SseProtocol {
    Chat,
    Anthropic,
}

/// Converter lifecycle extracted from the legacy finalization bit. This is not
/// a witness: full transport, raw provenance and snapshots are separate checks.
pub(crate) struct SseCompletion {
    protocol: SseProtocol,
    legacy_completed: bool,
    started: bool,
    terminal: bool,
    invalid: bool,
    unsupported_carrier: bool,
    allowed_reason: bool,
    chat_done: bool,
    selected_choice: Option<u64>,
    selected_response: Option<super::payload_text::PayloadText>,
    pending_event_name: Option<super::payload_text::PayloadText>,
    next_block: u64,
    open_block: Option<u64>,
    block_kind: u8,
    pending_raw: Option<super::payload_json::PayloadJson>,
    pending_done: bool,
    chat_tools: Option<super::completion_tools::ChatTools>,
    anthropic_tools: Option<super::completion_tools::AnthropicTools>,
}

impl SseCompletion {
    fn new(protocol: SseProtocol) -> Self {
        Self {
            protocol,
            legacy_completed: false,
            started: false,
            terminal: false,
            invalid: false,
            unsupported_carrier: false,
            allowed_reason: false,
            chat_done: false,
            selected_choice: None,
            selected_response: None,
            pending_event_name: None,
            next_block: 0,
            open_block: None,
            block_kind: 0,
            pending_raw: None,
            pending_done: false,
            chat_tools: None,
            anthropic_tools: None,
        }
    }

    pub(crate) fn chat() -> Self {
        Self::new(SseProtocol::Chat)
    }

    pub(crate) fn anthropic() -> Self {
        Self::new(SseProtocol::Anthropic)
    }

    pub(super) fn has_unsupported_carrier(&self) -> bool {
        self.unsupported_carrier
    }

    pub(crate) fn legacy_completed(&self) -> bool {
        self.legacy_completed
    }

    pub(crate) fn finish_legacy(&mut self) {
        self.legacy_completed = true;
    }

    pub(super) fn observe(
        &mut self,
        fact: &CompletionObservation<'_>,
        budget: &super::payload_budget::PayloadBudget,
    ) {
        match (self.protocol, fact) {
            (_, CompletionObservation::RawFrame(raw)) => {
                if self.raw_frame(raw, budget).is_err() {
                    self.invalid = true;
                }
            }
            (SseProtocol::Chat, CompletionObservation::ChatChunk(data)) => {
                self.consume_raw(data);
                self.chat_chunk(data, budget);
            }
            (SseProtocol::Chat, CompletionObservation::ChatDone) => {
                self.invalid |= !self.pending_done || self.chat_done || !self.terminal;
                self.pending_done = false;
                self.chat_done = true;
            }
            (
                SseProtocol::Anthropic,
                CompletionObservation::AnthropicEvent { event_name, data },
            ) => {
                self.invalid |= self
                    .pending_event_name
                    .take()
                    .as_ref()
                    .map(|name| name.as_str())
                    != *event_name;
                self.consume_raw(data);
                self.anthropic_event(*event_name, data, budget);
            }
            (
                _,
                CompletionObservation::InvalidJson
                | CompletionObservation::InvalidUtf8
                | CompletionObservation::IoFailure
                | CompletionObservation::RawJson(_)
                | CompletionObservation::AnthropicJson(_)
                | CompletionObservation::NativeEvent { .. }
                | CompletionObservation::BufferedBodyEnd { .. },
            )
            | (SseProtocol::Chat, CompletionObservation::AnthropicEvent { .. })
            | (
                SseProtocol::Anthropic,
                CompletionObservation::ChatChunk(_) | CompletionObservation::ChatDone,
            ) => self.invalid = true,
            (_, CompletionObservation::EntityEnd { .. }) => {}
        }
    }

    fn consume_raw(&mut self, data: &Value) {
        // Pair the exact ordered snapshot, including floating-point signed zero.
        self.invalid |= self
            .pending_raw
            .take()
            .is_none_or(|raw| !super::request_carriers::same_snapshot(raw.as_value(), data));
    }

    fn raw_frame(
        &mut self,
        raw: &str,
        budget: &super::payload_budget::PayloadBudget,
    ) -> Result<(), super::tokenization::TokenError> {
        use super::{
            payload_buffer::PayloadBuffer, payload_json::PayloadJson, tokenization::TokenError,
        };
        if raw.len() > 1024 * 1024 || self.pending_raw.is_some() || self.pending_done {
            return Err(TokenError::RedactionFailed);
        }
        let mut event_name = None;
        let mut data_bytes = 0usize;
        let mut data_count = 0usize;
        for line in raw.lines() {
            if line.is_empty() || line.starts_with(':') {
                continue;
            }
            let (name, value) = line.split_once(':').unwrap_or((line, ""));
            let value = value.strip_prefix(' ').unwrap_or(value);
            match name {
                "event" if event_name.is_none() => event_name = Some(value),
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
        if matches!(self.protocol, SseProtocol::Chat)
            && event_name.is_some_and(|name| !matches!(name, "" | "message"))
        {
            return Err(TokenError::RedactionFailed);
        }
        let mut data = PayloadBuffer::with_capacity(data_bytes, budget.clone(), 1024 * 1024)?;
        let mut first = true;
        for line in raw.lines() {
            if let Some(value) = line
                .strip_prefix("data:")
                .or_else(|| (line == "data").then_some(""))
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
            if !matches!(self.protocol, SseProtocol::Chat) {
                return Err(TokenError::RedactionFailed);
            }
            self.pending_done = true;
        } else {
            self.pending_raw = Some(PayloadJson::try_from_str(
                encoded,
                budget.clone(),
                budget.limit(),
            )?);
            if matches!(self.protocol, SseProtocol::Anthropic) {
                self.pending_event_name = event_name
                    .map(|name| {
                        super::payload_text::PayloadText::try_from_str(
                            name,
                            budget.clone(),
                            1024 * 1024,
                        )
                    })
                    .transpose()?;
            }
        }
        Ok(())
    }

    fn chat_chunk(&mut self, data: &Value, budget: &super::payload_budget::PayloadBudget) {
        let Some(id) = data
            .get("id")
            .and_then(Value::as_str)
            .filter(|id| valid_external_id(id))
        else {
            self.invalid = true;
            return;
        };
        if let Some(selected) = &self.selected_response {
            self.invalid |= selected.as_str() != id;
        } else {
            match super::payload_text::PayloadText::try_from_str(id, budget.clone(), 256) {
                Ok(selected) => self.selected_response = Some(selected),
                Err(_) => self.invalid = true,
            }
        }
        if self.chat_done || data.get("error").is_some() {
            self.invalid = true;
            return;
        }
        let Some(choices) = data.get("choices").and_then(Value::as_array) else {
            self.invalid = true;
            return;
        };
        if choices.is_empty() && data.get("usage").is_some() {
            return;
        }
        if choices.len() != 1 || self.terminal {
            self.invalid = true;
            return;
        }
        let choice = &choices[0];
        let Some(index) = choice.get("index").and_then(Value::as_u64) else {
            self.invalid = true;
            return;
        };
        if self
            .selected_choice
            .is_some_and(|selected| selected != index)
        {
            self.invalid = true;
        }
        let Some(delta) = choice.get("delta").and_then(Value::as_object) else {
            self.invalid = true;
            return;
        };
        self.unsupported_carrier |= !super::completion_json::chat_carriers_supported(delta, true);
        self.invalid |= self.unsupported_carrier
            || delta
                .get("role")
                .is_some_and(|v| v.as_str() != Some("assistant"))
            || delta.get("tool_calls").is_some_and(|v| !v.is_array());
        self.invalid |= delta.contains_key("function_call");
        if let Some(tools) = delta.get("tool_calls") {
            if self.chat_tools.is_none() {
                match super::completion_tools::ChatTools::new(budget.clone()) {
                    Ok(tools) => self.chat_tools = Some(tools),
                    Err(_) => self.invalid = true,
                }
            }
            if let Some(state) = &mut self.chat_tools {
                self.invalid |= state.observe(tools, budget).is_err();
            }
        }
        self.selected_choice = Some(index);
        self.started = true;
        if let Some(reason) = choice
            .get("finish_reason")
            .filter(|reason| !reason.is_null())
        {
            self.allowed_reason = ChatCompletionReason::new(reason.as_str()).permits_completion();
            let tool_reason = matches!(reason.as_str(), Some("tool_calls" | "function_call"));
            self.invalid |= !self.allowed_reason
                || match &self.chat_tools {
                    Some(tools) => tools.finish(tool_reason, budget).is_err(),
                    None => tool_reason,
                };
            self.terminal = true;
        }
    }

    fn anthropic_event(
        &mut self,
        event_name: Option<&str>,
        data: &Value,
        budget: &super::payload_budget::PayloadBudget,
    ) {
        let Some(kind) = data.get("type").and_then(Value::as_str) else {
            self.invalid = true;
            return;
        };
        // Inspect original carriers before lifecycle checks or converter normalization.
        let supported = match kind {
            "message_start" => data
                .pointer("/message/content")
                .and_then(Value::as_array)
                .is_none_or(|blocks| {
                    blocks
                        .iter()
                        .all(super::completion_json::anthropic_block_carrier_supported)
                }),
            "content_block_start" => data
                .get("content_block")
                .is_none_or(super::completion_json::anthropic_block_carrier_supported),
            "content_block_delta" => data
                .get("delta")
                .is_none_or(super::completion_json::anthropic_delta_carrier_supported),
            "message_delta" => data.get("delta").is_none_or(|delta| {
                super::completion_json::anthropic_carrier_keys_supported(
                    delta,
                    &["stop_reason", "stop_sequence"],
                )
            }),
            _ => true,
        };
        if !supported {
            self.unsupported_carrier = true;
            self.invalid = true;
        }
        if event_name.is_some_and(|name| name != kind) || kind == "error" {
            self.invalid = true;
        }
        if kind == "ping" {
            return;
        }
        if self.terminal {
            self.invalid = true;
            return;
        }
        if kind == "message_start" {
            let valid_message = data.get("message").is_some_and(|message| {
                message.is_object()
                    && message
                        .get("id")
                        .and_then(Value::as_str)
                        .is_some_and(valid_external_id)
                    && message.get("role").and_then(Value::as_str) == Some("assistant")
                    && message
                        .get("content")
                        .and_then(Value::as_array)
                        .is_some_and(Vec::is_empty)
            });
            self.invalid |= self.started || !valid_message;
            self.started = true;
            return;
        }
        self.invalid |= !self.started;
        match kind {
            "content_block_start" => {
                let index = data.get("index").and_then(Value::as_u64);
                self.invalid |= self.allowed_reason
                    || self.open_block.is_some()
                    || index != Some(self.next_block)
                    || self.next_block >= 4096;
                self.block_kind = match data.pointer("/content_block/type").and_then(Value::as_str)
                {
                    Some("text")
                        if data
                            .pointer("/content_block/text")
                            .is_some_and(Value::is_string) =>
                    {
                        1
                    }
                    Some("thinking")
                        if data
                            .pointer("/content_block/thinking")
                            .is_some_and(Value::is_string) =>
                    {
                        2
                    }
                    Some("tool_use")
                        if data
                            .pointer("/content_block/input")
                            .is_some_and(Value::is_object) =>
                    {
                        3
                    }
                    Some("redacted_thinking")
                        if data
                            .pointer("/content_block/data")
                            .is_some_and(Value::is_string) =>
                    {
                        4
                    }
                    _ => 0,
                };
                self.invalid |= self.block_kind == 0;
                if self.block_kind == 3 {
                    if self.anthropic_tools.is_none() {
                        match super::completion_tools::AnthropicTools::new(budget.clone()) {
                            Ok(tools) => self.anthropic_tools = Some(tools),
                            Err(_) => self.invalid = true,
                        }
                    }
                    if let Some(tools) = &mut self.anthropic_tools {
                        self.invalid |= data
                            .get("content_block")
                            .is_none_or(|block| tools.start(block, budget).is_err());
                    }
                }
                self.open_block = index;
            }
            "content_block_delta" => {
                self.invalid |= self.allowed_reason
                    || self.open_block.is_none()
                    || data.get("index").and_then(Value::as_u64) != self.open_block;
                let delta_kind = data.pointer("/delta/type").and_then(Value::as_str);
                self.invalid |= !match (self.block_kind, delta_kind) {
                    (1, Some("text_delta")) => {
                        data.pointer("/delta/text").is_some_and(Value::is_string)
                    }
                    (2, Some("thinking_delta")) => data
                        .pointer("/delta/thinking")
                        .is_some_and(Value::is_string),
                    (2, Some("signature_delta")) => data
                        .pointer("/delta/signature")
                        .is_some_and(Value::is_string),
                    (3, Some("input_json_delta")) => self
                        .anthropic_tools
                        .as_mut()
                        .is_some_and(|tools| tools.delta(data).is_ok()),
                    _ => false,
                };
            }
            "content_block_stop" => {
                if self.block_kind == 3 {
                    self.invalid |= self
                        .anthropic_tools
                        .as_mut()
                        .is_none_or(|tools| tools.close(budget).is_err());
                }
                self.invalid |= self.open_block.is_none()
                    || data.get("index").and_then(Value::as_u64) != self.open_block;
                self.open_block = None;
                self.next_block = self.next_block.saturating_add(1);
            }
            "message_delta" => {
                self.invalid |= self.open_block.is_some();
                if let Some(reason) = data
                    .pointer("/delta/stop_reason")
                    .filter(|reason| !reason.is_null())
                {
                    self.invalid |= self.allowed_reason;
                    self.allowed_reason =
                        AnthropicCompletionReason::new(reason.as_str()).permits_completion();
                    let tool_reason = reason.as_str() == Some("tool_use");
                    self.invalid |= !self.allowed_reason
                        || match &self.anthropic_tools {
                            Some(tools) => !tools.finish(tool_reason),
                            None => tool_reason,
                        };
                }
            }
            "message_stop" => {
                self.invalid |= !self.allowed_reason || self.open_block.is_some();
                self.terminal = true;
            }
            _ => self.invalid = true,
        }
    }

    pub(crate) fn permits_completion(&self) -> bool {
        self.started
            && self.terminal
            && self.allowed_reason
            && !self.invalid
            && self.pending_raw.is_none()
            && !self.pending_done
    }
}

fn valid_external_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 256 && !id.chars().any(|c| c.is_whitespace() || c.is_control())
}
