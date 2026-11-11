//! Borrowed Gemini response observations; completion and release remain shared owners.

use serde_json::Value;

use crate::proxy::{
    protocol_bridge::Protocol,
    security::{
        completed_context::ActualAttemptBinding,
        payload_budget::PayloadBudget,
        payload_metadata::PayloadVec,
        response_restoration::RestoreError,
        restoration_contract::{
            ClientProtocol, EditConstraint, ProtocolPair, SourceClass, TextPolicy, TextSites,
            UpstreamProtocol,
        },
        restoration_core::{
            stage_native_json_with_lifecycle, AttemptProvenance, NativeJsonObservation,
            StagedNativeJson,
        },
        tokenization::{DlpRequestLease, TokenError},
    },
};

fn client() -> ClientProtocol {
    ClientProtocol::new(Protocol::Gemini)
}

/// Inspection/staging only: strict HTTP proof and sealed finalization remain mandatory.
pub(in crate::proxy::security) fn stage_json<'native>(
    source: &'native Value,
    lease: &DlpRequestLease,
    binding: &ActualAttemptBinding,
    budget: PayloadBudget,
    attempt: &AttemptProvenance,
) -> Result<StagedNativeJson<'native>, RestoreError> {
    lease
        .require_native_attempt_binding(client(), binding)
        .map_err(|_| RestoreError::RestoreFailed)?;
    let object = source.as_object().ok_or(RestoreError::RestoreFailed)?;
    if object.get("error").is_some_and(|error| !error.is_null())
        || source
            .pointer("/promptFeedback/blockReason")
            .is_some_and(|reason| !reason.is_null())
    {
        return Err(RestoreError::RestoreFailed);
    }

    let candidates = source
        .get("candidates")
        .and_then(Value::as_array)
        .ok_or(RestoreError::RestoreFailed)?;
    let mut sites = TextSites::new(budget.clone()).map_err(|_| RestoreError::RestoreFailed)?;
    let mut policies = PayloadVec::new(budget.clone()).map_err(RestoreError::capacity)?;
    let mut delivered = PayloadVec::new(budget.clone()).map_err(RestoreError::capacity)?;
    for (position, candidate) in candidates.iter().enumerate() {
        let index = candidate
            .get("index")
            .and_then(Value::as_u64)
            .filter(|index| *index < 128)
            .ok_or(RestoreError::RestoreFailed)?;
        if candidates[..position]
            .iter()
            .any(|prior| prior.get("index").and_then(Value::as_u64) == Some(index))
        {
            return Err(RestoreError::RestoreFailed);
        }
        if candidate.get("finishReason").and_then(Value::as_str) != Some("STOP") {
            return Err(RestoreError::RestoreFailed);
        }
        let content = candidate
            .get("content")
            .ok_or(RestoreError::RestoreFailed)?;
        if content.get("role").and_then(Value::as_str) != Some("model") {
            return Err(RestoreError::RestoreFailed);
        }
        let parts = content
            .get("parts")
            .and_then(Value::as_array)
            .ok_or(RestoreError::RestoreFailed)?;
        let mut remaining_work = 4 * 1024 * 1024;
        for part in parts {
            if !part.is_object()
                || part.get("text").is_some_and(|text| !text.is_string())
                || part.get("functionCall").is_some_and(|call| {
                    !call.is_object() || !call.get("args").is_some_and(Value::is_object)
                })
            {
                return Err(RestoreError::RestoreFailed);
            }
            let constraint = if part.get("thoughtSignature").is_some()
                || part.get("thought_signature").is_some()
            {
                EditConstraint::Immutable
            } else {
                EditConstraint::Editable
            };
            if let Some(text) = part.get("text").filter(|text| text.is_string()) {
                sites
                    .observe(text, SourceClass::HistoricalText, constraint)
                    .map_err(|_| RestoreError::RestoreFailed)?;
                policies
                    .push(TextPolicy::Output)
                    .map_err(RestoreError::capacity)?;
            }
            if let Some(args) = part
                .get("functionCall")
                .and_then(|call| call.get("args"))
                .filter(|args| args.is_object())
            {
                observe_argument_leaves(
                    args,
                    0,
                    &mut remaining_work,
                    constraint,
                    &mut sites,
                    &mut policies,
                )?;
            }
        }
        delivered.push(content).map_err(RestoreError::capacity)?;
    }
    let observation = NativeJsonObservation::new_with_policies(
        source,
        client(),
        sites,
        delivered.as_slice(),
        policies.as_slice(),
        budget,
    )?;
    stage_native_json_with_lifecycle(
        &observation,
        lease,
        binding,
        ProtocolPair::new(client(), UpstreamProtocol::new(Protocol::Gemini)),
        attempt,
    )
}

