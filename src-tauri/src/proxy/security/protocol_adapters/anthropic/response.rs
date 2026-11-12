//! Anthropic response observations; completion and release remain shared owners.

use serde_json::Value;

use crate::proxy::{
    protocol_bridge::Protocol,
    security::{
        completed_context::ActualAttemptBinding,
        completion_json::{JsonCompletion, JsonCompletionProtocol},
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
        tokenization::{DlpRequestLease, TokenError},
    },
};

fn client() -> ClientProtocol {
    ClientProtocol::new(Protocol::Anthropic)
}

fn visit_node(depth: usize, remaining_work: &mut usize) -> Result<(), RestoreError> {
    if depth > 64 {
        return Err(RestoreError::capacity(TokenError::CapacityExceeded));
    }
    *remaining_work = remaining_work
        .checked_sub(1)
        .ok_or_else(|| RestoreError::capacity(TokenError::CapacityExceeded))?;
    Ok(())
}

fn observe_tool_input<'native>(
    native: &'native Value,
    sites: &mut TextSites<'native>,
    policies: &mut PayloadVec<TextPolicy>,
    depth: usize,
    remaining_work: &mut usize,
) -> Result<(), RestoreError> {
    visit_node(depth, remaining_work)?;
    match native {
        Value::String(_) => {
            sites
                .observe(native, SourceClass::ToolArguments, EditConstraint::Editable)
                .map_err(RestoreError::contract)?;
            policies
                .push(TextPolicy::ToolArguments)
                .map_err(RestoreError::capacity)?;
        }
        Value::Array(items) => {
            for item in items {
                observe_tool_input(item, sites, policies, depth + 1, remaining_work)?;
            }
        }
        Value::Object(fields) => {
            for value in fields.values() {
                observe_tool_input(value, sites, policies, depth + 1, remaining_work)?;
            }
        }
        _ => {}
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
        .map_err(RestoreError::scan_error)?;
    // Reuse native strict validation without granting transport or release authority.
    let encoded = serialize_json_with_budget(source, &budget, 1024 * 1024)?;
    let mut completion = JsonCompletion::new(JsonCompletionProtocol::AnthropicMessages);
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
    let content = source
        .get("content")
        .and_then(Value::as_array)
        .ok_or(RestoreError::RestoreFailed)?;
    let mut sites = TextSites::new(budget.clone()).map_err(RestoreError::contract)?;
    let mut policies = PayloadVec::new(budget.clone()).map_err(RestoreError::capacity)?;
    let mut remaining_work = 4 * 1024 * 1024;
    for block in content {
        match block.get("type").and_then(Value::as_str) {
            Some("text") => {
                if let Some(text) = block.get("text").filter(|text| text.is_string()) {
                    sites
                        .observe(text, SourceClass::HistoricalText, EditConstraint::Editable)
                        .map_err(RestoreError::contract)?;
                    policies
                        .push(TextPolicy::Output)
                        .map_err(RestoreError::capacity)?;
                }
            }
            Some("thinking") => {
                if let Some(thinking) = block.get("thinking").filter(|text| text.is_string()) {
                    sites
                        .observe(
                            thinking,
                            SourceClass::HistoricalText,
                            EditConstraint::Immutable,
                        )
                        .map_err(RestoreError::contract)?;
                    policies
                        .push(TextPolicy::Reasoning)
                        .map_err(RestoreError::capacity)?;
                }
            }
            Some("tool_use") => {
                if let Some(input) = block.get("input") {
                    observe_tool_input(input, &mut sites, &mut policies, 2, &mut remaining_work)?;
                }
            }
            _ => {}
        }
    }
    let mut delivered = PayloadVec::new(budget.clone()).map_err(RestoreError::capacity)?;
    for block in content {
        delivered.push(block).map_err(RestoreError::capacity)?;
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
        ProtocolPair::new(client(), UpstreamProtocol::new(Protocol::Anthropic)),
        attempt,
    )
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SseCarrier {
    Text,
    Thinking,
    Arguments,
    InputLeaf(u64),
}

impl SseCarrier {
    fn policy(self) -> TextPolicy {
        match self {
            Self::Text => TextPolicy::Output,
            Self::Thinking => TextPolicy::Reasoning,
            Self::Arguments | Self::InputLeaf(_) => TextPolicy::ToolArguments,
        }
    }
}

struct SseGroup<'native> {
    index: u64,
    carrier: SseCarrier,
    nodes: PayloadVec<&'native Value>,
}

fn observe_sse_fragment<'native>(
    index: u64,
    carrier: SseCarrier,
    node: &'native Value,
    sites: &mut TextSites<'native>,
    groups: &mut PayloadVec<SseGroup<'native>>,
    budget: &PayloadBudget,
) -> Result<(), RestoreError> {
    if index >= 128 || !node.is_string() {
        return Err(RestoreError::RestoreFailed);
    }
    let source = if carrier.policy() == TextPolicy::ToolArguments {
        SourceClass::ToolArguments
    } else {
        SourceClass::HistoricalText
    };
    let constraint = if carrier == SseCarrier::Thinking {
        EditConstraint::Immutable
    } else {
        EditConstraint::Editable
    };
    sites
        .observe(node, source, constraint)
        .map_err(RestoreError::contract)?;
    let position = match groups
        .as_slice()
        .iter()
        .position(|group| group.index == index && group.carrier == carrier)
    {
        Some(position) => position,
        None => {
            if groups.len() >= 128 {
                return Err(RestoreError::capacity(TokenError::CapacityExceeded));
            }
            let position = groups.len();
            groups
                .push(SseGroup {
                    index,
                    carrier,
                    nodes: PayloadVec::new(budget.clone()).map_err(RestoreError::capacity)?,
                })
                .map_err(RestoreError::capacity)?;
            position
        }
    };
    groups
        .get_mut(position)
        .ok_or(RestoreError::RestoreFailed)?
        .nodes
        .push(node)
        .map_err(RestoreError::capacity)
}

