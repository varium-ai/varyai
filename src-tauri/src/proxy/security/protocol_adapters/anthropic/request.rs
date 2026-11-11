//! Borrow supported Anthropic request text without granting route or history authority.
use serde_json::Value;

use crate::proxy::{
    protocol_bridge::Protocol,
    security::{
        payload_budget::PayloadBudget,
        restoration_contract::{
            ClientProtocol, ContractError, EditConstraint, HistoryClaim, RequestObservation,
            SourceClass,
        },
        tokenization::TokenError,
    },
};

// Match the shared native request membership search bounds, with root at depth zero.
const MAX_DEPTH: usize = 64;
const MAX_WORK: usize = 4 * 1024 * 1024;

/// Borrow the exact native owner using the caller's existing attempt budget.
/// Sites and history claims are fallible charged metadata, never mapping proof.
pub(in crate::proxy::security) fn observe_request<'native>(
    native: &'native Value,
    budget: PayloadBudget,
) -> Result<RequestObservation<'native>, ContractError> {
    let mut observed =
        RequestObservation::new(native, ClientProtocol::new(Protocol::Anthropic), budget)?;
    let mut remaining_work = MAX_WORK;
    visit_node(0, &mut remaining_work)?;
    if let Some(system) = native.get("system") {
        observe_content(
            &mut observed,
            system,
            SourceClass::CurrentInput,
            false,
            1,
            &mut remaining_work,
        )?;
    }
    if let Some(messages) = native.get("messages").and_then(Value::as_array) {
        visit_node(1, &mut remaining_work)?;
        for message in messages {
            visit_node(2, &mut remaining_work)?;
            let source = match message.get("role").and_then(Value::as_str) {
                Some("user") => SourceClass::CurrentInput,
                Some("assistant") => {
                    observed.claim_history(HistoryClaim::AssistantItem { native: message })?;
                    SourceClass::HistoricalText
                }
                _ => continue,
            };
            if let Some(content) = message.get("content") {
                observe_content(&mut observed, content, source, true, 3, &mut remaining_work)?;
            }
        }
    }
    Ok(observed)
}

fn visit_node(depth: usize, remaining_work: &mut usize) -> Result<(), ContractError> {
    if depth > MAX_DEPTH {
        return Err(TokenError::CapacityExceeded.into());
    }
    *remaining_work = remaining_work
        .checked_sub(1)
        .ok_or(TokenError::CapacityExceeded)?;
    Ok(())
}

fn observe_leaf<'native>(
    observed: &mut RequestObservation<'native>,
    native: &'native Value,
    source: SourceClass,
    constraint: EditConstraint,
    depth: usize,
    remaining_work: &mut usize,
) -> Result<(), ContractError> {
    visit_node(depth, remaining_work)?;
    if native.is_string() {
        observed.observe_text(native, source, constraint)?;
    }
    Ok(())
}

// System and tool-result content accept only strings and text blocks. Tool and
// thinking blocks are supported only in message content, not nested result media.
fn observe_content<'native>(
    observed: &mut RequestObservation<'native>,
    content: &'native Value,
    source: SourceClass,
    message_blocks: bool,
    depth: usize,
    remaining_work: &mut usize,
) -> Result<(), ContractError> {
    visit_node(depth, remaining_work)?;
    match content {
        Value::String(_) => observed.observe_text(content, source, EditConstraint::Editable)?,
        Value::Array(blocks) => {
            for block in blocks {
                visit_node(depth + 1, remaining_work)?;
                match block.get("type").and_then(Value::as_str) {
                    Some("text") => {
                        if let Some(text) = block.get("text") {
                            observe_leaf(
                                observed,
                                text,
                                source,
                                EditConstraint::Editable,
                                depth + 2,
                                remaining_work,
                            )?;
                        }
                    }
                    Some("tool_use") if message_blocks => {
                        if let Some(input) = block.get("input") {
                            observe_tool_input(observed, input, depth + 2, remaining_work)?;
                        }
                    }
                    Some("tool_result") if message_blocks => {
                        if let Some(result) = block.get("content") {
                            observe_content(
                                observed,
                                result,
                                SourceClass::ToolResult,
                                false,
                                depth + 2,
                                remaining_work,
                            )?;
                        }
                    }
                    Some("thinking") if message_blocks => {
                        if let Some(thinking) = block.get("thinking") {
                            // Immutability precedes inspection, even without a signature.
                            observe_leaf(
                                observed,
                                thinking,
                                SourceClass::HistoricalText,
                                EditConstraint::Immutable,
                                depth + 2,
                                remaining_work,
                            )?;
                        }
                    }
                    _ => {}
                }
            }
        }
        _ => {}
    }
    Ok(())
}

fn observe_tool_input<'native>(
    observed: &mut RequestObservation<'native>,
    native: &'native Value,
    depth: usize,
    remaining_work: &mut usize,
) -> Result<(), ContractError> {
    visit_node(depth, remaining_work)?;
    match native {
        Value::String(_) => {
            observed.observe_text(native, SourceClass::ToolArguments, EditConstraint::Editable)?;
        }
        Value::Array(items) => {
            for item in items {
                observe_tool_input(observed, item, depth + 1, remaining_work)?;
            }
        }
        Value::Object(fields) => {
            for value in fields.values() {
                observe_tool_input(observed, value, depth + 1, remaining_work)?;
            }
        }
        _ => {}
    }
    Ok(())
}