fn observe_argument_leaves<'native>(
    value: &'native Value,
    depth: usize,
    remaining_work: &mut usize,
    constraint: EditConstraint,
    sites: &mut TextSites<'native>,
    policies: &mut PayloadVec<TextPolicy>,
) -> Result<(), RestoreError> {
    if depth > 64 {
        return Err(RestoreError::capacity(TokenError::CapacityExceeded));
    }
    *remaining_work = remaining_work
        .checked_sub(1)
        .ok_or_else(|| RestoreError::capacity(TokenError::CapacityExceeded))?;
    match value {
        Value::String(_) => {
            sites
                .observe(value, SourceClass::ToolArguments, constraint)
                .map_err(|_| RestoreError::RestoreFailed)?;
            policies
                .push(TextPolicy::ToolArguments)
                .map_err(RestoreError::capacity)?;
        }
        Value::Array(values) => {
            for value in values {
                observe_argument_leaves(
                    value,
                    depth + 1,
                    remaining_work,
                    constraint,
                    sites,
                    policies,
                )?;
            }
        }
        Value::Object(fields) => {
            for value in fields.values() {
                observe_argument_leaves(
                    value,
                    depth + 1,
                    remaining_work,
                    constraint,
                    sites,
                    policies,
                )?;
            }
        }
        _ => {}
    }
    Ok(())
}

