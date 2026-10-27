//! Budgeted original Chat tool fragments; converter repairs grant no evidence.
use super::{
    payload_budget::PayloadBudget, payload_buffer::PayloadBuffer, payload_json::PayloadJson,
    payload_metadata::PayloadVec, payload_text::PayloadText, tokenization::TokenError,
};
use serde_json::Value;

const ARGUMENT_LIMIT: usize = 1024 * 1024;

struct ChatTool {
    id: Option<PayloadText>,
    name: Option<PayloadText>,
    function_type: bool,
    arguments_seen: bool,
    arguments: PayloadBuffer,
}

pub(super) struct ChatTools {
    tools: PayloadVec<ChatTool>,
}

impl ChatTools {
    pub(super) fn new(budget: PayloadBudget) -> Result<Self, TokenError> {
        Ok(Self { tools: PayloadVec::new(budget)? })
    }

    pub(super) fn observe(&mut self, value: &Value, budget: &PayloadBudget) -> Result<(), TokenError> {
        let tools = value.as_array().ok_or(TokenError::RedactionFailed)?;
        for delta in tools {
            let index = delta.get("index").and_then(Value::as_u64)
                .filter(|index| *index < 128).ok_or(TokenError::RedactionFailed)? as usize;
            if index > self.tools.len() { return Err(TokenError::RedactionFailed); }
            if index == self.tools.len() {
                self.tools.push(ChatTool {
                    id: None, name: None, function_type: false, arguments_seen: false,
                    arguments: PayloadBuffer::new(budget.clone(), ARGUMENT_LIMIT)?,
                })?;
            }
            let tool = self.tools.get_mut(index).ok_or(TokenError::RedactionFailed)?;
            if let Some(kind) = delta.get("type") {
                if kind.as_str() != Some("function") { return Err(TokenError::RedactionFailed); }
                tool.function_type = true;
            }
            set_identity(&mut tool.id, delta.get("id"), budget)?;
            let function = delta.get("function").and_then(Value::as_object).ok_or(TokenError::RedactionFailed)?;
            set_identity(&mut tool.name, function.get("name"), budget)?;
            if let Some(arguments) = function.get("arguments") {
                let arguments = arguments.as_str().ok_or(TokenError::RedactionFailed)?;
                tool.arguments_seen = true;
                tool.arguments.append(arguments.as_bytes())?;
            }
        }
        Ok(())
    }

    pub(super) fn finish(&self, tool_reason: bool, budget: &PayloadBudget) -> Result<(), TokenError> {
        if tool_reason != (self.tools.len() != 0) { return Err(TokenError::RedactionFailed); }
        for (index, tool) in self.tools.as_slice().iter().enumerate() {
            let id = tool.id.as_ref().ok_or(TokenError::RedactionFailed)?;
            if !tool.function_type || tool.name.is_none() || !tool.arguments_seen
                || self.tools.as_slice()[..index].iter().any(|prior| prior.id.as_ref().is_some_and(|prior| prior.as_str() == id.as_str())) {
                return Err(TokenError::RedactionFailed);
            }
            let arguments = std::str::from_utf8(tool.arguments.as_slice()).map_err(|_| TokenError::RedactionFailed)?;
            if arguments.trim().is_empty() { return Err(TokenError::RedactionFailed); }
            let parsed = PayloadJson::try_from_str(arguments, budget.clone(), budget.limit())?;
            if !parsed.as_value().is_object() { return Err(TokenError::RedactionFailed); }
        }
        Ok(())
    }
}

fn set_identity(target: &mut Option<PayloadText>, value: Option<&Value>, budget: &PayloadBudget) -> Result<(), TokenError> {
    if let Some(value) = value {
        let value = value.as_str().filter(|value| !value.is_empty() && value.len() <= 256
            && !value.chars().any(|ch| ch.is_whitespace() || ch.is_control())).ok_or(TokenError::RedactionFailed)?;
        if let Some(prior) = target {
            if prior.as_str() != value { return Err(TokenError::RedactionFailed); }
        } else {
            *target = Some(PayloadText::try_from_str(value, budget.clone(), 256)?);
        }
    }
    Ok(())
}

struct AnthropicTool {
    id: PayloadText,
    read_tool: bool,
    start_input_nonempty: bool,
    delta_seen: bool,
    arguments: PayloadBuffer,
}

pub(super) struct AnthropicTools {
    completed_ids: PayloadVec<PayloadText>,
    current: Option<AnthropicTool>,
}

impl AnthropicTools {
    pub(super) fn new(budget: PayloadBudget) -> Result<Self, TokenError> {
        Ok(Self { completed_ids: PayloadVec::new(budget)?, current: None })
    }

    pub(super) fn start(&mut self, block: &Value, budget: &PayloadBudget) -> Result<(), TokenError> {
        if self.current.is_some() || self.completed_ids.len() >= 128 { return Err(TokenError::RedactionFailed); }
        let mut id = None;
        let mut name = None;
        set_identity(&mut id, block.get("id"), budget)?;
        set_identity(&mut name, block.get("name"), budget)?;
        let id = id.ok_or(TokenError::RedactionFailed)?;
        let name = name.ok_or(TokenError::RedactionFailed)?;
        if self.completed_ids.as_slice().iter().any(|prior| prior.as_str() == id.as_str()) { return Err(TokenError::RedactionFailed); }
        let input = block.get("input").and_then(Value::as_object).ok_or(TokenError::RedactionFailed)?;
        let read_tool = name.as_str() == "Read";
        if read_tool && input.get("pages").and_then(Value::as_str) == Some("") { return Err(TokenError::RedactionFailed); }
        self.current = Some(AnthropicTool {
            id, read_tool, start_input_nonempty: !input.is_empty(), delta_seen: false,
            arguments: PayloadBuffer::new(budget.clone(), ARGUMENT_LIMIT)?,
        });
        Ok(())
    }

    pub(super) fn delta(&mut self, data: &Value) -> Result<(), TokenError> {
        let tool = self.current.as_mut().ok_or(TokenError::RedactionFailed)?;
        if tool.start_input_nonempty { return Err(TokenError::RedactionFailed); }
        let text = data.pointer("/delta/partial_json").and_then(Value::as_str).ok_or(TokenError::RedactionFailed)?;
        tool.delta_seen = true;
        tool.arguments.append(text.as_bytes())
    }

    pub(super) fn close(&mut self, budget: &PayloadBudget) -> Result<(), TokenError> {
        let tool = self.current.take().ok_or(TokenError::RedactionFailed)?;
        if tool.delta_seen {
            let encoded = std::str::from_utf8(tool.arguments.as_slice()).map_err(|_| TokenError::RedactionFailed)?;
            if encoded.trim().is_empty() { return Err(TokenError::RedactionFailed); }
            let parsed = PayloadJson::try_from_str(encoded, budget.clone(), budget.limit())?;
            if !parsed.as_value().is_object() || (tool.read_tool && parsed.as_value().get("pages").and_then(Value::as_str) == Some("")) {
                return Err(TokenError::RedactionFailed);
            }
        }
        self.completed_ids.push(tool.id)
    }

    pub(super) fn finish(&self, tool_reason: bool) -> bool {
        self.current.is_none() && tool_reason == (self.completed_ids.len() != 0)
    }
}