#[allow(clippy::too_many_arguments)]
fn observe_sse_input<'native>(
    index: u64,
    node: &'native Value,
    ordinal: &mut u64,
    sites: &mut TextSites<'native>,
    groups: &mut PayloadVec<SseGroup<'native>>,
    budget: &PayloadBudget,
    depth: usize,
    remaining_work: &mut usize,
) -> Result<(), RestoreError> {
    visit_node(depth, remaining_work)?;
    match node {
        Value::String(_) => {
            observe_sse_fragment(
                index,
                SseCarrier::InputLeaf(*ordinal),
                node,
                sites,
                groups,
                budget,
            )?;
            *ordinal = ordinal.checked_add(1).ok_or(RestoreError::RestoreFailed)?;
        }
        Value::Array(items) => {
            for node in items {
                observe_sse_input(
                    index,
                    node,
                    ordinal,
                    sites,
                    groups,
                    budget,
                    depth + 1,
                    remaining_work,
                )?;
            }
        }
        Value::Object(fields) => {
            for node in fields.values() {
                observe_sse_input(
                    index,
                    node,
                    ordinal,
                    sites,
                    groups,
                    budget,
                    depth + 1,
                    remaining_work,
                )?;
            }
        }
        _ => {}
    }
    Ok(())
}

/// Native observations only; strict EOF proof and sealed publication remain shared.
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
        .map_err(RestoreError::scan_error)?;
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
        let source = if has_data {
            let encoded =
                std::str::from_utf8(data.as_slice()).map_err(|_| RestoreError::RestoreFailed)?;
            Some(
                PayloadJson::try_from_str(encoded, budget.clone(), budget.limit())
                    .map_err(RestoreError::capacity)?,
            )
        } else {
            None
        };
        parsed.push((raw, source)).map_err(RestoreError::capacity)?;
    }
    let mut frames = PayloadVec::new(budget.clone()).map_err(RestoreError::capacity)?;
    let mut snapshots = PayloadVec::new(budget.clone()).map_err(RestoreError::capacity)?;
    let mut groups: PayloadVec<SseGroup<'_>> =
        PayloadVec::new(budget.clone()).map_err(RestoreError::capacity)?;
    let mut remaining_work = 4 * 1024 * 1024;
    let mut site_count = 0usize;
    for (frame_index, (raw, source)) in parsed.as_slice().iter().enumerate() {
        let mut sites = TextSites::new(budget.clone()).map_err(RestoreError::contract)?;
        if let Some(source) = source {
            let source = source.as_value();
            snapshots
                .push(NativeSseSnapshotSelection::new(frame_index, source))
                .map_err(RestoreError::capacity)?;
            let kind = source
                .get("type")
                .and_then(Value::as_str)
                .ok_or(RestoreError::RestoreFailed)?;
            let selected = match kind {
                "content_block_start" => match source
                    .pointer("/content_block/type")
                    .and_then(Value::as_str)
                {
                    Some("text") => Some((SseCarrier::Text, source.pointer("/content_block/text"))),
                    Some("thinking") => Some((
                        SseCarrier::Thinking,
                        source.pointer("/content_block/thinking"),
                    )),
                    Some("tool_use") => {
                        let index = source
                            .get("index")
                            .and_then(Value::as_u64)
                            .filter(|index| *index < 128)
                            .ok_or(RestoreError::RestoreFailed)?;
                        let input = source
                            .pointer("/content_block/input")
                            .filter(|input| input.is_object())
                            .ok_or(RestoreError::RestoreFailed)?;
                        observe_sse_input(
                            index,
                            input,
                            &mut 0,
                            &mut sites,
                            &mut groups,
                            &budget,
                            0,
                            &mut remaining_work,
                        )?;
                        None
                    }
                    Some("redacted_thinking") => None,
                    _ => return Err(RestoreError::RestoreUnsupported),
                },
                "content_block_delta" => {
                    match source.pointer("/delta/type").and_then(Value::as_str) {
                        Some("text_delta") => {
                            Some((SseCarrier::Text, source.pointer("/delta/text")))
                        }
                        Some("thinking_delta") => {
                            Some((SseCarrier::Thinking, source.pointer("/delta/thinking")))
                        }
                        Some("input_json_delta") => {
                            Some((SseCarrier::Arguments, source.pointer("/delta/partial_json")))
                        }
                        Some("signature_delta") => None,
                        _ => return Err(RestoreError::RestoreUnsupported),
                    }
                }
                "message_start" | "content_block_stop" | "message_delta" | "message_stop"
                | "ping" => None,
                _ => return Err(RestoreError::RestoreUnsupported),
            };
            if let Some((carrier, node)) = selected {
                let index = source
                    .get("index")
                    .and_then(Value::as_u64)
                    .filter(|index| *index < 128)
                    .ok_or(RestoreError::RestoreFailed)?;
                observe_sse_fragment(
                    index,
                    carrier,
                    node.ok_or(RestoreError::RestoreFailed)?,
                    &mut sites,
                    &mut groups,
                    &budget,
                )?;
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
                group.carrier.policy(),
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
        ProtocolPair::new(client(), UpstreamProtocol::new(Protocol::Anthropic)),
        attempt,
    )?;
    finalize_native_sse(stage, lease, proof, attempt, max)
}