/// Complete Content/Part observations only; EOF proof and publication stay shared.
#[allow(clippy::too_many_arguments)]
pub(in crate::proxy::security) fn finalize_sse(
    entity: &[u8],
    lease: &DlpRequestLease,
    binding: &ActualAttemptBinding,
    budget: PayloadBudget,
    attempt: &AttemptProvenance,
    proof: crate::proxy::security::http_restoration::VerifiedNativeSse,
    max: usize,
) -> Result<crate::proxy::security::response_restoration::FinalizedNativeSse, RestoreError> {
    use crate::proxy::security::{
        payload_buffer::PayloadBuffer,
        payload_json::PayloadJson,
        response_restoration::finalize_native_sse,
        restoration_core::{
            stage_native_sse_with_lifecycle, NativeSseFrameObservation, NativeSseObservation,
            NativeSseSnapshotSelection, NativeSseTextStream,
        },
    };
    lease
        .require_native_attempt_binding(client(), binding)
        .map_err(|_| RestoreError::RestoreFailed)?;
    if entity.len() > max.min(1024 * 1024) {
        return Err(RestoreError::capacity(TokenError::CapacityExceeded));
    }
    let mut remaining = std::str::from_utf8(entity).map_err(|_| RestoreError::RestoreFailed)?;
    let mut parsed = PayloadVec::new(budget.clone()).map_err(RestoreError::capacity)?;
    while !remaining.is_empty() {
        if parsed.len() >= 1024 {
            return Err(RestoreError::capacity(TokenError::CapacityExceeded));
        }
        let end = remaining
            .bytes()
            .enumerate()
            .find_map(|(offset, byte)| {
                let tail = &remaining.as_bytes()[offset..];
                if byte == b'\n' && tail.starts_with(b"\n\n") {
                    Some(offset + 2)
                } else if byte == b'\r' && tail.starts_with(b"\r\n\r\n") {
                    Some(offset + 4)
                } else {
                    None
                }
            })
            .ok_or(RestoreError::RestoreFailed)?;
        let (raw, tail) = remaining.split_at(end);
        remaining = tail;
        let mut data = PayloadBuffer::new(budget.clone(), max).map_err(RestoreError::capacity)?;
        let mut has_data = false;
        for line in raw.lines() {
            if let Some(value) = line
                .strip_prefix("data:")
                .or_else(|| (line == "data").then_some(""))
            {
                if has_data {
                    data.append(b"\n").map_err(RestoreError::capacity)?;
                }
                has_data = true;
                data.append(value.strip_prefix(' ').unwrap_or(value).as_bytes())
                    .map_err(RestoreError::capacity)?;
            }
        }
        if !has_data {
            return Err(RestoreError::RestoreUnsupported);
        }
        let encoded =
            std::str::from_utf8(data.as_slice()).map_err(|_| RestoreError::RestoreFailed)?;
        let source = PayloadJson::try_from_str(encoded, budget.clone(), budget.limit())
            .map_err(RestoreError::capacity)?;
        parsed.push((raw, source)).map_err(RestoreError::capacity)?;
    }
    let mut frames = PayloadVec::new(budget.clone()).map_err(RestoreError::capacity)?;
    let mut snapshots = PayloadVec::new(budget.clone()).map_err(RestoreError::capacity)?;
    let mut nodes = PayloadVec::new(budget.clone()).map_err(RestoreError::capacity)?;
    let mut remaining_work = 4 * 1024 * 1024;
    for (frame_index, (raw, source)) in parsed.as_slice().iter().enumerate() {
        let source = source.as_value();
        let mut sites = TextSites::new(budget.clone()).map_err(|_| RestoreError::RestoreFailed)?;
        let mut policies = PayloadVec::new(budget.clone()).map_err(RestoreError::capacity)?;
        if let Some(candidates) = source.get("candidates") {
            let candidates = candidates
                .as_array()
                .filter(|items| !items.is_empty() && items.len() <= 128)
                .ok_or(RestoreError::RestoreFailed)?;
            for candidate in candidates {
                snapshots
                    .push(NativeSseSnapshotSelection::new(frame_index, candidate))
                    .map_err(RestoreError::capacity)?;
                if let Some(content) = candidate.get("content") {
                    let mut unsupported = false;
                    crate::proxy::security::completion_json::validate_gemini_content(
                        content,
                        &mut unsupported,
                    )
                    .map_err(|_| {
                        if unsupported {
                            RestoreError::RestoreUnsupported
                        } else {
                            RestoreError::RestoreFailed
                        }
                    })?;
                    let parts = content
                        .get("parts")
                        .and_then(Value::as_array)
                        .ok_or(RestoreError::RestoreFailed)?;
                    for part in parts {
                        let constraint = if part.get("thoughtSignature").is_some()
                            || part.get("thought_signature").is_some()
                        {
                            EditConstraint::Immutable
                        } else {
                            EditConstraint::Editable
                        };
                        if let Some(text) = part.get("text") {
                            sites
                                .observe(text, SourceClass::HistoricalText, constraint)
                                .map_err(|_| RestoreError::RestoreFailed)?;
                            policies
                                .push(
                                    if part.get("thought").and_then(Value::as_bool) == Some(true) {
                                        TextPolicy::Reasoning
                                    } else {
                                        TextPolicy::Output
                                    },
                                )
                                .map_err(RestoreError::capacity)?;
                        }
                        if let Some(args) = part.pointer("/functionCall/args") {
                            observe_argument_leaves(
                                args,
                                0,
                                &mut remaining_work,
                                constraint,
                                &mut sites,
                                &mut policies,
                            )?;
                        }
                    }
                }
            }
        }
        for (index, policy) in policies.as_slice().iter().enumerate() {
            if nodes.len() >= 128 {
                return Err(RestoreError::capacity(TokenError::CapacityExceeded));
            }
            let site = sites.site(index).ok_or(RestoreError::RestoreFailed)?;
            nodes
                .push((site.native(), *policy))
                .map_err(RestoreError::capacity)?;
        }
        frames
            .push(NativeSseFrameObservation::new(
                raw,
                Some(source),
                sites,
                budget.clone(),
            )?)
            .map_err(RestoreError::capacity)?;
    }
    let mut ordered = PayloadVec::new(budget.clone()).map_err(RestoreError::capacity)?;
    for frame in frames.as_slice() {
        ordered.push(frame).map_err(RestoreError::capacity)?;
    }
    let observation = NativeSseObservation::new(
        entity,
        ordered.as_slice(),
        client(),
        lease,
        binding,
        budget.clone(),
    )?;
    let mut streams = PayloadVec::new(budget.clone()).map_err(RestoreError::capacity)?;
    // Each complete native Part/leaf has its own observation-local carrier. Never
    // concatenate repeated array positions or retroactively attach a signature.
    for (node, policy) in nodes.as_slice() {
        streams
            .push(NativeSseTextStream::with_policy(
                std::slice::from_ref(node),
                *policy,
            ))
            .map_err(RestoreError::capacity)?;
    }
    let stage = stage_native_sse_with_lifecycle(
        &observation,
        snapshots.as_slice(),
        streams.as_slice(),
        lease,
        binding,
        budget,
        ProtocolPair::new(client(), UpstreamProtocol::new(Protocol::Gemini)),
        attempt,
    )?;
    finalize_native_sse(stage, lease, proof, attempt, max)
}
