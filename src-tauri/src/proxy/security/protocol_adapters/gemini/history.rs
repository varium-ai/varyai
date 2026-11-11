//! Exact Gemini Content replay; native correlation never trusts result bodies.
use http::{Method, Uri};
use serde_json::Value;

use crate::proxy::{
    protocol_bridge::Protocol,
    security::{
        completed_context::ActualAttemptBinding,
        payload_budget::PayloadBudget,
        request_plan::PlanError,
        restoration_contract::{ClientProtocol, ContractError, HistoryClaim, RequestObservation},
        tokenization::{DlpRequestLease, TokenError},
    },
};

/// Consume only the shared exact delivered lookup and import authority.
pub(in crate::proxy::security) fn import_delivered(
    lease: &mut DlpRequestLease,
    binding: &ActualAttemptBinding,
    delivered: &Value,
) -> Result<(), TokenError> {
    let client = ClientProtocol::new(Protocol::Gemini);
    let handle = lease.verify_native_delivered_extracted(binding, client, delivered)?;
    lease.import_native_completed_context_extracted(binding, client, &handle)
}

fn charge_work(remaining: &mut usize, count: usize) -> Result<(), PlanError> {
    *remaining = remaining
        .checked_sub(count)
        .ok_or(TokenError::CapacityExceeded)?;
    Ok(())
}

fn contract_error(error: ContractError) -> PlanError {
    match error {
        ContractError::Resource(error) => PlanError::Token(error),
        ContractError::ImmutableSite => PlanError::ImmutableContent,
        _ => PlanError::Unsupported,
    }
}

// Match the frozen shared Gemini label contract without widening visibility.
fn valid_native_label(label: &str, remaining: &mut usize) -> Result<bool, PlanError> {
    if label.is_empty() || label.len() > 256 {
        return Ok(false);
    }
    charge_work(remaining, label.len())?;
    Ok(!label.chars().any(|c| c.is_whitespace() || c.is_control()))
}

fn function<'a>(
    part: &'a Value,
    carrier: &str,
    field: &str,
    remaining: &mut usize,
) -> Result<Option<&'a Value>, PlanError> {
    // A mixed call/result Part has no unambiguous structural identity.
    if part.get("functionCall").is_some() && part.get("functionResponse").is_some() {
        return Ok(None);
    }
    let Some(native) = part.get(carrier) else {
        return Ok(None);
    };
    let Some(name) = native.get("name").and_then(Value::as_str) else {
        return Ok(None);
    };
    if !native.get(field).is_some_and(Value::is_object) || !valid_native_label(name, remaining)? {
        return Ok(None);
    }
    Ok(Some(native))
}

/// Adjacent model/user Contents only. IDs win; ID-free fallback is single-pair
/// only, never positional parallel matching, a generated ID or a history cursor.
pub(in crate::proxy::security) fn observe_request<'a>(
    method: &Method,
    uri: &Uri,
    native: &'a Value,
    budget: PayloadBudget,
) -> Result<RequestObservation<'a>, PlanError> {
    super::request::classify_generation(method, uri)?;
    observe_admitted_request(native, budget)
}

/// Correlate native calls/results only after the handler admitted a supported route.
pub(in crate::proxy::security) fn observe_admitted_request<'a>(
    native: &'a Value,
    budget: PayloadBudget,
) -> Result<RequestObservation<'a>, PlanError> {
    let mut observation = super::request::observe_admitted_request(native, budget)?;
    let Some(contents) = native.get("contents").and_then(Value::as_array) else {
        return Ok(observation);
    };
    let mut remaining = 4 * 1024 * 1024;
    for pair in contents.windows(2) {
        charge_work(&mut remaining, 1)?;
        if pair[0].get("role").and_then(Value::as_str) != Some("model")
            || pair[1].get("role").and_then(Value::as_str) != Some("user")
        {
            continue;
        }
        let (Some(calls), Some(results)) = (
            pair[0].get("parts").and_then(Value::as_array),
            pair[1].get("parts").and_then(Value::as_array),
        ) else {
            continue;
        };
        let mut call_count = 0;
        let mut result_count = 0;
        for part in calls {
            charge_work(&mut remaining, 1)?;
            if part.get("functionCall").is_some() {
                call_count += 1;
            }
        }
        for part in results {
            charge_work(&mut remaining, 1)?;
            if part.get("functionResponse").is_some() {
                result_count += 1;
            }
        }
        for result in results {
            charge_work(&mut remaining, 1)?;
            let Some(response) = function(result, "functionResponse", "response", &mut remaining)?
            else {
                continue;
            };
            let name = response["name"].as_str().ok_or(PlanError::Unsupported)?;
            let selected = if response.get("id").is_some() {
                let Some(id) = response.get("id").and_then(Value::as_str) else {
                    continue;
                };
                if !valid_native_label(id, &mut remaining)? {
                    continue;
                }
                let mut matching_calls = 0;
                let mut selected = None;
                for call in calls {
                    charge_work(&mut remaining, 1 + id.len() + name.len())?;
                    if call
                        .get("functionCall")
                        .and_then(|call| call.get("id"))
                        .and_then(Value::as_str)
                        == Some(id)
                    {
                        matching_calls += 1;
                        if function(call, "functionCall", "args", &mut remaining)?.is_some_and(
                            |call| call.get("name").and_then(Value::as_str) == Some(name),
                        ) {
                            selected = Some(call);
                        }
                    }
                }
                if matching_calls != 1 {
                    continue;
                }
                let mut matching_results = 0;
                for candidate in results {
                    charge_work(&mut remaining, 1 + id.len())?;
                    if candidate
                        .get("functionResponse")
                        .and_then(|result| result.get("id"))
                        .and_then(Value::as_str)
                        == Some(id)
                    {
                        matching_results += 1;
                    }
                }
                if matching_results != 1 {
                    continue;
                }
                selected
            } else if call_count == 1 && result_count == 1 {
                let mut selected = None;
                for call in calls {
                    charge_work(&mut remaining, 1 + name.len())?;
                    if function(call, "functionCall", "args", &mut remaining)?.is_some_and(|call| {
                        call.get("id").is_none()
                            && call.get("name").and_then(Value::as_str) == Some(name)
                    }) {
                        selected = Some(call);
                    }
                }
                selected
            } else {
                None
            };
            if let Some(call) = selected {
                observation
                    .claim_history(HistoryClaim::CallResultPair { call, result })
                    .map_err(contract_error)?;
            }
        }
    }
    Ok(observation)
}
