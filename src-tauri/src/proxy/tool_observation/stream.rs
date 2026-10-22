use super::parsers::{
    bounded_string, source_index, termination, PROTOCOL_ANTHROPIC, PROTOCOL_GEMINI,
    PROTOCOL_OPENAI_CHAT, PROTOCOL_OPENAI_RESPONSES,
};
use super::types::{
    bounded_input_size, Correlation, ExecutionScope, IdentityQuality, InputEncoding, InputFidelity,
    InputStatus, ObservationContext, ObservationQuality, ObservationSource, ObservationStage,
    QualityOrigin, SourceTermination, ToolIdentity, ToolIdentityQuality, ToolInput,
    ToolObservation, MAX_ID_BYTES, MAX_INPUT_BYTES, MAX_NAME_BYTES,
};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

pub const MAX_CALL_INPUT_BYTES: usize = MAX_INPUT_BYTES;
pub const MAX_RESPONSE_INPUT_BYTES: usize = 2 * 1024 * 1024;
pub const MAX_PENDING_CALLS: usize = 32;
type CallKey = (u64, u64);

#[derive(Clone)]
pub struct StreamContext {
    pub scope_id: String,
    pub client: Option<String>,
    pub execution_scope: ExecutionScope,
}
impl std::fmt::Debug for StreamContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("StreamContext { <redacted> }")
    }
}

#[derive(Clone, Default)]
struct PendingCall {
    call_id: Option<String>,
    item_id: Option<String>,
    name: Option<String>,
    namespace: Option<String>,
    args: String,
    free_text: bool,
    has_input: bool,
    has_delta: bool,
    conflicted: bool,
    identity_conflicted: bool,
    overflowed: bool,
}
struct CompletedCall {
    call: PendingCall,
    fingerprint: [u8; 32],
}

#[derive(Debug, Default, Clone, Copy)]
pub struct StreamDiagnostics {
    pub invalid_event: u64,
    pub missing_delta: u64,
    pub budget_exceeded: u64,
    pub conflicting_snapshot: u64,
    /// Fixed code only; never store an upstream field or payload.
    pub first_reason: Option<&'static str>,
    pub first_line: Option<u32>,
}

pub struct StreamAssembler {
    protocol: &'static str,
    ctx: StreamContext,
    pending: BTreeMap<CallKey, PendingCall>,
    completed: BTreeMap<CallKey, CompletedCall>,
    // Malformed Chat carriers invalidate only their candidate, never discard other inputs.
    chat_faults: BTreeSet<u64>,
    emitted: usize,
    degraded: bool,
    diagnostics: StreamDiagnostics,
}
impl std::fmt::Debug for StreamAssembler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StreamAssembler")
            .field("pending_count", &self.pending.len())
            .field("degraded", &self.degraded)
            .finish_non_exhaustive()
    }
}

impl StreamAssembler {
    pub fn new(protocol: &'static str, ctx: StreamContext) -> Self {
        Self {
            protocol,
            ctx,
            pending: BTreeMap::new(),
            completed: BTreeMap::new(),
            chat_faults: BTreeSet::new(),
            emitted: 0,
            degraded: false,
            diagnostics: StreamDiagnostics::default(),
        }
    }
    pub fn is_degraded(&self) -> bool {
        self.degraded
    }
    pub fn diagnostics(&self) -> StreamDiagnostics {
        self.diagnostics
    }
    pub fn buffered_bytes(&self) -> usize {
        self.pending.values().map(|call| call.args.len()).sum()
    }

