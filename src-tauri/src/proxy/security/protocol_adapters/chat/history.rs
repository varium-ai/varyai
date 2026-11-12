//! Exact Chat replay consumer; client tool results never become delivered authority.

use crate::proxy::{
    protocol_bridge::Protocol,
    security::{
        completed_context::ActualAttemptBinding,
        payload_budget::PayloadBudget,
        restoration_contract::{ClientProtocol, ContractError, HistoryClaim, RequestObservation},
        tokenization::{DlpRequestLease, TokenError},
    },
};
use serde_json::Value;

/// Strict lookup/import only. Optional degradation belongs to HTTP orchestration.
/// The caller supplies the complete ordered assistant evidence, never result bodies.
pub(in crate::proxy::security) fn import_delivered(
    lease: &mut DlpRequestLease,
    binding: &ActualAttemptBinding,
    delivered: &Value,
) -> Result<(), TokenError> {
    let client = ClientProtocol::new(Protocol::Chat);
    let handle = lease.verify_native_delivered_extracted(binding, client, delivered)?;
    lease.import_native_completed_context_extracted(binding, client, &handle)
}

fn charge_work(remaining: &mut usize, count: usize) -> Result<(), ContractError> {
    *remaining = remaining
        .checked_sub(count)
        .ok_or(TokenError::CapacityExceeded)?;
    Ok(())
}

/// Add exact structural pairs to CH1 observations without trusting any result text.
/// Only the immediately preceding assistant/tool group is eligible; ambiguous or
/// duplicate IDs and already-consumed results remain ordinary unpaired input.
pub(in crate::proxy::security) fn observe_request(
    native: &Value,
    budget: PayloadBudget,
) -> Result<RequestObservation<'_>, ContractError> {
    let mut observation = super::request::observe_request(native, budget)?;
    let Some(messages) = native.get("messages").and_then(Value::as_array) else {
        return Ok(observation);
    };
    let mut remaining = 4 * 1024 * 1024;
    let mut assistant = None;
    for (index, message) in messages.iter().enumerate() {
        charge_work(&mut remaining, 1)?;
        match message.get("role").and_then(Value::as_str) {
            Some("assistant") => {
                assistant = Some(index);
                continue;
            }
            Some("tool") => {}
            _ => {
                assistant = None;
                continue;
            }
        }
        let Some(assistant_index) = assistant else {
            continue;
        };
        let Some(id) = message
            .get("tool_call_id")
            .and_then(Value::as_str)
            .filter(|id| !id.is_empty())
        else {
            continue;
        };
        let Some(calls) = messages[assistant_index]
            .get("tool_calls")
            .and_then(Value::as_array)
        else {
            continue;
        };
        let mut selected = None;
        let mut matches = 0usize;
        for call in calls {
            charge_work(&mut remaining, 1 + id.len())?;
            if call.get("id").and_then(Value::as_str) == Some(id) {
                matches += 1;
                if call.get("type").and_then(Value::as_str) == Some("function")
                    && call
                        .get("function")
                        .and_then(|function| function.get("name"))
                        .and_then(Value::as_str)
                        .is_some_and(|name| !name.is_empty())
                    && call
                        .get("function")
                        .and_then(|function| function.get("arguments"))
                        .is_some_and(Value::is_string)
                {
                    selected = Some(call);
                }
            }
        }
        if matches != 1 {
            continue;
        }
        let mut consumed = false;
        for prior in &messages[assistant_index + 1..index] {
            charge_work(&mut remaining, 1 + id.len())?;
            if prior.get("tool_call_id").and_then(Value::as_str) == Some(id) {
                consumed = true;
                break;
            }
        }
        if !consumed {
            if let Some(call) = selected {
                observation.claim_history(HistoryClaim::CallResultPair {
                    call,
                    result: message,
                })?;
            }
        }
    }
    Ok(observation)
}
