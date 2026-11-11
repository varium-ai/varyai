//! Borrowed Chat response observations; completion and release remain shared owners.

use serde_json::Value;

use crate::proxy::{
    protocol_bridge::Protocol,
    security::{
        completed_context::ActualAttemptBinding,
        completion_json::JsonCompletion,
        payload_budget::PayloadBudget,
        payload_metadata::PayloadVec,
        response_restoration::{serialize_json_with_budget, RestoreError},
        restoration_contract::{
            ClientProtocol, EditConstraint, ProtocolPair, SourceClass, TextPolicy, TextSites,
            UpstreamProtocol,
        },
        restoration_core::{
            stage_native_json_with_lifecycle, AttemptProvenance, NativeJsonObservation,
            StagedNativeJson,
        },
        tokenization::DlpRequestLease,
    },
};

fn client() -> ClientProtocol {
    ClientProtocol::new(Protocol::Chat)
}

fn observe_text<'native>(
    node: &'native Value,
    policy: TextPolicy,
    sites: &mut TextSites<'native>,
    policies: &mut PayloadVec<TextPolicy>,
) -> Result<(), RestoreError> {
    if node.is_string() {
        sites
            .observe(
                node,
                if policy == TextPolicy::ToolArguments {
                    SourceClass::ToolArguments
                } else {
                    SourceClass::HistoricalText
                },
                EditConstraint::Editable,
            )
            .map_err(|_| RestoreError::RestoreFailed)?;
        policies.push(policy).map_err(RestoreError::capacity)?;
    }
    Ok(())
}

/// Select only supported native carriers; equal opaque siblings are never sites.
fn observe_message<'native>(
    message: &'native Value,
    sites: &mut TextSites<'native>,
    policies: &mut PayloadVec<TextPolicy>,
) -> Result<(), RestoreError> {
    if let Some(content) = message.get("content") {
        if let Some(parts) = content.as_array() {
            for part in parts {
                let (field, policy) = match part.get("type").and_then(Value::as_str) {
                    Some("text" | "output_text") => ("text", TextPolicy::Output),
                    Some("refusal") => ("refusal", TextPolicy::Refusal),
                    _ => continue,
                };
                if let Some(text) = part.get(field) {
                    observe_text(text, policy, sites, policies)?;
                }
            }
        } else {
            observe_text(content, TextPolicy::Output, sites, policies)?;
        }
    }
    for (field, policy) in [
        ("refusal", TextPolicy::Refusal),
        ("reasoning", TextPolicy::Reasoning),
        ("reasoning_content", TextPolicy::Reasoning),
    ] {
        if let Some(text) = message.get(field) {
            observe_text(text, policy, sites, policies)?;
        }
    }
    if let Some(calls) = message.get("tool_calls").and_then(Value::as_array) {
        for call in calls {
            if let Some(arguments) = call.get("function").and_then(|f| f.get("arguments")) {
                observe_text(arguments, TextPolicy::ToolArguments, sites, policies)?;
            }
        }
    }
    Ok(())
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
    // Reuse native strict validation without granting transport or release authority.
    let encoded = serialize_json_with_budget(source, &budget, 1024 * 1024)?;
    let mut completion = JsonCompletion::native_chat();
    completion.observe_raw_json(
        std::str::from_utf8(encoded.as_slice()).map_err(|_| RestoreError::RestoreFailed)?,
        &budget,
    );
    if completion.has_unsupported_carrier() {
        return Err(RestoreError::RestoreUnsupported);
    }
    if !completion.permits_completion() {
        return Err(RestoreError::RestoreFailed);
    }
    drop(encoded);
    let choices = source
        .get("choices")
        .and_then(Value::as_array)
        .ok_or(RestoreError::RestoreFailed)?;
    let mut sites = TextSites::new(budget.clone()).map_err(|_| RestoreError::RestoreFailed)?;
    let mut policies = PayloadVec::new(budget.clone()).map_err(RestoreError::capacity)?;
    let mut delivered = PayloadVec::new(budget.clone()).map_err(RestoreError::capacity)?;
    for choice in choices {
        let message = choice.get("message").ok_or(RestoreError::RestoreFailed)?;
        observe_message(message, &mut sites, &mut policies)?;
        delivered.push(message).map_err(RestoreError::capacity)?;
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
        ProtocolPair::new(client(), UpstreamProtocol::new(Protocol::Chat)),
        attempt,
    )
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SseCarrier {
    Text(&'static str),
    Arguments(u64),
}

struct SseFragments<'native> {
    choice: u64,
    carrier: SseCarrier,
    policy: TextPolicy,
    nodes: PayloadVec<&'native Value>,
}

fn observe_fragment<'native>(
    choice: u64,
    carrier: SseCarrier,
    policy: TextPolicy,
    node: &'native Value,
    sites: &mut TextSites<'native>,
    groups: &mut PayloadVec<SseFragments<'native>>,
    budget: &PayloadBudget,
) -> Result<(), RestoreError> {
    if !node.is_string() {
        return Ok(());
    }
    sites
        .observe(
            node,
            if policy == TextPolicy::ToolArguments {
                SourceClass::ToolArguments
            } else {
                SourceClass::HistoricalText
            },
            EditConstraint::Editable,
        )
        .map_err(|_| RestoreError::RestoreFailed)?;
    let index = match groups
        .as_slice()
        .iter()
        .position(|group| group.choice == choice && group.carrier == carrier)
    {
        Some(index) => index,
        None => {
            groups
                .push(SseFragments {
                    choice,
                    carrier,
                    policy,
                    nodes: PayloadVec::new(budget.clone()).map_err(RestoreError::capacity)?,
                })
                .map_err(RestoreError::capacity)?;
            groups.len() - 1
        }
    };
    groups
        .get_mut(index)
        .ok_or(RestoreError::RestoreFailed)?
        .nodes
        .push(node)
        .map_err(RestoreError::capacity)
}