    #[track_caller]
    pub(crate) fn mark_degraded_with_reason(&mut self, reason: &'static str) {
        if self.diagnostics.first_reason.is_none() {
            self.diagnostics.first_reason = Some(reason);
            self.diagnostics.first_line = Some(std::panic::Location::caller().line());
        }
        self.degraded = true;
        for call in self.pending.values_mut() {
            call.args = String::new();
            call.has_input = false;
            call.overflowed = true;
        }
    }
    #[track_caller]
    fn invalid_event(&mut self) {
        self.diagnostics.invalid_event = self.diagnostics.invalid_event.saturating_add(1);
        self.mark_degraded_with_reason("invalid_event");
    }
    #[track_caller]
    fn missing_delta(&mut self) {
        self.missing_delta_with_reason("missing_delta");
    }
    #[track_caller]
    fn missing_delta_with_reason(&mut self, reason: &'static str) {
        self.diagnostics.missing_delta = self.diagnostics.missing_delta.saturating_add(1);
        self.mark_degraded_with_reason(reason);
    }
    #[track_caller]
    fn budget_exceeded(&mut self) {
        self.diagnostics.budget_exceeded = self.diagnostics.budget_exceeded.saturating_add(1);
        self.mark_degraded_with_reason("budget_exceeded");
    }
    /// Carrier corruption is candidate-local. Preserve observed facts for static hits,
    /// but never certify a candidate with an unparsed carrier as complete.
    #[track_caller]
    fn chat_carrier_fault(&mut self, index: u64, reason: &'static str) {
        self.diagnostics.invalid_event = self.diagnostics.invalid_event.saturating_add(1);
        if self.diagnostics.first_reason.is_none() {
            self.diagnostics.first_reason = Some(reason);
            self.diagnostics.first_line = Some(std::panic::Location::caller().line());
        }
        if !self.chat_faults.contains(&index) && self.chat_faults.len() >= MAX_PENDING_CALLS {
            self.budget_exceeded();
            return;
        }
        self.chat_faults.insert(index);
    }
    fn ensure_slot(&mut self, key: CallKey) -> bool {
        if self.pending.contains_key(&key) {
            return true;
        }

        if self.pending.len() + self.completed.len() >= MAX_PENDING_CALLS {
            self.budget_exceeded();
            return false;
        }
        self.pending.insert(
            key,
            PendingCall {
                overflowed: self.degraded,
                ..Default::default()
            },
        );
        true
    }
    fn update_identity(&mut self, key: CallKey, value: &Value, responses: bool) {
        let fields = [
            ("id", MAX_ID_BYTES),
            ("call_id", MAX_ID_BYTES),
            ("name", MAX_NAME_BYTES),
            ("namespace", MAX_NAME_BYTES),
        ];
        for (field, limit) in fields {
            if value.get(field).is_some_and(|v| {
                !v.is_null()
                    && (!v.is_string()
                        || v.as_str()
                            .is_some_and(|s| s.trim().is_empty() || s.len() > limit))
            }) {
                self.invalid_event();
                if let Some(call) = self.pending.get_mut(&key) {
                    call.identity_conflicted = true;
                }
            }
        }
        if let Some(call) = self.pending.get_mut(&key) {
            if let Some(id) = bounded_string(value.get("id"), MAX_ID_BYTES) {
                if responses {
                    call.item_id = Some(id);
                } else {
                    call.call_id = Some(id);
                }
            }
            if responses {
                if let Some(id) = bounded_string(value.get("call_id"), MAX_ID_BYTES) {
                    call.call_id = Some(id);
                }
            }
            if let Some(name) = bounded_string(value.get("name"), MAX_NAME_BYTES) {
                call.name = Some(name);
            }
            if let Some(ns) = bounded_string(value.get("namespace"), MAX_NAME_BYTES) {
                call.namespace = Some(ns);
            }
        }
    }
    fn set_args(&mut self, key: CallKey, raw: &str, append: bool, check_snapshot: bool) {
        if self.degraded {
            return;
        }
        let Some(call) = self.pending.get(&key) else {
            self.missing_delta();
            return;
        };
        let new_len = if append {
            call.args.len().saturating_add(raw.len())
        } else {
            raw.len()
        };
        let total = self
            .buffered_bytes()
            .saturating_sub(call.args.len())
            .saturating_add(new_len);
        if new_len > MAX_CALL_INPUT_BYTES || total > MAX_RESPONSE_INPUT_BYTES {
            self.budget_exceeded();
            return;
        }
        let conflict = check_snapshot
            && call.has_input
            && !call.args.is_empty()
            && !raw.starts_with(&call.args)
            && fingerprint(&call.args, call.free_text) != fingerprint(raw, call.free_text);
        if conflict {
            self.diagnostics.conflicting_snapshot =
                self.diagnostics.conflicting_snapshot.saturating_add(1);
        }
        if let Some(call) = self.pending.get_mut(&key) {
            call.conflicted |= conflict;
            if append {
                let mut args = String::with_capacity(new_len);
                args.push_str(&call.args);
                args.push_str(raw);
                call.args = args;
            } else {
                call.args = raw.to_string();
            }
            call.has_input = true;
            call.has_delta |= append;
        }
    }
    fn set_value(&mut self, key: CallKey, value: &Value) {
        if self.degraded {
            return;
        }
        if bounded_input_size(value).is_err() {
            self.budget_exceeded();
            return;
        }
        self.set_args(key, &value.to_string(), false, false);
    }
    pub fn feed(&mut self, event: &Value) -> Vec<ToolObservation> {
        if !event.is_object() {
            self.invalid_event();
            return Vec::new();
        }
        if event.get("error").is_some_and(|v| !v.is_null()) {
            self.mark_degraded_with_reason("upstream_error");
        }
        match self.protocol {
            PROTOCOL_ANTHROPIC => self.feed_anthropic(event),
            PROTOCOL_OPENAI_CHAT => self.feed_chat(event),
            PROTOCOL_OPENAI_RESPONSES => self.feed_responses(event),
            PROTOCOL_GEMINI => self.feed_gemini(event),
            _ => {
                self.invalid_event();
                Vec::new()
            }
        }
    }
    pub fn finish(&mut self) -> Vec<ToolObservation> {
        let calls = std::mem::take(&mut self.pending);
        let out = calls
            .iter()
            .map(|(key, call)| self.build(*key, call, false))
            .collect();
        self.completed.clear();
        self.chat_faults.clear();
        out
    }
    fn build(&mut self, key: CallKey, call: &PendingCall, normal: bool) -> ToolObservation {
        let normal = normal && !self.chat_faults.contains(&key.0);
        let complete =
            normal && !self.degraded && !call.overflowed && !call.conflicted && call.has_input;
        let (encoding, value, mut status) = if !call.has_input || call.overflowed {
            (
                if call.free_text {
                    InputEncoding::Text
                } else {
                    InputEncoding::Json
                },
                None,
                InputStatus::Incomplete,
            )
        } else if call.free_text {
            (
                InputEncoding::Text,
                Some(Value::String(call.args.clone())),
                InputStatus::Complete,
            )
        } else {
            match serde_json::from_str::<Value>(&call.args) {
                Ok(value) if bounded_input_size(&value).is_ok() => {
                    (InputEncoding::Json, Some(value), InputStatus::Complete)
                }
                Ok(_) => (InputEncoding::Json, None, InputStatus::Incomplete),
                Err(_) => (
                    InputEncoding::Text,
                    Some(Value::String(call.args.clone())),
                    InputStatus::Invalid,
                ),
            }
        };
        if status == InputStatus::Complete && !complete {
            status = InputStatus::Incomplete;
        }
        let index = self.emitted;
        self.emitted = self.emitted.saturating_add(1);
        let tool_identity = if !call.identity_conflicted
            && call
                .name
                .as_deref()
                .is_some_and(|name| name != "unknown_tool")
        {
            ToolIdentityQuality::Source
        } else {
            ToolIdentityQuality::Unknown
        };
        ToolObservation {
            schema_version: 1,
            observation_id: format!("{}:{}:stream:{index}", self.protocol, self.ctx.scope_id),
            source: ObservationSource::ModelResponse,
            stage: ObservationStage::Proposed,
            execution_scope: self.ctx.execution_scope,
            protocol: self.protocol.into(),
            scope_id: self.ctx.scope_id.clone(),
            correlation: Correlation {
                call_id: call.call_id.clone(),
                item_id: call.item_id.clone(),
                candidate_index: if self.protocol == PROTOCOL_OPENAI_CHAT
                    || self.protocol == PROTOCOL_GEMINI
                {
                    u32::try_from(key.0).ok()
                } else {
                    None
                },
                tool_index: u32::try_from(key.1).ok(),
                identity_quality: Some(if call.call_id.is_some() {
                    IdentityQuality::SourceId
                } else {
                    IdentityQuality::Synthetic
                }),
                ..Default::default()
            },
            tool: ToolIdentity {
                client: self.ctx.client.clone(),
                name: call.name.clone().unwrap_or_else(|| "unknown_tool".into()),
                namespace: call.namespace.clone(),
                server_id: None,
                definition_ref: None,
            },
            input: ToolInput {
                encoding,
                value,
                status,
            },
            quality: ObservationQuality {
                origin: QualityOrigin::Native,
                source_termination: if normal && !self.degraded && !call.overflowed {
                    SourceTermination::Normal
                } else {
                    SourceTermination::Abnormal
                },
                tool_identity,
                input_fidelity: if self.degraded || call.overflowed || call.conflicted {
                    InputFidelity::RepairedOrDropped
                } else {
                    InputFidelity::Preserved
                },
            },
            context: ObservationContext::default(),
        }
    }
    fn end_call(&mut self, key: CallKey, normal: bool, remember: bool) -> Vec<ToolObservation> {
        let Some(mut call) = self.pending.remove(&key) else {
            return Vec::new();
        };
        let out = vec![self.build(key, &call, normal)];
        if remember {
            let fingerprint = fingerprint(&call.args, call.free_text);
            call.args = String::new();
            self.completed
                .insert(key, CompletedCall { call, fingerprint });
        }
        out
    }
    fn end_candidate(&mut self, index: u64, normal: bool) -> Vec<ToolObservation> {
        let keys: Vec<_> = self
            .pending
            .keys()
            .copied()
            .filter(|key| key.0 == index)
            .collect();
        keys.into_iter()
            .flat_map(|key| self.end_call(key, normal, false))
            .collect()
    }
    fn event_key(&mut self, event: &Value, field: &str) -> Option<CallKey> {
        match event.get(field).and_then(Value::as_u64) {
            Some(index) => Some((0, index)),
            None => {
                self.invalid_event();
                None
            }
        }
    }
    fn feed_anthropic(&mut self, event: &Value) -> Vec<ToolObservation> {
        match event.get("type").and_then(Value::as_str) {
            Some("content_block_start") => {
                let Some(key) = self.event_key(event, "index") else {
                    return Vec::new();
                };
                let Some(block) = event.get("content_block") else {
                    self.invalid_event();
                    return Vec::new();
                };
                if block.get("type").and_then(Value::as_str) != Some("tool_use") {
                    return Vec::new();
                }
                if !self.ensure_slot(key) {
                    return Vec::new();
                }
                self.update_identity(key, block, false);
                if let Some(input) = block.get("input").filter(|v| !v.is_null()) {
                    self.set_value(key, input);
                }
            }
            Some("content_block_delta") => {
                let Some(delta) = event.get("delta") else {
                    self.missing_delta();
                    return Vec::new();
                };
                if delta.get("type").and_then(Value::as_str) != Some("input_json_delta") {
                    return Vec::new();
                }
                let Some(key) = self.event_key(event, "index") else {
                    return Vec::new();
                };
                let Some(raw) = delta.get("partial_json").and_then(Value::as_str) else {
                    self.missing_delta();
                    return Vec::new();
                };
                if let Some(call) = self.pending.get_mut(&key) {
                    if !call.has_delta {
                        call.args = String::new();
                        call.has_input = false;
                    }
                }
                self.set_args(key, raw, true, false);
            }
            Some("content_block_stop") => {
                if let Some(key) = self.event_key(event, "index") {
                    return self.end_call(key, true, false);
                }
            }
            Some("error") => self.mark_degraded_with_reason("upstream_error"),
            Some("message_start" | "message_delta" | "message_stop" | "ping") => {}
            _ => self.invalid_event(),
        }
        Vec::new()
    }
    fn feed_chat(&mut self, event: &Value) -> Vec<ToolObservation> {
        let Some(choices) = event.get("choices").and_then(Value::as_array) else {
            self.invalid_event();
            return Vec::new();
        };
        let mut out = Vec::new();
        for (ci, choice) in choices.iter().enumerate() {
            let Some(index) = source_index(choice, ci) else {
                self.invalid_event();
                continue;
            };
            for (carrier, snapshot) in [("delta", false), ("message", true)] {
                let Some(message) = choice.get(carrier) else {
                    continue;
                };
                let calls = match message.get("tool_calls") {
                    None | Some(Value::Null) => Vec::new(),
                    Some(Value::Array(calls)) => calls.iter().collect::<Vec<_>>(),
                    Some(invalid) => {
                        let reason = match invalid {
                            Value::Object(_) => "chat_tool_calls_object",
                            Value::String(_) => "chat_tool_calls_string",
                            Value::Number(_) => "chat_tool_calls_number",
                            Value::Bool(_) => "chat_tool_calls_boolean",
                            _ => "chat_tool_calls_invalid",
                        };
                        self.chat_carrier_fault(index, reason);
                        Vec::new()
                    }
                };
                let legacy =
                    calls.is_empty() && message.get("function_call").is_some_and(|v| !v.is_null());
                let calls = if legacy {
                    message.get("function_call").into_iter().collect()
                } else {
                    calls
                };
                for (ti, tool) in calls.into_iter().enumerate() {
                    let Some(tool_index) = source_index(tool, ti) else {
                        self.invalid_event();
                        continue;
                    };
                    let key = (index, tool_index);
                    if !self.ensure_slot(key) {
                        continue;
                    }
                    if tool.get("type").is_some_and(|value| value != "function") {
                        if let Some(call) = self.pending.get_mut(&key) {
                            call.identity_conflicted = true;
                        }
                    }
                    self.update_identity(key, tool, false);
                    let function = if legacy {
                        tool
                    } else {
                        let Some(function) = tool.get("function") else {
                            self.invalid_event();
                            continue;
                        };
                        function
                    };
                    self.update_identity(key, function, false);
                    if let Some(args) = function.get("arguments") {
                        if let Some(raw) = args.as_str() {
                            self.set_args(key, raw, !snapshot, false);
                        } else {
                            self.invalid_event();
                        }
                    } else if function.get("name").is_none() {
                        self.missing_delta_with_reason(if function.is_null() {
                            "chat_function_null"
                        } else {
                            "chat_missing_arguments"
                        });
                    }
                }
            }
            if choice.get("error").is_some_and(|v| !v.is_null()) {
                self.mark_degraded_with_reason("upstream_error");
            }
            if choice.get("finish_reason").is_some_and(|v| !v.is_null()) {
                let normal = termination(
                    choice.get("finish_reason"),
                    &["tool_calls", "stop", "function_call"],
                ) == SourceTermination::Normal;
                out.extend(self.end_candidate(index, normal));
            }
        }
        out
    }
    fn responses_snapshot(
        &mut self,
        key: CallKey,
        item: &Value,
        normal: bool,
        terminal: bool,
    ) -> Vec<ToolObservation> {
        let kind = item.get("type").and_then(Value::as_str);
        let free_text = kind == Some("custom_tool_call");
        if kind != Some("function_call") && !free_text {
            return Vec::new();
        }
        let raw = item
            .get(if free_text { "input" } else { "arguments" })
            .and_then(Value::as_str);
        if let Some(done) = self.completed.get(&key) {
            let identity_changed = [
                ("name", done.call.name.as_deref()),
                ("namespace", done.call.namespace.as_deref()),
                ("call_id", done.call.call_id.as_deref()),
                ("id", done.call.item_id.as_deref()),
            ]
            .iter()
            .any(|(field, old)| {
                item.get(*field).is_some_and(|value| {
                    value.as_str() != *old
                        || (!value.is_null()
                            && bounded_string(Some(value), MAX_NAME_BYTES).is_none())
                })
            });
            let changed = identity_changed
                || raw.is_some_and(|raw| fingerprint(raw, free_text) != done.fingerprint)
                || item
                    .get("name")
                    .and_then(Value::as_str)
                    .is_some_and(|name| Some(name) != done.call.name.as_deref())
                || (!normal && terminal && !done.call.conflicted);
            if !changed {
                return Vec::new();
            }
            let mut call = done.call.clone();
            call.conflicted = true;
            call.identity_conflicted |= identity_changed;
            self.completed.remove(&key);
            self.pending.insert(key, call);
            self.diagnostics.conflicting_snapshot =
                self.diagnostics.conflicting_snapshot.saturating_add(1);
        } else if !self.ensure_slot(key) {
            return Vec::new();
        }
        self.update_identity(key, item, true);
        if let Some(call) = self.pending.get_mut(&key) {
            call.free_text = free_text;
        }
        if let Some(raw) = raw {
            self.set_args(key, raw, false, terminal);
        } else if terminal {
            self.missing_delta();
        }
        if terminal {
            self.end_call(key, normal, true)
        } else {
            Vec::new()
        }
    }
    fn feed_responses(&mut self, event: &Value) -> Vec<ToolObservation> {
        let Some(kind) = event.get("type").and_then(Value::as_str) else {
            self.invalid_event();
            return Vec::new();
        };
        match kind {
            "response.output_item.added" | "response.output_item.done" => {
                let Some(key) = self.event_key(event, "output_index") else {
                    return Vec::new();
                };
                let Some(item) = event.get("item") else {
                    self.invalid_event();
                    return Vec::new();
                };
                let terminal = kind.ends_with(".done");
                let normal = item
                    .get("status")
                    .is_none_or(|status| status == "completed");
                self.responses_snapshot(key, item, normal, terminal)
            }
            "response.function_call_arguments.delta" | "response.custom_tool_call_input.delta" => {
                let Some(key) = self.event_key(event, "output_index") else {
                    return Vec::new();
                };
                let Some(raw) = event.get("delta").and_then(Value::as_str) else {
                    self.missing_delta();
                    return Vec::new();
                };
                self.set_args(key, raw, true, false);
                Vec::new()
            }
            "response.function_call_arguments.done" | "response.custom_tool_call_input.done" => {
                let Some(key) = self.event_key(event, "output_index") else {
                    return Vec::new();
                };
                let free_text = kind == "response.custom_tool_call_input.done";
                let field = if free_text { "input" } else { "arguments" };
                let Some(raw) = event.get(field).and_then(Value::as_str) else {
                    self.missing_delta();
                    return Vec::new();
                };
                if let Some(done) = self.completed.get(&key) {
                    if fingerprint(raw, free_text) == done.fingerprint {
                        return Vec::new();
                    }
                    let mut call = done.call.clone();
                    call.conflicted = true;
                    self.completed.remove(&key);
                    self.pending.insert(key, call);
                    self.diagnostics.conflicting_snapshot =
                        self.diagnostics.conflicting_snapshot.saturating_add(1);
                }
                if !self.pending.contains_key(&key) {
                    self.missing_delta();
                    return Vec::new();
                }
                self.set_args(key, raw, false, true);
                self.end_call(key, true, true)
            }
            "response.completed" | "response.failed" | "response.incomplete" => {
                let Some(response) = event.get("response") else {
                    self.invalid_event();
                    return Vec::new();
                };
                let normal = kind == "response.completed"
                    && response.get("status").and_then(Value::as_str) == Some("completed")
                    && !response.get("error").is_some_and(|v| !v.is_null());
                let mut out = Vec::new();
                if let Some(items) = response.get("output").and_then(Value::as_array) {
                    for (i, item) in items.iter().enumerate() {
                        let key = self
                            .completed
                            .iter()
                            .find(|(_, done)| {
                                item.get("id")
                                    .and_then(Value::as_str)
                                    .is_some_and(|id| Some(id) == done.call.item_id.as_deref())
                            })
                            .map(|(key, _)| *key)
                            .unwrap_or((0, i as u64));
                        out.extend(self.responses_snapshot(key, item, normal, true));
                    }
                }
                if !normal {
                    let keys: Vec<_> = self.completed.keys().copied().collect();
                    for key in keys {
                        if let Some(done) = self.completed.get_mut(&key) {
                            if done.call.conflicted {
                                continue;
                            }
                            done.call.conflicted = true;
                            let call = done.call.clone();
                            out.push(self.build(key, &call, false));
                        }
                    }
                }
                let keys: Vec<_> = self.pending.keys().copied().collect();
                for key in keys {
                    out.extend(self.end_call(key, normal, true));
                }
                out
            }
            "error" => {
                self.mark_degraded_with_reason("upstream_error");
                Vec::new()
            }

            "response.created"
            | "response.in_progress"
            | "response.queued"
            | "response.output_text.delta"
            | "response.output_text.done"
            | "response.content_part.added"
            | "response.content_part.done"
            | "response.refusal.delta"
            | "response.refusal.done"
            | "response.reasoning_summary_part.added"
            | "response.reasoning_summary_part.done"
            | "response.reasoning_summary_text.delta"
            | "response.reasoning_summary_text.done" => Vec::new(),
            _ => {
                self.invalid_event();
                Vec::new()
            }
        }
    }
    fn feed_gemini(&mut self, event: &Value) -> Vec<ToolObservation> {
        let Some(candidates) = event.get("candidates").and_then(Value::as_array) else {
            self.invalid_event();
            return Vec::new();
        };
        let mut out = Vec::new();
        for (ci, candidate) in candidates.iter().enumerate() {
            let Some(index) = source_index(candidate, ci) else {
                self.invalid_event();
                continue;
            };
            if let Some(parts) = candidate
                .get("content")
                .and_then(|c| c.get("parts"))
                .and_then(Value::as_array)
            {
                for (pi, part) in parts.iter().enumerate() {
                    let Some(function) = part.get("functionCall") else {
                        continue;
                    };

                    if function.get("partialArgs").is_some()
                        || function.get("willContinue").is_some()
                    {
                        self.invalid_event();
                    }
                    let key = (index, pi as u64);
                    if !self.ensure_slot(key) {
                        continue;
                    }
                    self.update_identity(key, function, false);
                    if let Some(args) = function.get("args") {
                        self.set_value(key, args);
                    } else {
                        self.missing_delta();
                    }
                }
            }
            if candidate.get("finishReason").is_some_and(|v| !v.is_null()) {
                let normal = termination(candidate.get("finishReason"), &["STOP"])
                    == SourceTermination::Normal;
                out.extend(self.end_candidate(index, normal));
            }
        }
        out
    }
}

