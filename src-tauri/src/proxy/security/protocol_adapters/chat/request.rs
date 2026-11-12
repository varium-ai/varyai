//! Borrowed CH1 request observation only; native routes remain Disabled.

use serde_json::Value;

use crate::proxy::{
    protocol_bridge::Protocol,
    security::{
        payload_budget::PayloadBudget,
        restoration_contract::{
            ClientProtocol, ContractError, EditConstraint, HistoryClaim, RequestObservation,
            SourceClass,
        },
    },
};

/// Borrow exact native sites using the caller's budget; grant no route or history authority.
/// The shared owner charges retained metadata and drops partial observations on failure.
pub(in crate::proxy::security) fn observe_request<'native>(
    native: &'native Value,
    budget: PayloadBudget,
) -> Result<RequestObservation<'native>, ContractError> {
    let mut observation =
        RequestObservation::new(native, ClientProtocol::new(Protocol::Chat), budget)?;
    let Some(messages) = native.get("messages").and_then(Value::as_array) else {
        return Ok(observation);
    };
    // Match CurrentTurn's latest-user boundary without allocating diagnostic paths.
    let latest_user = messages
        .iter()
        .rposition(|message| message.get("role").and_then(Value::as_str) == Some("user"));
    for (index, message) in messages.iter().enumerate() {
        let Some(role) = message.get("role").and_then(Value::as_str) else {
            continue;
        };
        let source = match role {
            "system" | "developer" => SourceClass::CurrentInput,
            "user" if latest_user.is_some_and(|latest| index < latest) => {
                SourceClass::HistoricalText
            }
            "user" => SourceClass::CurrentInput,
            "assistant" => SourceClass::HistoricalText,
            "tool" => SourceClass::ToolResult,
            _ => continue,
        };
        if let Some(content) = message.get("content") {
            if content.is_string() {
                observation.observe_text(content, source, EditConstraint::Editable)?;
            } else if let Some(parts) = content.as_array() {
                for part in parts {
                    if part.get("type").and_then(Value::as_str) != Some("text") {
                        continue;
                    }
                    if let Some(text) = part.get("text").filter(|text| text.is_string()) {
                        observation.observe_text(text, source, EditConstraint::Editable)?;
                    }
                }
            }
        }
        match role {
            "assistant" => {
                observation.claim_history(HistoryClaim::AssistantItem { native: message })?;
                if let Some(calls) = message.get("tool_calls").and_then(Value::as_array) {
                    for call in calls {
                        if call.get("type").and_then(Value::as_str) != Some("function") {
                            continue;
                        }
                        if let Some(arguments) = call
                            .get("function")
                            .and_then(|function| function.get("arguments"))
                            .filter(|arguments| arguments.is_string())
                        {
                            observation.observe_text(
                                arguments,
                                SourceClass::ToolArguments,
                                EditConstraint::Editable,
                            )?;
                        }
                    }
                }
            }
            "tool" => observation.claim_history(HistoryClaim::OpaqueItem {
                native: message,
                source: SourceClass::ToolResult,
            })?,
            _ => {}
        }
    }
    Ok(observation)
}