/// Native SSE consumer boundary; no completion or publication capability is exposed.
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
        tokenization::TokenError,
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
        let encoded =
            std::str::from_utf8(data.as_slice()).map_err(|_| RestoreError::RestoreFailed)?;
        let source = if !has_data || encoded.trim() == "[DONE]" {
            None
        } else {
            Some(
                PayloadJson::try_from_str(encoded, budget.clone(), budget.limit())
                    .map_err(RestoreError::capacity)?,
            )
        };
        parsed.push((raw, source)).map_err(RestoreError::capacity)?;
    }
    let mut frames = PayloadVec::new(budget.clone()).map_err(RestoreError::capacity)?;
    let mut snapshots = PayloadVec::new(budget.clone()).map_err(RestoreError::capacity)?;
    let mut groups = PayloadVec::new(budget.clone()).map_err(RestoreError::capacity)?;
    let mut site_count = 0usize;
    for (frame_index, (raw, source)) in parsed.as_slice().iter().enumerate() {
        let mut sites = TextSites::new(budget.clone()).map_err(|_| RestoreError::RestoreFailed)?;
        if let Some(source) = source {
            let choices = source
                .as_value()
                .get("choices")
                .and_then(Value::as_array)
                .filter(|choices| choices.len() <= 128)
                .ok_or(RestoreError::RestoreFailed)?;
            for choice in choices {
                if snapshots.len() >= 128 {
                    return Err(RestoreError::capacity(TokenError::CapacityExceeded));
                }
                snapshots
                    .push(NativeSseSnapshotSelection::chat_choice(frame_index, choice))
                    .map_err(RestoreError::capacity)?;
                let index = choice
                    .get("index")
                    .and_then(Value::as_u64)
                    .filter(|index| *index < 128)
                    .ok_or(RestoreError::RestoreFailed)?;
                let delta = choice
                    .get("delta")
                    .and_then(Value::as_object)
                    .ok_or(RestoreError::RestoreFailed)?;
                for (field, policy) in [
                    ("content", TextPolicy::Output),
                    ("refusal", TextPolicy::Refusal),
                    ("reasoning", TextPolicy::Reasoning),
                    ("reasoning_content", TextPolicy::Reasoning),
                ] {
                    if let Some(node) = delta.get(field) {
                        observe_fragment(
                            index,
                            SseCarrier::Text(field),
                            policy,
                            node,
                            &mut sites,
                            &mut groups,
                            &budget,
                        )?;
                    }
                }
                if let Some(calls) = delta.get("tool_calls").filter(|calls| !calls.is_null()) {
                    let calls = calls
                        .as_array()
                        .filter(|calls| calls.len() <= 128)
                        .ok_or(RestoreError::RestoreFailed)?;
                    for call in calls {
                        let call_index = call
                            .get("index")
                            .and_then(Value::as_u64)
                            .filter(|index| *index < 128)
                            .ok_or(RestoreError::RestoreFailed)?;
                        if let Some(node) = call
                            .get("function")
                            .and_then(|function| function.get("arguments"))
                        {
                            observe_fragment(
                                index,
                                SseCarrier::Arguments(call_index),
                                TextPolicy::ToolArguments,
                                node,
                                &mut sites,
                                &mut groups,
                                &budget,
                            )?;
                        }
                    }
                }
            }
        }
        site_count = site_count
            .checked_add(sites.len())
            .filter(|count| *count <= 128)
            .ok_or_else(|| RestoreError::capacity(TokenError::CapacityExceeded))?;
        frames
            .push(NativeSseFrameObservation::new(
                raw,
                source.as_ref().map(PayloadJson::as_value),
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
    for group in groups.as_slice() {
        streams
            .push(NativeSseTextStream::with_policy(
                group.nodes.as_slice(),
                group.policy,
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
        ProtocolPair::new(client(), UpstreamProtocol::new(Protocol::Chat)),
        attempt,
    )?;
    finalize_native_sse(stage, lease, proof, attempt, max)
}
