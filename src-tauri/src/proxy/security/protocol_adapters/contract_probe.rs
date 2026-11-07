//! Compile-only contract body, compiled separately inside each disabled lane.
//! Authority is supplied by callers; these functions are never executed by tests.
#![allow(dead_code)]

use serde_json::Value;

use crate::proxy::security::{
    completed_context::ActualAttemptBinding,
    payload_budget::PayloadBudget,
    payload_buffer::PayloadBuffer,
    request_preparation::prepare_observed_request,
    restoration_contract::{
        ClientProtocol, EditConstraint, Framing, OriginalResponseObservation, ProtocolPair,
        RequestObservation, SourceClass, TextPatch, TextPolicy, UpstreamProtocol,
    },
    restoration_core::{AttemptProvenance, LifecycleLimits, ResponseLifecycle},
    runtime::SecurityRuntimeConfig,
    tokenization::DlpRequestLease,
};

enum ProbeError {
    Contract(crate::proxy::security::restoration_contract::ContractError),
    Plan(crate::proxy::security::request_plan::PlanError),
    Restore(crate::proxy::security::response_restoration::RestoreError),
    Token(crate::proxy::security::tokenization::TokenError),
    MissingSite,
}

macro_rules! probe_error_from {
    ($source:path, $variant:ident) => {
        impl From<$source> for ProbeError {
            fn from(error: $source) -> Self {
                Self::$variant(error)
            }
        }
    };
}
probe_error_from!(
    crate::proxy::security::restoration_contract::ContractError,
    Contract
);
probe_error_from!(crate::proxy::security::request_plan::PlanError, Plan);
probe_error_from!(
    crate::proxy::security::response_restoration::RestoreError,
    Restore
);
probe_error_from!(crate::proxy::security::tokenization::TokenError, Token);

type ProbeResult = Result<(), ProbeError>;

fn request_contract(
    lease: &mut DlpRequestLease,
    native: &Value,
    leaf: &Value,
    budget: PayloadBudget,
    runtime: &SecurityRuntimeConfig,
) -> ProbeResult {
    let client = ClientProtocol::new(super::CONTRACT_PROTOCOL);
    let mut observation = RequestObservation::new(native, client, budget)?;
    observation.observe_text(leaf, SourceClass::CurrentInput, EditConstraint::Editable)?;
    let site = observation.sites().site(0).ok_or(ProbeError::MissingSite)?;
    let patch = TextPatch::new(site, 0..0, "");
    let plan = observation.sites().plan(&[patch])?;
    for edit in plan.edits() {
        let _ = (edit.native(), edit.text());
    }
    let prepared =
        prepare_observed_request(lease, &observation, runtime, &[], &[], |_, _, _| Ok(()))?;
    drop(prepared);
    Ok(())
}

struct LifecycleInputs<'native> {
    lease: DlpRequestLease,
    binding: ActualAttemptBinding,
    budget: PayloadBudget,
    pair: ProtocolPair,
    attempt: AttemptProvenance,
    limits: LifecycleLimits,
    original_json: &'native Value,
    original_frame: &'native [u8],
}

fn finish_contract(input: LifecycleInputs<'_>) -> ProbeResult {
    let mut lifecycle = ResponseLifecycle::begin(
        input.lease,
        input.binding,
        input.budget.clone(),
        input.pair,
        input.attempt.clone(),
        input.limits,
    )?;
    let mut original = OriginalResponseObservation::json(
        input.original_json,
        input.pair.upstream(),
        input.attempt.clone(),
        input.budget.clone(),
    )?;
    original.claim_node(input.original_json)?;
    lifecycle.observe_original(&original)?;
    drop(original);
    let frame = OriginalResponseObservation::frame(
        input.original_frame,
        input.pair.upstream(),
        input.attempt,
        input.budget.clone(),
    )?;
    lifecycle.observe_original(&frame)?;
    drop(frame);
    let text = lifecycle.open_text(TextPolicy::Output)?;
    drop(lifecycle.delta(&text, "")?);
    drop(lifecycle.done(&text, "")?);
    drop(lifecycle.arguments("{}")?);
    lifecycle.hold(PayloadBuffer::new(
        input.budget,
        input.limits.max_held_bytes,
    )?)?;
    // Inspection ownership is not completion, publication or release authority.
    drop(lifecycle.finish()?);
    Ok(())
}

fn abort_contract(input: LifecycleInputs<'_>) -> ProbeResult {
    ResponseLifecycle::begin(
        input.lease,
        input.binding,
        input.budget,
        input.pair,
        input.attempt,
        input.limits,
    )?
    .abort()?;
    Ok(())
}

#[test]
fn disabled_lane_contract_compiles_without_registration() {
    // Referencing the functions does not run them or construct their authority inputs.
    let _ = request_contract;
    let _ = finish_contract;
    let _ = abort_contract;
    let client = ClientProtocol::new(super::CONTRACT_PROTOCOL);
    let pair = ProtocolPair::new(client, UpstreamProtocol::new(super::CONTRACT_PROTOCOL));
    for framing in [Framing::Json, Framing::Sse] {
        assert!(matches!(
            crate::proxy::security::protocol_adapters::registration(
                pair,
                super::CONTRACT_OPERATION,
                framing,
            ),
            crate::proxy::security::protocol_adapters::Registration::Disabled
        ));
    }
}
