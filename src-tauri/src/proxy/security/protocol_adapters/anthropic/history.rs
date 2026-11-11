//! Exact Anthropic block replay; tool-result bodies never become delivered authority.
use serde_json::Value;

use crate::proxy::{
    protocol_bridge::Protocol,
    security::{
        completed_context::ActualAttemptBinding,
        payload_budget::PayloadBudget,
        restoration_contract::{ClientProtocol, ContractError, HistoryClaim, RequestObservation},
        tokenization::{DlpRequestLease, TokenError},
    },
};

/// Strict shared lookup/import only; the caller supplies complete delivered blocks.
pub(in crate::proxy::security) fn import_delivered(
    lease: &mut DlpRequestLease,
    binding: &ActualAttemptBinding,
    delivered: &Value,
) -> Result<(), TokenError> {
    let client = ClientProtocol::new(Protocol::Anthropic);
    let handle = lease.verify_native_delivered_extracted(binding, client, delivered)?;
    lease.import_native_completed_context_extracted(binding, client, &handle)
}

fn charge_work(remaining: &mut usize, count: usize) -> Result<(), ContractError> {
    *remaining = remaining
        .checked_sub(count)
        .ok_or(TokenError::CapacityExceeded)?;
    Ok(())
}

// Match the frozen shared Anthropic label contract without widening visibility.
fn valid_native_label(label: &str, remaining: &mut usize) -> Result<bool, ContractError> {
    if label.is_empty() || label.len() > 256 {
        return Ok(false);
    }
    charge_work(remaining, label.len())?;
    Ok(!label.chars().any(|c| c.is_whitespace() || c.is_control()))
}

/// Observe native ID pairs only in an immediately adjacent assistant/user group.
/// Ambiguous/duplicate/missing IDs stay unpaired; result text remains AN1 input.
pub(in crate::proxy::security) fn observe_request(
    native: &Value,
    budget: PayloadBudget,
) -> Result<RequestObservation<'_>, ContractError> {
    let mut observation = super::request::observe_request(native, budget)?;
    let Some(messages) = native.get("messages").and_then(Value::as_array) else {
        return Ok(observation);
    };
    let mut remaining = 4 * 1024 * 1024;
    for pair in messages.windows(2) {
        charge_work(&mut remaining, 1)?;
        if pair[0].get("role").and_then(Value::as_str) != Some("assistant")
            || pair[1].get("role").and_then(Value::as_str) != Some("user")
        {
            continue;
        }
        let (Some(calls), Some(results)) = (
            pair[0].get("content").and_then(Value::as_array),
            pair[1].get("content").and_then(Value::as_array),
        ) else {
            continue;
        };
        for result in results {
            charge_work(&mut remaining, 1)?;
            if result.get("type").and_then(Value::as_str) != Some("tool_result") {
                continue;
            }
            let Some(id) = result.get("tool_use_id").and_then(Value::as_str) else {
                continue;
            };
            if !valid_native_label(id, &mut remaining)? {
                continue;
            }
            let mut selected = None;
            let mut matches = 0usize;
            for call in calls {
                charge_work(&mut remaining, 1 + id.len())?;
                if call.get("type").and_then(Value::as_str) == Some("tool_use")
                    && call.get("id").and_then(Value::as_str) == Some(id)
                {
                    matches += 1;
                    if call.get("input").is_some_and(Value::is_object) {
                        if let Some(name) = call.get("name").and_then(Value::as_str) {
                            if valid_native_label(name, &mut remaining)? {
                                selected = Some(call);
                            }
                        }
                    }
                }
            }
            if matches != 1 {
                continue;
            }
            let mut result_matches = 0usize;
            for candidate in results {
                charge_work(&mut remaining, 1 + id.len())?;
                if candidate.get("type").and_then(Value::as_str) == Some("tool_result")
                    && candidate.get("tool_use_id").and_then(Value::as_str) == Some(id)
                {
                    result_matches += 1;
                }
            }
            if result_matches == 1 {
                if let Some(call) = selected {
                    observation.claim_history(HistoryClaim::CallResultPair { call, result })?;
                }
            }
        }
    }
    Ok(observation)
}