fn fingerprint(raw: &str, free_text: bool) -> [u8; 32] {
    if !free_text && raw.len() <= MAX_CALL_INPUT_BYTES {
        if let Ok(value) = serde_json::from_str::<Value>(raw) {
            if bounded_input_size(&value).is_ok() {
                return Sha256::digest(value.to_string().as_bytes()).into();
            }
        }
    }
    Sha256::digest(raw.as_bytes()).into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn ctx() -> StreamContext {
        StreamContext {
            scope_id: "s".into(),
            client: Some("claude_code".into()),
            execution_scope: ExecutionScope::Client,
        }
    }

    #[test]
    fn diagnostic_first_reason_identifies_null_without_changing_observation() {
        let mut a = StreamAssembler::new(PROTOCOL_OPENAI_CHAT, ctx());
        let event = json!({"choices":[{"delta":{"tool_calls":[{"index":0,"id":"PRIVATE_CALL_ID","function":{"name":"exec_command","arguments":"{\"cmd\":\"echo PRIVATE_TOKEN\"}"}}]}}]});
        let original = event.clone();
        a.feed(&event);
        assert_eq!(event, original);
        assert_eq!(a.diagnostics().first_reason, None);
        a.feed(&json!({"choices":[{"delta":{"tool_calls":[{"index":0,"function":null}]}}]}));
        assert_eq!(a.diagnostics().first_reason, Some("chat_function_null"));
        a.feed(&json!({"choices":null}));
        assert_eq!(a.diagnostics().first_reason, Some("chat_function_null"));
        let diagnostic = format!("{:?}", a.diagnostics());
        for secret in ["PRIVATE_CALL_ID", "PRIVATE_TOKEN", "exec_command", "echo"] {
            assert!(!diagnostic.contains(secret));
        }
        let out = a.finish();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].tool.name, "exec_command");
        assert_eq!(out[0].input.status, InputStatus::Incomplete);
        assert!(out[0].input.value.is_none());
    }

    #[test]
    fn diagnostic_normal_finish_and_external_fault_are_distinct() {
        let mut a = StreamAssembler::new(PROTOCOL_OPENAI_CHAT, ctx());
        a.feed(&json!({"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"name":"exec_command","arguments":"{\"cmd\":\"echo PRIVATE_TOKEN\"}"}}]}}]}));
        let out = a.feed(&json!({"choices":[{"delta":{},"finish_reason":"tool_calls"}]}));
        assert_eq!(out[0].input.status, InputStatus::Complete);
        assert_eq!(
            out[0].input.value,
            Some(json!({"cmd":"echo PRIVATE_TOKEN"}))
        );
        assert_eq!(a.diagnostics().first_reason, None);
        a.mark_degraded_with_reason("sequence_gap");
        assert_eq!(a.diagnostics().first_reason, Some("sequence_gap"));
        a.mark_degraded_with_reason("invalid_json");
        assert_eq!(a.diagnostics().first_reason, Some("sequence_gap"));
    }

    #[test]
    fn diagnostic_responses_error_has_fixed_upstream_reason() {
        let mut a = StreamAssembler::new(PROTOCOL_OPENAI_RESPONSES, ctx());
        a.feed(&json!({"type":"error","message":"PRIVATE_UPSTREAM_ERROR"}));
        assert_eq!(a.diagnostics().first_reason, Some("upstream_error"));
        assert!(!format!("{:?}", a.diagnostics()).contains("PRIVATE_UPSTREAM_ERROR"));
    }

    #[test]
    fn regression_chat_empty_tool_carriers_preserve_real_calls() {
        for empty in [Value::Null, json!([])] {
            let mut a = StreamAssembler::new(PROTOCOL_OPENAI_CHAT, ctx());
            let placeholder =
                json!({"choices":[{"index":0,"delta":{"tool_calls":empty,"function_call":null}}]});
            let before = placeholder.clone();
            assert!(a.feed(&placeholder).is_empty());
            a.feed(&json!({"choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"function":{"name":"exec_command","arguments":"{\"cmd\":"}}]}}]}));
            a.feed(&placeholder);
            a.feed(&json!({"choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"function":{"arguments":"\"ls\",\"workdir\":\"/workspace\",\"yield_time_ms\":1000}"}}]}}]}));
            let out = a.feed(&json!({"choices":[{"index":0,"delta":{"tool_calls":empty},"finish_reason":"tool_calls"}]}));
            assert_eq!(placeholder, before);
            assert_eq!(out.len(), 1);
            assert_eq!(out[0].input.status, InputStatus::Complete);
            assert_eq!(
                out[0].input.value,
                Some(json!({"cmd":"ls","workdir":"/workspace","yield_time_ms":1000}))
            );
            assert_eq!(a.diagnostics().first_reason, None);
            assert!(a.finish().is_empty());
        }
    }

    #[test]
    fn regression_chat_empty_tool_calls_allow_legacy_function_calls() {
        for empty in [Value::Null, json!([])] {
            let mut a = StreamAssembler::new(PROTOCOL_OPENAI_CHAT, ctx());
            let out = a.feed(&json!({"choices":[{"message":{"tool_calls":empty,
                "function_call":{"name":"exec_command","arguments":"{\"cmd\":\"pwd\"}"}},"finish_reason":"function_call"}]}));
            assert_eq!(out.len(), 1);
            assert_eq!(out[0].tool.name, "exec_command");
            assert_eq!(out[0].input.status, InputStatus::Complete);
            assert_eq!(out[0].input.value, Some(json!({"cmd":"pwd"})));
        }
    }

    #[test]
    fn regression_chat_non_array_carriers_are_isolated_and_not_coerced() {
        for (invalid, reason) in [
            (
                json!({"index":0,"function":{"name":"exec_command","arguments":"{}"}}),
                "chat_tool_calls_object",
            ),
            (
                json!("PRIVATE_INVALID_TOOL_CALLS"),
                "chat_tool_calls_string",
            ),
            (json!(42), "chat_tool_calls_number"),
            (json!(true), "chat_tool_calls_boolean"),
        ] {
            for before_call in [true, false] {
                let mut a = StreamAssembler::new(PROTOCOL_OPENAI_CHAT, ctx());
                let malformed = json!({"choices":[{"index":0,"delta":{"tool_calls":invalid}}]});
                let original = malformed.clone();
                if before_call {
                    assert!(a.feed(&malformed).is_empty());
                }
                a.feed(&json!({"choices":[
                    {"index":0,"delta":{"tool_calls":[{"index":0,"function":{"name":"exec_command","arguments":"{\"cmd\":\"echo hi\"}"}}]}},
                    {"index":1,"delta":{"tool_calls":[{"index":0,"function":{"name":"exec_command","arguments":"{\"cmd\":\"pwd\"}"}}]}}
                ]}));
                if !before_call {
                    assert!(a.feed(&malformed).is_empty());
                }
                let out = a.feed(&json!({"choices":[{"index":0,"finish_reason":"tool_calls"},{"index":1,"finish_reason":"tool_calls"}]}));
                assert_eq!(out.len(), 2);
                assert_eq!(out[0].input.status, InputStatus::Incomplete);
                assert_eq!(out[0].input.value, Some(json!({"cmd":"echo hi"})));
                assert_eq!(out[0].quality.input_fidelity, InputFidelity::Preserved);
                assert_eq!(out[1].input.status, InputStatus::Complete);
                assert_eq!(out[1].input.value, Some(json!({"cmd":"pwd"})));
                assert_eq!(a.diagnostics().first_reason, Some(reason));
                assert_eq!(a.diagnostics().invalid_event, 1);
                assert_eq!(malformed, original);
                assert!(!format!("{:?}", a.diagnostics()).contains("PRIVATE_INVALID"));
                assert!(a.finish().is_empty());
            }
        }
    }

    #[test]
    fn regression_chat_malformed_carriers_alone_never_invent_calls() {
        for carrier in [
            Value::Null,
            json!([]),
            json!({}),
            json!("bad"),
            json!(false),
            json!(12),
        ] {
            let mut a = StreamAssembler::new(PROTOCOL_OPENAI_CHAT, ctx());
            assert!(a
                .feed(&json!({"choices":[{"delta":{"tool_calls":carrier},"finish_reason":"stop"}]}))
                .is_empty());
            assert!(a.finish().is_empty());
        }
    }

    #[test]
    fn regression_chat_fault_candidates_are_bounded_without_index_truncation() {
        let mut a = StreamAssembler::new(PROTOCOL_OPENAI_CHAT, ctx());
        for index in 0..MAX_PENDING_CALLS as u64 {
            a.feed(&json!({"choices":[{"index":index,"delta":{"tool_calls":false}}]}));
        }
        assert_eq!(a.chat_faults.len(), MAX_PENDING_CALLS);
        assert!(!a.is_degraded());
        for _ in 0..3 {
            a.feed(&json!({"choices":[{"index":0,"delta":{"tool_calls":17}}]}));
        }
        assert_eq!(a.chat_faults.len(), MAX_PENDING_CALLS);
        assert!(!a.is_degraded());
        a.feed(&json!({"choices":[{"index":u64::MAX,"delta":{"tool_calls":{}}}]}));
        assert_eq!(a.chat_faults.len(), MAX_PENDING_CALLS);
        assert!(a.is_degraded());
        assert_eq!(a.diagnostics().budget_exceeded, 1);
        assert!(a.finish().is_empty());
        assert!(a.chat_faults.is_empty());

        let mut a = StreamAssembler::new(PROTOCOL_OPENAI_CHAT, ctx());
        a.feed(&json!({"choices":[{"index":u64::MAX,"delta":{"tool_calls":"bad"}}]}));
        a.feed(&json!({"choices":[
            {"index":u64::MAX,"delta":{"tool_calls":[{"function":{"name":"exec_command","arguments":"{\"cmd\":\"echo hi\"}"}}]}},
            {"index":0,"delta":{"tool_calls":[{"function":{"name":"exec_command","arguments":"{\"cmd\":\"pwd\"}"}}]}}
        ]}));
        let out = a.feed(&json!({"choices":[{"index":u64::MAX,"finish_reason":"tool_calls"},{"index":0,"finish_reason":"tool_calls"}]}));
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].input.status, InputStatus::Incomplete);
        assert_eq!(out[0].correlation.candidate_index, None);
        assert_eq!(out[1].input.status, InputStatus::Complete);
        assert_eq!(out[1].correlation.candidate_index, Some(0));
        assert!(a.finish().is_empty());
        assert!(a.chat_faults.is_empty());
    }

    fn anthropic() -> StreamAssembler {
        StreamAssembler::new(PROTOCOL_ANTHROPIC, ctx())
    }

    #[test]
    fn anthropic_assembles_tool_use_across_deltas() {
        let mut a = anthropic();
        assert!(a
            .feed(&json!({"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"toolu_1","name":"Bash"}}))
            .is_empty());
        assert!(a
            .feed(&json!({"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":"{\"command\":"}}))
            .is_empty());
        assert!(a
            .feed(&json!({"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":"\"rm -rf /tmp/x\"}"}}))
            .is_empty());
        let out = a.feed(&json!({"type":"content_block_stop","index":0}));
        assert_eq!(out.len(), 1);
        let o = &out[0];
        assert_eq!(o.tool.name, "Bash");
        assert_eq!(o.correlation.call_id.as_deref(), Some("toolu_1"));
        assert_eq!(o.input.value, Some(json!({"command": "rm -rf /tmp/x"})));
        assert_eq!(o.input.status, InputStatus::Complete);
        assert_eq!(o.quality.source_termination, SourceTermination::Normal);
        assert!(o.validate().is_ok());
    }

    #[test]
    fn anthropic_unterminated_call_is_incomplete() {
        let mut a = anthropic();
        a.feed(&json!({"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"t1","name":"Bash"}}));
        a.feed(&json!({"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":"{\"command\":\"ls\"}"}}));
        let out = a.finish();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].input.status, InputStatus::Incomplete);
        assert_eq!(
            out[0].quality.source_termination,
            SourceTermination::Abnormal
        );
    }

    #[test]
    fn anthropic_invalid_json_is_invalid_not_complete() {
        let mut a = anthropic();
        a.feed(&json!({"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"t1","name":"Bash"}}));
        a.feed(&json!({"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":"{not json"}}));
        let out = a.feed(&json!({"type":"content_block_stop","index":0}));
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].input.status, InputStatus::Invalid);
        assert!(out[0].validate().is_ok());
    }

    #[test]
    fn anthropic_ignores_non_tool_blocks() {
        let mut a = anthropic();
        let out = a.feed(&json!({"type":"content_block_start","index":0,"content_block":{"type":"text","text":"hi"}}));
        assert!(out.is_empty());
        assert!(a.finish().is_empty());
    }

    #[test]
    fn chat_assembles_tool_calls_across_deltas() {
        let mut a = StreamAssembler::new(PROTOCOL_OPENAI_CHAT, ctx());
        a.feed(&json!({"choices":[{"delta":{"tool_calls":[{"index":0,"id":"call_1","function":{"name":"Bash","arguments":"{\"command\":"}}]}}]}));
        a.feed(&json!({"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":"\"ls\"}"}}]}}]}));
        let out = a.feed(&json!({"choices":[{"delta":{},"finish_reason":"tool_calls"}]}));
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].tool.name, "Bash");
        assert_eq!(out[0].correlation.call_id.as_deref(), Some("call_1"));
        assert_eq!(out[0].input.value, Some(json!({"command": "ls"})));
        assert_eq!(out[0].input.status, InputStatus::Complete);
    }

    #[test]
    fn chat_separates_parallel_calls_by_index() {
        let mut a = StreamAssembler::new(PROTOCOL_OPENAI_CHAT, ctx());
        a.feed(&json!({"choices":[{"delta":{"tool_calls":[{"index":0,"id":"c0","function":{"name":"Bash","arguments":"{\"command\":\"ls\"}"}},{"index":1,"id":"c1","function":{"name":"Read","arguments":"{\"path\":\"/x\"}"}}]}}]}));
        let out = a.feed(&json!({"choices":[{"delta":{},"finish_reason":"tool_calls"}]}));
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].tool.name, "Bash");
        assert_eq!(out[1].tool.name, "Read");
    }

    #[test]
    fn responses_assembles_function_call() {
        let mut a = StreamAssembler::new(PROTOCOL_OPENAI_RESPONSES, ctx());
        a.feed(&json!({"type":"response.output_item.added","output_index":0,"item":{"type":"function_call","call_id":"call_1","id":"item_1","name":"Bash"}}));
        a.feed(&json!({"type":"response.function_call_arguments.delta","output_index":0,"delta":"{\"command\":"}));
        a.feed(&json!({"type":"response.function_call_arguments.delta","output_index":0,"delta":"\"ls\"}"}));
        let out = a.feed(&json!({"type":"response.function_call_arguments.done","output_index":0,"arguments":"{\"command\":\"ls\"}"}));
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].tool.name, "Bash");
        assert_eq!(out[0].correlation.call_id.as_deref(), Some("call_1"));
        assert_eq!(out[0].correlation.item_id.as_deref(), Some("item_1"));
        assert_eq!(out[0].input.value, Some(json!({"command": "ls"})));
        assert_eq!(out[0].input.status, InputStatus::Complete);
    }

    #[test]
    fn responses_custom_tool_call_is_free_text() {
        let mut a = StreamAssembler::new(PROTOCOL_OPENAI_RESPONSES, ctx());
        a.feed(&json!({"type":"response.output_item.added","output_index":0,"item":{"type":"custom_tool_call","call_id":"c1","name":"apply_patch"}}));
        a.feed(&json!({"type":"response.custom_tool_call_input.delta","output_index":0,"delta":"*** Begin Patch"}));
        let out = a.feed(&json!({"type":"response.custom_tool_call_input.done","output_index":0,"input":"*** Begin Patch\n*** End Patch"}));
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].input.encoding, InputEncoding::Text);
        assert_eq!(out[0].input.status, InputStatus::Complete);
    }

    #[test]
    fn gemini_assembles_function_call_snapshot() {
        let mut a = StreamAssembler::new(PROTOCOL_GEMINI, ctx());
        a.feed(&json!({"candidates":[{"content":{"parts":[{"functionCall":{"name":"Bash","args":{"command":"ls"}}}]}}]}));
        let out = a.feed(&json!({"candidates":[{"content":{"parts":[{"functionCall":{"name":"Bash","args":{"command":"ls"}}}]},"finishReason":"STOP"}]}));
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].tool.name, "Bash");
        assert_eq!(out[0].input.value, Some(json!({"command": "ls"})));
        assert_eq!(out[0].input.status, InputStatus::Complete);
    }

    #[test]
    fn gemini_snapshot_replaces_not_appends() {
        let mut a = StreamAssembler::new(PROTOCOL_GEMINI, ctx());
        a.feed(&json!({"candidates":[{"content":{"parts":[{"functionCall":{"name":"Bash","args":{"command":"l"}}}]}}]}));
        a.feed(&json!({"candidates":[{"content":{"parts":[{"functionCall":{"name":"Bash","args":{"command":"ls -la"}}}]}}]}));
        let out = a.feed(&json!({"candidates":[{"content":{"parts":[{"functionCall":{"name":"Bash","args":{"command":"ls -la"}}}]},"finishReason":"STOP"}]}));
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].input.value, Some(json!({"command": "ls -la"})));
    }

    #[test]
    fn client_app_name_is_not_a_response_protocol() {
        for app in ["unknown", "claude", "codex", "gemini"] {
            let mut assembler = StreamAssembler::new(app, ctx());
            assert!(assembler.feed(&json!({"type":"content_block_start","index":0,
                "content_block":{"type":"tool_use","id":"c","name":"Bash","input":{"command":"ls"}}})).is_empty());
            assert!(assembler.finish().is_empty());
        }
    }

    #[test]
    fn anthropic_text_and_tool_blocks_do_not_cross_contaminate() {
        let mut a = anthropic();

        a.feed(&json!({"type":"content_block_start","index":0,"content_block":{"type":"text"}}));
        a.feed(&json!({"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"hi"}}));
        a.feed(&json!({"type":"content_block_stop","index":0}));

        a.feed(&json!({"type":"content_block_start","index":1,"content_block":{"type":"tool_use","id":"t1","name":"Bash"}}));
        a.feed(&json!({"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":"{\"command\":\"ls\"}"}}));
        let out = a.feed(&json!({"type":"content_block_stop","index":1}));
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].tool.name, "Bash");
        assert_eq!(out[0].input.value, Some(json!({"command": "ls"})));
        assert!(a.finish().is_empty());
    }

    #[test]
    fn pending_limit_degrades_without_panic() {
        let mut a = anthropic();
        for i in 0..(MAX_PENDING_CALLS as u64 + 5) {
            a.feed(&json!({"type":"content_block_start","index":i,"content_block":{"type":"tool_use","id":format!("t{i}"),"name":"Bash"}}));
        }
        assert!(a.is_degraded());

        let out = a.feed(&json!({"type":"content_block_stop","index":0}));
        assert_eq!(out.len(), 1);
    }

    #[test]
    fn regression_anthropic_empty_start_input_is_not_a_delta_prefix() {
        let mut a = anthropic();
        a.feed(&json!({"type":"content_block_start","index":0,"content_block":{"type":"tool_use","name":"Bash","input":{}}}));
        a.feed(&json!({"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":"{\"command\":\"ls\"}"}}));
        let out = a.feed(&json!({"type":"content_block_stop","index":0}));
        assert_eq!(out[0].input.value, Some(json!({"command":"ls"})));
        assert_eq!(out[0].input.status, InputStatus::Complete);
    }

    #[test]
    fn regression_chat_choice_indices_survive_reordering_and_huge_tool_indices() {
        let mut a = StreamAssembler::new(PROTOCOL_OPENAI_CHAT, ctx());
        a.feed(&json!({"choices":[
            {"index":7,"delta":{"tool_calls":[{"index":4294967296u64,"function":{"name":"A","arguments":"{}"}}]}},
            {"index":6,"delta":{"tool_calls":[{"index":0,"function":{"name":"B","arguments":"{}"}}]}}
        ]}));
        let first =
            a.feed(&json!({"choices":[{"index":6,"delta":{},"finish_reason":"tool_calls"}]}));
        assert_eq!(first.len(), 1);
        assert_eq!(first[0].tool.name, "B");
        assert_eq!(first[0].correlation.candidate_index, Some(6));
        let second =
            a.feed(&json!({"choices":[{"index":7,"delta":{},"finish_reason":"tool_calls"}]}));
        assert_eq!(second.len(), 1);
        assert_eq!(second[0].tool.name, "A");
        assert_eq!(second[0].correlation.tool_index, None);
    }

    #[test]
    fn regression_chat_complete_message_snapshot_replaces_delta() {
        let mut a = StreamAssembler::new(PROTOCOL_OPENAI_CHAT, ctx());
        a.feed(&json!({"choices":[{"index":2,"delta":{"tool_calls":[{"index":0,"function":{"name":"Bash","arguments":"{\"command\":"}}]}}]}));
        let out = a.feed(&json!({"choices":[{"index":2,"message":{"tool_calls":[{"function":{"name":"Bash","arguments":"{\"command\":\"ls\"}"}}]},"finish_reason":"tool_calls"}]}));
        assert_eq!(out[0].input.value, Some(json!({"command":"ls"})));
        assert_eq!(out[0].input.status, InputStatus::Complete);
    }

    #[test]
    fn regression_chat_abnormal_finish_does_not_complete() {
        for reason in ["length", "content_filter", "error", "unknown"] {
            let mut a = StreamAssembler::new(PROTOCOL_OPENAI_CHAT, ctx());
            a.feed(&json!({"choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"function":{"name":"Bash","arguments":"{}"}}]}}]}));
            let out = a.feed(&json!({"choices":[{"index":0,"finish_reason":reason}]}));
            assert_eq!(out[0].input.status, InputStatus::Incomplete, "{reason}");
            assert_eq!(
                out[0].quality.source_termination,
                SourceTermination::Abnormal
            );
        }
    }

    #[test]
    fn regression_gemini_candidate_indices_and_terminal_without_parts() {
        let mut a = StreamAssembler::new(PROTOCOL_GEMINI, ctx());
        a.feed(&json!({"candidates":[{"index":8,"content":{"parts":[{"functionCall":{"name":"A","args":{}}}]}},{"index":2,"content":{"parts":[{"functionCall":{"name":"B","args":{}}}]}}]}));
        let out = a.feed(&json!({"candidates":[{"index":2,"finishReason":"MAX_TOKENS"},{"index":8,"finishReason":"STOP"}]}));
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].tool.name, "B");
        assert_eq!(out[0].input.status, InputStatus::Incomplete);
        assert_eq!(out[1].correlation.candidate_index, Some(8));
        assert_eq!(out[1].input.status, InputStatus::Complete);
    }

    #[test]
    fn regression_real_name_without_call_id_retains_source_identity() {
        let mut a = anthropic();
        a.feed(&json!({"type":"content_block_start","index":0,"content_block":{"type":"tool_use","name":"Bash","input":{}}}));
        let out = a.feed(&json!({"type":"content_block_stop","index":0}));
        assert_eq!(
            out[0].correlation.identity_quality,
            Some(IdentityQuality::Synthetic)
        );
        assert_eq!(out[0].quality.tool_identity, ToolIdentityQuality::Source);
        let mut a = anthropic();
        a.feed(&json!({"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"source-id","input":{}}}));
        let out = a.feed(&json!({"type":"content_block_stop","index":0}));
        assert_eq!(out[0].quality.tool_identity, ToolIdentityQuality::Unknown);
    }

    #[test]
    fn regression_responses_conflicting_terminal_snapshots_emit_degraded_revision() {
        let mut a = StreamAssembler::new(PROTOCOL_OPENAI_RESPONSES, ctx());
        a.feed(&json!({"type":"response.output_item.added","output_index":0,"item":{"type":"function_call","id":"i","call_id":"c","name":"Bash"}}));
        a.feed(&json!({"type":"response.function_call_arguments.delta","output_index":0,"delta":"{\"command\":\"ls\"}"}));
        let first = a.feed(&json!({"type":"response.function_call_arguments.done","output_index":0,"arguments":"{\"command\":\"ls\"}"}));
        assert_eq!(first.len(), 1);
        let revision = a.feed(&json!({"type":"response.output_item.done","output_index":0,"item":{"type":"function_call","id":"i","call_id":"c","name":"Bash","status":"completed","arguments":"{\"command\":\"pwd\"}"}}));
        assert_eq!(revision.len(), 1);
        assert_ne!(revision[0].observation_id, first[0].observation_id);
        assert_eq!(revision[0].input.status, InputStatus::Incomplete);
        assert_eq!(
            revision[0].quality.input_fidelity,
            InputFidelity::RepairedOrDropped
        );
        let completed = a.feed(&json!({"type":"response.completed","response":{"status":"completed","output":[{"type":"function_call","id":"i","call_id":"c","name":"Bash","arguments":"{}"}]}}));
        assert_eq!(completed.len(), 1);
        assert_ne!(completed[0].input.status, InputStatus::Complete);
    }

    #[test]
    fn regression_responses_done_conflict_with_delta_cannot_complete() {
        let mut a = StreamAssembler::new(PROTOCOL_OPENAI_RESPONSES, ctx());
        a.feed(&json!({"type":"response.output_item.added","output_index":0,"item":{"type":"function_call","name":"Bash"}}));
        a.feed(
            &json!({"type":"response.function_call_arguments.delta","output_index":0,"delta":"{}"}),
        );
        let out = a.feed(&json!({"type":"response.function_call_arguments.done","output_index":0,"arguments":"{\"x\":1}"}));
        assert_eq!(out[0].input.status, InputStatus::Incomplete);
        assert_eq!(
            out[0].quality.input_fidelity,
            InputFidelity::RepairedOrDropped
        );
    }

    #[test]
    fn regression_overflow_releases_all_input_and_prevents_completion() {
        let mut a = anthropic();
        a.feed(&json!({"type":"content_block_start","index":0,"content_block":{"type":"tool_use","name":"Bash"}}));
        a.feed(&json!({"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":"x".repeat(MAX_CALL_INPUT_BYTES)}}));
        a.feed(&json!({"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":"x"}}));
        assert!(a.is_degraded());
        assert!(a
            .pending
            .values()
            .all(|call| call.args.is_empty() && call.args.capacity() == 0));
        let out = a.feed(&json!({"type":"content_block_stop","index":0}));
        assert_eq!(out[0].input.value, None);
        assert_eq!(out[0].input.status, InputStatus::Incomplete);
    }

    #[test]
    fn regression_response_budget_releases_input_at_two_mib() {
        let mut a = anthropic();
        for i in 0..9 {
            a.feed(&json!({"type":"content_block_start","index":i,"content_block":{"type":"tool_use","name":"Bash"}}));
            a.feed(&json!({"type":"content_block_delta","index":i,"delta":{"type":"input_json_delta","partial_json":"x".repeat(MAX_CALL_INPUT_BYTES)}}));
        }
        assert!(a.is_degraded());
        assert!(a
            .pending
            .values()
            .all(|call| call.args.is_empty() && call.args.capacity() == 0));
        assert!(a
            .finish()
            .iter()
            .all(|o| o.input.status == InputStatus::Incomplete));
    }

    #[test]
    fn regression_missing_delta_and_illegal_event_degrade_response() {
        for protocol in [
            PROTOCOL_ANTHROPIC,
            PROTOCOL_OPENAI_CHAT,
            PROTOCOL_OPENAI_RESPONSES,
            PROTOCOL_GEMINI,
        ] {
            let mut a = StreamAssembler::new(protocol, ctx());
            a.feed(&json!("illegal-event"));
            assert!(a.is_degraded(), "{protocol}");
        }
        let mut a = anthropic();
        a.feed(&json!({"type":"content_block_start","index":0,"content_block":{"type":"tool_use","name":"Bash","input":{}}}));
        a.feed(
            &json!({"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta"}}),
        );
        let out = a.feed(&json!({"type":"content_block_stop","index":0}));
        assert_eq!(out[0].input.status, InputStatus::Incomplete);
    }

    #[test]
    fn regression_completed_response_identity_changes_emit_untrusted_revision() {
        for (field, changed) in [
            ("namespace", "other"),
            ("call_id", "another"),
            ("id", "other-item"),
        ] {
            let mut assembler = StreamAssembler::new(PROTOCOL_OPENAI_RESPONSES, ctx());
            let mut item = json!({"type":"function_call","name":"Bash","id":"item","call_id":"call","arguments":"{}"});
            assembler
                .feed(&json!({"type":"response.output_item.added","output_index":0,"item":item}));
            assert_eq!(assembler.feed(&json!({"type":"response.function_call_arguments.done","output_index":0,"arguments":"{}"}))[0].input.status, InputStatus::Complete);
            item[field] = json!(changed);
            let result = assembler
                .feed(&json!({"type":"response.output_item.done","output_index":0,"item":item}));
            assert_eq!(result.len(), 1, "{field}");
            assert_ne!(result[0].input.status, InputStatus::Complete);
            assert_eq!(
                result[0].quality.tool_identity,
                ToolIdentityQuality::Unknown
            );
        }
    }
}
