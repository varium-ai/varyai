//! Borrowed native JSON inspection and private staging; no completion or publication authority.
use serde_json::Value;

use super::super::{
    completed_context::ActualAttemptBinding,
    payload_budget::PayloadBudget,
    payload_json::PayloadJson,
    payload_metadata::PayloadVec,
    payload_text::PayloadText,
    response_restoration::RestoreError,
    restoration_contract::{ClientProtocol, TextSites},
    tokenization::DlpRequestLease,
};

/// Adapter-selected native nodes are inspection claims, never proof of delivery.
/// Metadata is owned and charged; native strings and snapshots remain exact borrows.
pub(in crate::proxy::security) struct NativeJsonObservation<'native> {
    source: &'native Value,
    client: ClientProtocol,
    sites: TextSites<'native>,
    delivered_nodes: PayloadVec<&'native Value>,
    budget: PayloadBudget,
}

impl<'native> NativeJsonObservation<'native> {
    pub(in crate::proxy::security) fn new(
        source: &'native Value,
        client: ClientProtocol,
        sites: TextSites<'native>,
        delivered_nodes: &[&'native Value],
        budget: PayloadBudget,
    ) -> Result<Self, RestoreError> {
        use crate::proxy::protocol_bridge::Protocol;

        use super::super::tokenization::TokenError;

        // Bound all identity searches together without copying or indexing native data.
        fn contains_node(
            source: &Value,
            target: &Value,
            depth: usize,
            remaining_work: &mut usize,
        ) -> Result<bool, RestoreError> {
            if depth > 64 {
                return Err(RestoreError::capacity(TokenError::CapacityExceeded));
            }
            *remaining_work = remaining_work
                .checked_sub(1)
                .ok_or_else(|| RestoreError::capacity(TokenError::CapacityExceeded))?;
            if std::ptr::eq(source, target) {
                return Ok(true);
            }
            match source {
                Value::Array(items) => {
                    for item in items {
                        if contains_node(item, target, depth + 1, remaining_work)? {
                            return Ok(true);
                        }
                    }
                }
                Value::Object(fields) => {
                    for value in fields.values() {
                        if contains_node(value, target, depth + 1, remaining_work)? {
                            return Ok(true);
                        }
                    }
                }
                _ => {}
            }
            Ok(false)
        }

        if client.protocol() == Protocol::Responses {
            return Err(RestoreError::RestoreUnsupported);
        }
        if delivered_nodes.is_empty() || delivered_nodes.len() > 128 {
            return Err(RestoreError::RestoreFailed);
        }
        let mut remaining_work = 4 * 1024 * 1024;
        for index in 0..sites.len() {
            let site = sites.site(index).ok_or(RestoreError::RestoreFailed)?;
            if !contains_node(source, site.native(), 0, &mut remaining_work)? {
                return Err(RestoreError::RestoreFailed);
            }
        }
        for node in delivered_nodes {
            if !contains_node(source, node, 0, &mut remaining_work)? {
                return Err(RestoreError::RestoreFailed);
            }
        }
        let mut snapshots = PayloadVec::new(budget.clone()).map_err(RestoreError::capacity)?;
        for node in delivered_nodes {
            snapshots.push(*node).map_err(RestoreError::capacity)?;
        }
        Ok(Self {
            source,
            client,
            sites,
            delivered_nodes: snapshots,
            budget,
        })
    }

    pub(in crate::proxy::security) fn source(&self) -> &'native Value {
        self.source
    }

    pub(in crate::proxy::security) fn client(&self) -> ClientProtocol {
        self.client
    }

    pub(in crate::proxy::security) fn sites(&self) -> &TextSites<'native> {
        &self.sites
    }

    pub(in crate::proxy::security) fn delivered_nodes(&self) -> &[&'native Value] {
        self.delivered_nodes.as_slice()
    }

    pub(in crate::proxy::security) fn budget(&self) -> PayloadBudget {
        self.budget.clone()
    }
}

struct NativeJsonEdit<'native> {
    target: &'native Value,
    text: PayloadText,
}

// Keep the charged lifecycle inline; boxing would add an uncharged allocation.
#[allow(clippy::large_enum_variant)]
enum NativeJsonRestoration<'native> {
    Legacy {
        edits: PayloadVec<NativeJsonEdit<'native>>,
        output_tokens: PayloadVec<PayloadText>,
        occurrences: usize,
    },
    Lifecycle(super::FinishedInspection<NativeJsonEdit<'native>>),
}

impl<'native> NativeJsonRestoration<'native> {
    fn edits(&self) -> &[NativeJsonEdit<'native>] {
        match self {
            Self::Legacy { edits, .. } => edits.as_slice(),
            Self::Lifecycle(inspection) => inspection.held_units(),
        }
    }

    fn output_tokens(&self) -> &[PayloadText] {
        match self {
            Self::Legacy { output_tokens, .. } => output_tokens.as_slice(),
            Self::Lifecycle(inspection) => inspection.dependency_tokens(),
        }
    }

    fn occurrences(&self) -> usize {
        match self {
            Self::Legacy { occurrences, .. } => *occurrences,
            Self::Lifecycle(inspection) => inspection.restored_occurrences(),
        }
    }
}

/// Fully charged candidate, ordered snapshots and dependency ledger, inspection only.
/// No mutable view, frames, pending publication, proof issuer or unguarded extraction.
pub(in crate::proxy::security) struct StagedNativeJson<'native> {
    source: &'native Value,
    candidate: PayloadJson,
    delivered: PayloadJson,
    restoration: NativeJsonRestoration<'native>,
    changed: bool,
    binding: ActualAttemptBinding,
    client: ClientProtocol,
    budget: PayloadBudget,
}

impl<'native> StagedNativeJson<'native> {
    pub(in crate::proxy::security) fn source(&self) -> &'native Value {
        self.source
    }

    pub(in crate::proxy::security) fn as_value(&self) -> &Value {
        self.candidate.as_value()
    }

    pub(in crate::proxy::security) fn delivered(&self) -> &Value {
        self.delivered.as_value()
    }

    pub(in crate::proxy::security) fn output_tokens(&self) -> &[PayloadText] {
        self.restoration.output_tokens()
    }

    pub(in crate::proxy::security) fn restored_occurrences(&self) -> usize {
        self.restoration.occurrences()
    }

    pub(in crate::proxy::security) fn changed(&self) -> bool {
        self.changed
    }

    pub(in crate::proxy::security) fn binding(&self) -> &ActualAttemptBinding {
        &self.binding
    }

    pub(in crate::proxy::security) fn client(&self) -> ClientProtocol {
        self.client
    }

    pub(in crate::proxy::security) fn budget(&self) -> PayloadBudget {
        self.budget.clone()
    }

    pub(in crate::proxy::security) fn check_lifecycle_attempt(
        &self,
        attempt: &super::AttemptProvenance,
    ) -> Result<(), RestoreError> {
        if let NativeJsonRestoration::Lifecycle(inspection) = &self.restoration {
            if !inspection.attempt().matches(attempt) || inspection.pair().client() != self.client {
                return Err(RestoreError::RestoreFailed);
            }
        }
        Ok(())
    }
}

/// Lifecycle composition entry; no completion or release capability.
pub(in crate::proxy::security) fn stage_native_json_with_lifecycle<'native>(
    observed: &NativeJsonObservation<'native>,
    lease: &DlpRequestLease,
    binding: &ActualAttemptBinding,
    pair: super::super::restoration_contract::ProtocolPair,
    attempt: &super::AttemptProvenance,
) -> Result<StagedNativeJson<'native>, RestoreError> {
    stage_native_json_inner(observed, lease, binding, Some((pair, attempt)))
}

pub(in crate::proxy::security) fn stage_native_json<'native>(
    observed: &NativeJsonObservation<'native>,
    lease: &DlpRequestLease,
    binding: &ActualAttemptBinding,
) -> Result<StagedNativeJson<'native>, RestoreError> {
    stage_native_json_inner(observed, lease, binding, None)
}

fn stage_native_json_inner<'native>(
    observed: &NativeJsonObservation<'native>,
    lease: &DlpRequestLease,
    binding: &ActualAttemptBinding,
    lifecycle: Option<(
        super::super::restoration_contract::ProtocolPair,
        &super::AttemptProvenance,
    )>,
) -> Result<StagedNativeJson<'native>, RestoreError> {
    use super::super::{
        payload_json::{JsonProjection, JsonStringReplacement, ProjectedItem},
        response_restoration::serialize_json_with_budget,
        restoration_contract::EditConstraint,
        tokenization::TokenError,
    };

    // Follow exact source-node identity into the independently rebuilt candidate.
    fn candidate_node<'candidate>(
        source: &Value,
        target: &Value,
        candidate: &'candidate Value,
        depth: usize,
        remaining_work: &mut usize,
    ) -> Result<Option<&'candidate Value>, RestoreError> {
        if depth > 64 {
            return Err(RestoreError::capacity(TokenError::CapacityExceeded));
        }
        *remaining_work = remaining_work
            .checked_sub(1)
            .ok_or_else(|| RestoreError::capacity(TokenError::CapacityExceeded))?;
        if std::ptr::eq(source, target) {
            return Ok(Some(candidate));
        }
        match source {
            Value::Array(items) => {
                let projected = candidate.as_array().ok_or(RestoreError::RestoreFailed)?;
                if projected.len() != items.len() {
                    return Err(RestoreError::RestoreFailed);
                }
                for (item, projected) in items.iter().zip(projected) {
                    if let Some(found) =
                        candidate_node(item, target, projected, depth + 1, remaining_work)?
                    {
                        return Ok(Some(found));
                    }
                }
            }
            Value::Object(fields) => {
                let projected = candidate.as_object().ok_or(RestoreError::RestoreFailed)?;
                if projected.len() != fields.len() {
                    return Err(RestoreError::RestoreFailed);
                }
                for (key, value) in fields {
                    let projected = projected.get(key).ok_or(RestoreError::RestoreFailed)?;
                    if let Some(found) =
                        candidate_node(value, target, projected, depth + 1, remaining_work)?
                    {
                        return Ok(Some(found));
                    }
                }
            }
            _ => {}
        }
        Ok(None)
    }

    lease
        .require_native_attempt_binding(observed.client(), binding)
        .map_err(RestoreError::scan_error)?;
    let budget = observed.budget();
    // Exactly one mechanics owner supplies both edits and dependencies. Finish is
    // inspection only; the finalizer still consumes the genuine HTTP completion proof.
    let restoration = if let Some((pair, attempt)) = lifecycle {
        use super::super::restoration_contract::{RestoreDecision, TextPolicy};
        use crate::proxy::protocol_bridge::Protocol;

        if pair.client() != observed.client() {
            return Err(RestoreError::RestoreFailed);
        }
        // Only the existing Chat/Chat verifier is executable here; no lane support is inferred.
        if pair.client().protocol() != Protocol::Chat
            || pair.upstream().protocol() != Protocol::Chat
        {
            return Err(RestoreError::RestoreUnsupported);
        }
        let mut owner = super::ResponseLifecycle::begin_native(
            lease
                .fork_attempt_extracted()
                .map_err(RestoreError::capacity)?,
            binding.clone(),
            budget.clone(),
            pair,
            attempt.clone(),
            super::LifecycleLimits {
                max_text_bytes: budget.limit(),
                max_text_streams: observed.sites().len(),
                max_held_bytes: budget.limit(),
                max_held_units: observed.sites().len(),
            },
        )?;
        for index in 0..observed.sites().len() {
            let site = observed
                .sites()
                .site(index)
                .ok_or(RestoreError::RestoreFailed)?;
            let id = owner.open_text(TextPolicy::Output)?;
            let before = owner.restored_occurrences();
            let decision = owner.done(&id, site.text())?;
            if site.constraint() == EditConstraint::Immutable
                && owner.restored_occurrences() != before
            {
                return Err(RestoreError::RestoreFailed);
            }
            match decision {
                RestoreDecision::Unchanged => {}
                RestoreDecision::Patch(text) => {
                    let bytes = text.as_str().len();
                    owner.hold_native(
                        NativeJsonEdit {
                            target: site.native(),
                            text,
                        },
                        bytes,
                    )?;
                }
                RestoreDecision::Hold => return Err(RestoreError::RestoreFailed),
            }
        }
        NativeJsonRestoration::Lifecycle(owner.finish()?)
    } else {
        let mut output_tokens = PayloadVec::new(budget.clone()).map_err(RestoreError::capacity)?;
        let mut occurrences = 0;
        let mut edits = PayloadVec::new(budget.clone()).map_err(RestoreError::capacity)?;
        for index in 0..observed.sites().len() {
            let site = observed
                .sites()
                .site(index)
                .ok_or(RestoreError::RestoreFailed)?;
            let before = occurrences;
            let text = super::restore_text(
                site.text(),
                lease,
                &budget,
                &mut output_tokens,
                &mut occurrences,
            )?;
            if site.constraint() == EditConstraint::Immutable && occurrences != before {
                return Err(RestoreError::RestoreFailed);
            }
            if text.as_str() != site.text() {
                edits
                    .push(NativeJsonEdit {
                        target: site.native(),
                        text,
                    })
                    .map_err(RestoreError::capacity)?;
            }
        }
        NativeJsonRestoration::Legacy {
            edits,
            output_tokens,
            occurrences,
        }
    };
    let changed = !restoration.edits().is_empty();
    let mut edits = PayloadVec::new(budget.clone()).map_err(RestoreError::capacity)?;
    for edit in restoration.edits() {
        edits
            .push(JsonStringReplacement {
                target: edit.target,
                text: edit.text.as_str(),
            })
            .map_err(RestoreError::capacity)?;
    }
    let candidate = PayloadJson::try_with_replacements(
        observed.source(),
        edits.as_slice(),
        budget.clone(),
        budget.limit(),
    )
    .map_err(RestoreError::capacity)?;
    drop(edits);

    let mut snapshots = PayloadVec::new(budget.clone()).map_err(RestoreError::capacity)?;
    let mut remaining_work = 4 * 1024 * 1024;
    for node in observed.delivered_nodes() {
        let value = candidate_node(
            observed.source(),
            node,
            candidate.as_value(),
            0,
            &mut remaining_work,
        )?
        .ok_or(RestoreError::RestoreFailed)?;
        snapshots
            .push(ProjectedItem {
                value,
                cached: None,
                missing: 0,
            })
            .map_err(RestoreError::capacity)?;
    }
    let delivered = PayloadJson::try_from_projection(
        JsonProjection::Input(snapshots.as_slice()),
        budget.clone(),
        budget.limit(),
    )
    .map_err(RestoreError::capacity)?;
    drop(snapshots);
    // Validate budgeted snapshot encoding internally, without frames or publication.
    let encoded = serialize_json_with_budget(delivered.as_value(), &budget, budget.limit())?;
    drop(encoded);
    lease
        .require_native_attempt_binding(observed.client(), binding)
        .map_err(RestoreError::scan_error)?;
    Ok(StagedNativeJson {
        source: observed.source(),
        candidate,
        delivered,
        restoration,
        changed,
        binding: binding.clone(),
        client: observed.client(),
        budget,
    })
}
/// Exact borrowed native SSE frame claim, with no completion or delivery authority.
/// `raw` includes its LF/LF or CRLF/CRLF delimiter. JSON must be independently
/// reparsed with PayloadJson and equal `source`; sites still identify exact source
/// nodes, not nodes from that temporary parse. Only opaque `[DONE]` has no source
/// and no sites. UTF-8 is guaranteed by the borrowed str, never repaired.
///
/// Observation contract: at most 1 MiB per frame, 128 sites, depth 64 and 4M
/// inspection work. Retained metadata and parsing scratch must be precharged.
/// Parsing and exact-node searches share one conservative work allowance.
/// No Debug, Clone, mutable view, snapshot, EOF, proof or extraction capability.
pub(in crate::proxy::security) struct NativeSseFrameObservation<'native> {
    raw: &'native str,
    source: Option<&'native Value>,
    sites: TextSites<'native>,
}

impl<'native> NativeSseFrameObservation<'native> {
    pub(in crate::proxy::security) fn new(
        raw: &'native str,
        source: Option<&'native Value>,
        sites: TextSites<'native>,
        budget: PayloadBudget,
    ) -> Result<Self, RestoreError> {
        use super::super::{payload_buffer::PayloadBuffer, tokenization::TokenError};

        if raw.len() > 1024 * 1024 || sites.len() > 128 {
            return Err(RestoreError::capacity(TokenError::CapacityExceeded));
        }
        // Keep the shared SSE delimiter semantics without copying or normalizing wire bytes.
        let boundary = ["\n\n", "\r\n\r\n"]
            .iter()
            .filter_map(|delimiter| raw.find(delimiter).map(|offset| offset + delimiter.len()))
            .min();
        if boundary != Some(raw.len()) {
            return Err(RestoreError::RestoreFailed);
        }
        let mut remaining_work = 4 * 1024 * 1024;
        Self::charge_work(&mut remaining_work, raw.len())?;
        let data_len = raw.lines().try_fold(None, |length: Option<usize>, line| {
            let Some(value) = Self::data_line(line) else {
                return Ok(length);
            };
            let next = length
                .unwrap_or(0)
                .checked_add(usize::from(length.is_some()))
                .and_then(|bytes| bytes.checked_add(value.len()))
                .ok_or_else(|| RestoreError::capacity(TokenError::CapacityExceeded))?;
            Ok::<_, RestoreError>(Some(next))
        })?;
        let data_len = data_len.ok_or(RestoreError::RestoreFailed)?;
        let mut data = PayloadBuffer::with_capacity(data_len, budget.clone(), 1024 * 1024)
            .map_err(RestoreError::capacity)?;
        let mut first = true;
        for value in raw.lines().filter_map(Self::data_line) {
            if !first {
                data.append(b"\n").map_err(RestoreError::capacity)?;
            }
            first = false;
            data.append(value.as_bytes())
                .map_err(RestoreError::capacity)?;
        }
        let encoded =
            std::str::from_utf8(data.as_slice()).map_err(|_| RestoreError::RestoreFailed)?;
        if encoded.trim() == "[DONE]" {
            if source.is_some() || sites.len() != 0 {
                return Err(RestoreError::RestoreFailed);
            }
        } else {
            let source = source.ok_or(RestoreError::RestoreFailed)?;
            Self::charge_work(&mut remaining_work, encoded.len())?;
            let parsed = PayloadJson::try_from_str(encoded, budget.clone(), budget.limit())
                .map_err(RestoreError::capacity)?;
            if !Self::same_value(source, parsed.as_value(), 0, &mut remaining_work)? {
                return Err(RestoreError::RestoreFailed);
            }
            for index in 0..sites.len() {
                let site = sites.site(index).ok_or(RestoreError::RestoreFailed)?;
                if !Self::contains_node(source, site.native(), 0, &mut remaining_work)? {
                    return Err(RestoreError::RestoreFailed);
                }
            }
        }
        Ok(Self { raw, source, sites })
    }

    fn data_line(line: &str) -> Option<&str> {
        line.strip_prefix("data:")
            .or_else(|| (line == "data").then_some(""))
            .map(|value| value.strip_prefix(' ').unwrap_or(value))
    }

    fn charge_work(remaining: &mut usize, work: usize) -> Result<(), RestoreError> {
        *remaining = remaining.checked_sub(work).ok_or_else(|| {
            RestoreError::capacity(super::super::tokenization::TokenError::CapacityExceeded)
        })?;
        Ok(())
    }

    fn same_value(
        source: &Value,
        parsed: &Value,
        depth: usize,
        remaining: &mut usize,
    ) -> Result<bool, RestoreError> {
        if depth > 64 {
            return Err(RestoreError::capacity(
                super::super::tokenization::TokenError::CapacityExceeded,
            ));
        }
        Self::charge_work(remaining, 1)?;
        match (source, parsed) {
            (Value::Array(left), Value::Array(right)) => {
                if left.len() != right.len() {
                    return Ok(false);
                }
                for (left, right) in left.iter().zip(right) {
                    if !Self::same_value(left, right, depth + 1, remaining)? {
                        return Ok(false);
                    }
                }
                Ok(true)
            }
            (Value::Object(left), Value::Object(right)) => {
                if left.len() != right.len() {
                    return Ok(false);
                }
                for (key, right) in right {
                    Self::charge_work(remaining, key.len())?;
                    let Some(left) = left.get(key) else {
                        return Ok(false);
                    };
                    if !Self::same_value(left, right, depth + 1, remaining)? {
                        return Ok(false);
                    }
                }
                Ok(true)
            }
            (Value::String(left), Value::String(right)) => {
                if left.len() != right.len() {
                    return Ok(false);
                }
                Self::charge_work(remaining, right.len())?;
                Ok(left == right)
            }
            (Value::Null, Value::Null) => Ok(true),
            (Value::Bool(left), Value::Bool(right)) => Ok(left == right),
            (Value::Number(left), Value::Number(right)) => Ok(left == right),
            _ => Ok(false),
        }
    }

    fn contains_node(
        source: &Value,
        target: &Value,
        depth: usize,
        remaining: &mut usize,
    ) -> Result<bool, RestoreError> {
        if depth > 64 {
            return Err(RestoreError::capacity(
                super::super::tokenization::TokenError::CapacityExceeded,
            ));
        }
        Self::charge_work(remaining, 1)?;
        if std::ptr::eq(source, target) {
            return Ok(true);
        }
        match source {
            Value::Array(items) => {
                for item in items {
                    if Self::contains_node(item, target, depth + 1, remaining)? {
                        return Ok(true);
                    }
                }
            }
            Value::Object(fields) => {
                for value in fields.values() {
                    if Self::contains_node(value, target, depth + 1, remaining)? {
                        return Ok(true);
                    }
                }
            }
            _ => {}
        }
        Ok(false)
    }

    pub(in crate::proxy::security) fn raw(&self) -> &'native str {
        self.raw
    }

    pub(in crate::proxy::security) fn source(&self) -> Option<&'native Value> {
        self.source
    }

    pub(in crate::proxy::security) fn sites(&self) -> &TextSites<'native> {
        &self.sites
    }
}

/// Ordered original-entity inspection, not stream completion or a publication plan.
/// Frame raw borrows must partition this exact entity slice by pointer and length,
/// in order, without gaps, overlap, foreign copies or unmatched trailing bytes.
/// The client must be non-Responses and match the lease's actual attempt binding;
/// that binding remains an exact borrow, not a caller-supplied completion snapshot.
///
/// The complete entity is UTF-8 and at most 1 MiB, with 1..=1024 frames and at most
/// 128 total sites. Depth 64 and 4M inspection work are shared across all frames;
/// neither metadata-only bytes nor frames without sites bypass these limits.
/// Ordered frame-reference storage is charged before allocation and refunded on
/// failure/drop. No protocol-specific event semantics are interpreted here.
/// This owner retains only immutable observations and grants no completion authority.
pub(in crate::proxy::security) struct NativeSseObservation<'native> {
    entity: &'native [u8],
    frames: PayloadVec<&'native NativeSseFrameObservation<'native>>,
    client: ClientProtocol,
    binding: &'native ActualAttemptBinding,
}

impl<'native> NativeSseObservation<'native> {
    pub(in crate::proxy::security) fn new(
        entity: &'native [u8],
        ordered_frames: &[&'native NativeSseFrameObservation<'native>],
        client: ClientProtocol,
        lease: &DlpRequestLease,
        binding: &'native ActualAttemptBinding,
        budget: PayloadBudget,
    ) -> Result<Self, RestoreError> {
        use super::super::tokenization::TokenError;
        use crate::proxy::protocol_bridge::Protocol;

        if client.protocol() == Protocol::Responses {
            return Err(RestoreError::RestoreUnsupported);
        }
        lease
            .require_native_attempt_binding(client, binding)
            .map_err(RestoreError::scan_error)?;
        if entity.len() > 1024 * 1024 || ordered_frames.len() > 1024 {
            return Err(RestoreError::capacity(TokenError::CapacityExceeded));
        }
        if entity.is_empty() || ordered_frames.is_empty() || std::str::from_utf8(entity).is_err() {
            return Err(RestoreError::RestoreFailed);
        }
        let mut remaining_work = 4 * 1024 * 1024;
        NativeSseFrameObservation::charge_work(&mut remaining_work, entity.len())?;
        let mut offset = 0usize;
        let mut site_count = 0usize;
        for frame in ordered_frames {
            let end = offset
                .checked_add(frame.raw.len())
                .ok_or_else(|| RestoreError::capacity(TokenError::CapacityExceeded))?;
            let original = entity.get(offset..end).ok_or(RestoreError::RestoreFailed)?;
            // Equal bytes, a shared allocation or a matching Value are insufficient.
            if !std::ptr::eq(original, frame.raw.as_bytes()) {
                return Err(RestoreError::RestoreFailed);
            }
            offset = end;
            site_count = site_count
                .checked_add(frame.sites.len())
                .filter(|count| *count <= 128)
                .ok_or_else(|| RestoreError::capacity(TokenError::CapacityExceeded))?;
            if let Some(source) = frame.source {
                // Reuse bounded immutable source traversal, never a caller snapshot.
                if !NativeSseFrameObservation::same_value(source, source, 0, &mut remaining_work)? {
                    return Err(RestoreError::RestoreFailed);
                }
                for index in 0..frame.sites.len() {
                    let site = frame.sites.site(index).ok_or(RestoreError::RestoreFailed)?;
                    if !NativeSseFrameObservation::contains_node(
                        source,
                        site.native(),
                        0,
                        &mut remaining_work,
                    )? {
                        return Err(RestoreError::RestoreFailed);
                    }
                }
            }
        }
        if offset != entity.len() {
            return Err(RestoreError::RestoreFailed);
        }
        // Retain only validated exact references; tentative allocations refund on failure.
        let mut frames = PayloadVec::new(budget).map_err(RestoreError::capacity)?;
        for frame in ordered_frames {
            frames.push(*frame).map_err(RestoreError::capacity)?;
        }
        lease
            .require_native_attempt_binding(client, binding)
            .map_err(RestoreError::scan_error)?;
        Ok(Self {
            entity,
            frames,
            client,
            binding,
        })
    }

    pub(in crate::proxy::security) fn entity(&self) -> &'native [u8] {
        self.entity
    }

    pub(in crate::proxy::security) fn frame_count(&self) -> usize {
        self.frames.len()
    }

    pub(in crate::proxy::security) fn frame(
        &self,
        index: usize,
    ) -> Option<&'native NativeSseFrameObservation<'native>> {
        self.frames.as_slice().get(index).copied()
    }

    pub(in crate::proxy::security) fn client(&self) -> ClientProtocol {
        self.client
    }

    pub(in crate::proxy::security) fn binding(&self) -> &'native ActualAttemptBinding {
        self.binding
    }
}

/// Adapter-selected original node, not a delivered snapshot or completion claim.
/// The index only selects a frame owner: staging must also establish exact node
/// identity inside that frame's original JSON. Equal foreign values are invalid.
/// Selection order and duplicates are preserved; staging accepts 1..=128 entries.
pub(in crate::proxy::security) struct NativeSseSnapshotSelection<'native> {
    frame_index: usize,
    native: &'native Value,
}

impl<'native> NativeSseSnapshotSelection<'native> {
    pub(in crate::proxy::security) fn new(frame_index: usize, native: &'native Value) -> Self {
        Self {
            frame_index,
            native,
        }
    }
}

// All retained candidates and encodings have independent, precharged owners.
// A missing candidate denotes opaque DONE, whose original wire is preserved.
// No wire getter or extraction is available outside this private module.
struct StagedNativeSseFrame<'native> {
    source: &'native NativeSseFrameObservation<'native>,
    candidate: Option<PayloadJson>,
    encoded: super::super::payload_buffer::PayloadBuffer,
}

struct NativeSseEdit<'native> {
    target: &'native Value,
    text: PayloadText,
}
// Keep the charged lifecycle inline; boxing would add an uncharged allocation.
#[allow(clippy::large_enum_variant)]
enum NativeSseRestoration<'native> {
    Legacy {
        output_tokens: PayloadVec<PayloadText>,
        occurrences: usize,
    },
    Lifecycle(super::FinishedInspection<NativeSseEdit<'native>>),
}

/// Opaque, charged SSE candidate and ledger; never a completion or release permit.
/// Original entity/frame references remain exact immutable borrows, independently
/// of the observation's metadata allocation. Ordered snapshots must be projected
/// from the restored candidates using exact source-bound adapter selections.
///
/// Every final frame encoding is retained and charged, changed or unchanged,
/// including opaque DONE. The inspection digest uses SHA-256 over the domain
/// `native-sse-staging-wire-v1\0` followed by each actual encoded frame's u64
/// little-endian length and bytes in order; it is not a ReleaseIdentity or proof.
/// Protocol validation (including completed tool arguments) belongs to the sealed
/// finalizer. Only its unconstructible key permits a bounded wire reader or consuming
/// transfer. This owner cannot issue EOF, publication, release identity or activation.
pub(in crate::proxy::security) struct StagedNativeSse<'native> {
    entity: &'native [u8],
    frames: PayloadVec<StagedNativeSseFrame<'native>>,
    // Retain exact ordered selections without borrowing the caller's metadata.
    _snapshot_selections: PayloadVec<NativeSseSnapshotSelection<'native>>,
    delivered: PayloadJson,
    restoration: NativeSseRestoration<'native>,
    changed: bool,
    binding: &'native ActualAttemptBinding,
    client: ClientProtocol,
    budget: PayloadBudget,
    release_digest: [u8; 32],
}

impl<'native> StagedNativeSse<'native> {
    pub(in crate::proxy::security) fn entity(&self) -> &'native [u8] {
        self.entity
    }

    pub(in crate::proxy::security) fn frame_count(&self) -> usize {
        self.frames.len()
    }

    pub(in crate::proxy::security) fn source_frame(
        &self,
        index: usize,
    ) -> Option<&'native NativeSseFrameObservation<'native>> {
        self.frames.as_slice().get(index).map(|frame| frame.source)
    }

    pub(in crate::proxy::security) fn delivered(&self) -> &Value {
        self.delivered.as_value()
    }

    pub(in crate::proxy::security) fn output_tokens(&self) -> &[PayloadText] {
        match &self.restoration {
            NativeSseRestoration::Legacy { output_tokens, .. } => output_tokens.as_slice(),
            NativeSseRestoration::Lifecycle(owner) => owner.dependency_tokens(),
        }
    }

    pub(in crate::proxy::security) fn restored_occurrences(&self) -> usize {
        match &self.restoration {
            NativeSseRestoration::Legacy { occurrences, .. } => *occurrences,
            NativeSseRestoration::Lifecycle(owner) => owner.restored_occurrences(),
        }
    }

    pub(in crate::proxy::security) fn changed(&self) -> bool {
        self.changed
    }

    pub(in crate::proxy::security) fn binding(&self) -> &'native ActualAttemptBinding {
        self.binding
    }

    pub(in crate::proxy::security) fn client(&self) -> ClientProtocol {
        self.client
    }

    pub(in crate::proxy::security) fn budget(&self) -> PayloadBudget {
        self.budget.clone()
    }

    pub(in crate::proxy::security) fn check_lifecycle_attempt(
        &self,
        attempt: &super::AttemptProvenance,
    ) -> Result<(), RestoreError> {
        if let NativeSseRestoration::Lifecycle(owner) = &self.restoration {
            if !owner.attempt().matches(attempt) || owner.pair().client() != self.client {
                return Err(RestoreError::RestoreFailed);
            }
        }
        Ok(())
    }

    pub(in crate::proxy::security) fn release_digest(&self) -> [u8; 32] {
        self.release_digest
    }
}

/// Adapter-declared fragments of one decoded native text stream; not completion evidence.
pub(in crate::proxy::security) struct NativeSseTextStream<'native> {
    fragments: &'native [&'native Value],
}
impl<'native> NativeSseTextStream<'native> {
    pub(in crate::proxy::security) fn new(fragments: &'native [&'native Value]) -> Self {
        Self { fragments }
    }
}

/// Compose declared decoded streams without learning protocol indices or terminals.
/// Exact, unique, ordered site coverage is checked before any matcher work.
fn inspect_native_sse_streams<'native>(
    observed: &NativeSseObservation<'native>,
    streams: &[NativeSseTextStream<'native>],
    lease: &DlpRequestLease,
    binding: &ActualAttemptBinding,
    budget: &PayloadBudget,
    pair: super::super::restoration_contract::ProtocolPair,
    attempt: &super::AttemptProvenance,
) -> Result<super::FinishedInspection<NativeSseEdit<'native>>, RestoreError> {
    use super::super::restoration_contract::{EditConstraint, RestoreDecision, TextPolicy};
    use super::super::tokenization::TokenError;
    use crate::proxy::protocol_bridge::Protocol;
    if pair.client() != observed.client() {
        return Err(RestoreError::RestoreFailed);
    }
    if pair.client().protocol() != Protocol::Chat || pair.upstream().protocol() != Protocol::Chat {
        return Err(RestoreError::RestoreUnsupported);
    }
    let mut sites = PayloadVec::new(budget.clone()).map_err(RestoreError::capacity)?;
    for index in 0..observed.frame_count() {
        let frame = observed.frame(index).ok_or(RestoreError::RestoreFailed)?;
        for site_index in 0..frame.sites().len() {
            sites
                .push(
                    frame
                        .sites()
                        .site(site_index)
                        .ok_or(RestoreError::RestoreFailed)?,
                )
                .map_err(RestoreError::capacity)?;
        }
    }
    let count = sites.len();
    if streams.len() > count || count > 128 {
        return Err(RestoreError::RestoreFailed);
    }
    let mut selected = PayloadVec::new(budget.clone()).map_err(RestoreError::capacity)?;
    for stream in streams {
        if stream.fragments.is_empty() || stream.fragments.len() > count {
            return Err(RestoreError::RestoreFailed);
        }
        let mut previous = None;
        for native in stream.fragments {
            let index = sites
                .as_slice()
                .iter()
                .position(|site| std::ptr::eq(site.native(), *native))
                .ok_or(RestoreError::RestoreFailed)?;
            if previous.is_some_and(|previous| index <= previous)
                || selected.as_slice().contains(&index)
            {
                return Err(RestoreError::RestoreFailed);
            }
            selected.push(index).map_err(RestoreError::capacity)?;
            previous = Some(index);
        }
    }
    if selected.len() != count {
        return Err(RestoreError::RestoreFailed);
    }
    let mut owner = super::ResponseLifecycle::begin_native(
        lease
            .fork_attempt_extracted()
            .map_err(RestoreError::capacity)?,
        binding.clone(),
        budget.clone(),
        pair,
        attempt.clone(),
        super::LifecycleLimits {
            max_text_bytes: budget.limit().min(1024 * 1024),
            max_text_streams: count,
            max_held_bytes: budget.limit(),
            max_held_units: count,
        },
    )?;
    owner.hold_all()?;
    for stream in streams {
        let length = stream.fragments.iter().try_fold(0usize, |total, node| {
            total
                .checked_add(node.as_str().ok_or(RestoreError::RestoreFailed)?.len())
                .filter(|bytes| *bytes <= 1024 * 1024)
                .ok_or_else(|| RestoreError::capacity(TokenError::CapacityExceeded))
        })?;
        let mut raw = PayloadText::with_capacity(length, budget.clone(), 1024 * 1024)
            .map_err(RestoreError::capacity)?;
        let mut emitted = PayloadVec::new(budget.clone()).map_err(RestoreError::capacity)?;
        let id = owner.open_text(TextPolicy::Output)?;
        let before = owner.restored_occurrences();
        for node in stream.fragments {
            let text = node.as_str().ok_or(RestoreError::RestoreFailed)?;
            raw.append(text).map_err(RestoreError::capacity)?;
            let bytes = match owner.delta(&id, text)? {
                RestoreDecision::Unchanged => text.len(),
                RestoreDecision::Hold => 0,
                RestoreDecision::Patch(text) => text.as_str().len(),
            };
            emitted.push(bytes).map_err(RestoreError::capacity)?;
        }
        let decision = owner.done(&id, raw.as_str())?;
        if let RestoreDecision::Patch(restored) = decision {
            if owner.restored_occurrences() != before
                && stream.fragments.iter().any(|node| {
                    sites.as_slice().iter().any(|site| {
                        std::ptr::eq(site.native(), *node)
                            && site.constraint() == EditConstraint::Immutable
                    })
                })
            {
                return Err(RestoreError::RestoreFailed);
            }
            let mut offset = 0usize;
            for (index, node) in stream.fragments.iter().enumerate() {
                let end = if index + 1 == stream.fragments.len() {
                    restored.as_str().len()
                } else {
                    offset
                        .checked_add(emitted.as_slice()[index])
                        .ok_or(RestoreError::RestoreFailed)?
                };
                let text = restored
                    .as_str()
                    .get(offset..end)
                    .ok_or(RestoreError::RestoreFailed)?;
                let site = sites
                    .as_slice()
                    .iter()
                    .find(|site| std::ptr::eq(site.native(), *node))
                    .ok_or(RestoreError::RestoreFailed)?;
                if text != site.text() {
                    if site.constraint() == EditConstraint::Immutable {
                        return Err(RestoreError::RestoreFailed);
                    }
                    let text = PayloadText::try_from_str(text, budget.clone(), budget.limit())
                        .map_err(RestoreError::capacity)?;
                    let bytes = text.as_str().len();
                    owner.hold_native(NativeSseEdit { target: node, text }, bytes)?;
                }
                offset = end;
            }
        }
    }
    owner.finish()
}

// Adapter observations and independent authority inputs stay explicit in the frozen API.
#[allow(clippy::too_many_arguments)]
pub(in crate::proxy::security) fn stage_native_sse_with_lifecycle<'native>(
    observed: &NativeSseObservation<'native>,
    snapshots: &[NativeSseSnapshotSelection<'native>],
    streams: &[NativeSseTextStream<'native>],
    lease: &DlpRequestLease,
    binding: &ActualAttemptBinding,
    budget: PayloadBudget,
    pair: super::super::restoration_contract::ProtocolPair,
    attempt: &super::AttemptProvenance,
) -> Result<StagedNativeSse<'native>, RestoreError> {
    stage_native_sse_inner(
        observed,
        snapshots,
        lease,
        binding,
        budget,
        Some((streams, pair, attempt)),
    )
}

/// Stage exact native frames and restored snapshots as one local, charged transaction.
/// This does not interpret completion, validate complete tool calls or grant release.
/// No candidate, encoding or dependency escapes unless every operation succeeds.
pub(in crate::proxy::security) fn stage_native_sse<'native>(
    observed: &NativeSseObservation<'native>,
    snapshots: &[NativeSseSnapshotSelection<'native>],
    lease: &DlpRequestLease,
    binding: &ActualAttemptBinding,
    budget: PayloadBudget,
) -> Result<StagedNativeSse<'native>, RestoreError> {
    stage_native_sse_inner(observed, snapshots, lease, binding, budget, None)
}

fn stage_native_sse_inner<'native>(
    observed: &NativeSseObservation<'native>,
    snapshots: &[NativeSseSnapshotSelection<'native>],
    lease: &DlpRequestLease,
    binding: &ActualAttemptBinding,
    budget: PayloadBudget,
    lifecycle: Option<(
        &[NativeSseTextStream<'native>],
        super::super::restoration_contract::ProtocolPair,
        &super::AttemptProvenance,
    )>,
) -> Result<StagedNativeSse<'native>, RestoreError> {
    use sha2::{Digest, Sha256};

    use super::super::{
        payload_buffer::PayloadBuffer,
        payload_json::{JsonProjection, JsonStringReplacement, ProjectedItem},
        response_restoration::serialize_json_with_budget,
        restoration_contract::EditConstraint,
        tokenization::TokenError,
    };

    // Walk source and candidate in lockstep; equal siblings never select a node.
    fn candidate_node<'candidate>(
        source: &Value,
        target: &Value,
        candidate: &'candidate Value,
        depth: usize,
        remaining: &mut usize,
    ) -> Result<Option<&'candidate Value>, RestoreError> {
        if depth > 64 {
            return Err(RestoreError::capacity(TokenError::CapacityExceeded));
        }
        NativeSseFrameObservation::charge_work(remaining, 1)?;
        if std::ptr::eq(source, target) {
            return Ok(Some(candidate));
        }
        match source {
            Value::Array(items) => {
                let projected = candidate.as_array().ok_or(RestoreError::RestoreFailed)?;
                if projected.len() != items.len() {
                    return Err(RestoreError::RestoreFailed);
                }
                for (item, projected) in items.iter().zip(projected) {
                    if let Some(found) =
                        candidate_node(item, target, projected, depth + 1, remaining)?
                    {
                        return Ok(Some(found));
                    }
                }
            }
            Value::Object(fields) => {
                let projected = candidate.as_object().ok_or(RestoreError::RestoreFailed)?;
                if projected.len() != fields.len() {
                    return Err(RestoreError::RestoreFailed);
                }
                for (key, value) in fields {
                    let projected = projected.get(key).ok_or(RestoreError::RestoreFailed)?;
                    if let Some(found) =
                        candidate_node(value, target, projected, depth + 1, remaining)?
                    {
                        return Ok(Some(found));
                    }
                }
            }
            _ => {}
        }
        Ok(None)
    }

    // Replace only data values, retaining metadata and every original line ending.
    // Later data lines contribute trailing JSON whitespace, not original payload.
    fn visit_encoded_parts(
        raw: &str,
        data: &[u8],
        mut visit: impl FnMut(&[u8]) -> Result<(), RestoreError>,
    ) -> Result<(), RestoreError> {
        let mut emitted = false;
        for original in raw.split_inclusive('\n') {
            let line = original.strip_suffix('\n').unwrap_or(original);
            let line = line.strip_suffix('\r').unwrap_or(line);
            if NativeSseFrameObservation::data_line(line).is_some() {
                visit(if line.starts_with("data: ") {
                    b"data: "
                } else {
                    b"data:"
                })?;
                if !emitted {
                    visit(data)?;
                    emitted = true;
                }
                visit(&original.as_bytes()[line.len()..])?;
            } else {
                visit(original.as_bytes())?;
            }
        }
        if !emitted {
            return Err(RestoreError::RestoreFailed);
        }
        Ok(())
    }

    let original_binding = observed.binding();
    if binding.store_id != original_binding.store_id
        || binding.namespace != original_binding.namespace
        || binding.generation != original_binding.generation
        || binding.policy_revision != original_binding.policy_revision
        || binding.provider_fingerprint != original_binding.provider_fingerprint
        || binding.reusable_principal != original_binding.reusable_principal
        || binding.operation_lineage != original_binding.operation_lineage
    {
        return Err(RestoreError::RestoreFailed);
    }
    lease
        .require_native_attempt_binding(observed.client(), original_binding)
        .map_err(RestoreError::scan_error)?;
    lease
        .require_native_attempt_binding(observed.client(), binding)
        .map_err(RestoreError::scan_error)?;
    if snapshots.is_empty() {
        return Err(RestoreError::RestoreFailed);
    }
    if snapshots.len() > 128 {
        return Err(RestoreError::capacity(TokenError::CapacityExceeded));
    }

    let inspection = lifecycle
        .map(|(streams, pair, attempt)| {
            inspect_native_sse_streams(observed, streams, lease, binding, &budget, pair, attempt)
        })
        .transpose()?;

    let wire_limit = budget.limit().min(1024 * 1024);
    let mut remaining_work = 4 * 1024 * 1024;
    NativeSseFrameObservation::charge_work(&mut remaining_work, observed.entity().len())?;
    let mut frames = PayloadVec::new(budget.clone()).map_err(RestoreError::capacity)?;
    let mut output_tokens = PayloadVec::new(budget.clone()).map_err(RestoreError::capacity)?;
    let mut restored_occurrences = 0;
    let mut changed = false;
    let mut entity_bytes = 0usize;
    for index in 0..observed.frame_count() {
        let frame = observed.frame(index).ok_or(RestoreError::RestoreFailed)?;
        let (candidate, encoded) = if let Some(source) = frame.source() {
            if !NativeSseFrameObservation::same_value(source, source, 0, &mut remaining_work)? {
                return Err(RestoreError::RestoreFailed);
            }
            let mut restored_sites =
                PayloadVec::new(budget.clone()).map_err(RestoreError::capacity)?;
            for index in 0..frame.sites().len() {
                let site = frame
                    .sites()
                    .site(index)
                    .ok_or(RestoreError::RestoreFailed)?;
                if !NativeSseFrameObservation::contains_node(
                    source,
                    site.native(),
                    0,
                    &mut remaining_work,
                )? {
                    return Err(RestoreError::RestoreFailed);
                }
                NativeSseFrameObservation::charge_work(&mut remaining_work, site.text().len())?;
                if let Some(owner) = inspection.as_ref() {
                    for edit in owner.held_units() {
                        if std::ptr::eq(edit.target, site.native()) {
                            restored_sites
                                .push((edit.target, edit.text.as_str()))
                                .map_err(RestoreError::capacity)?;
                        }
                    }
                }
            }
            // Legacy callers retain the established per-site behavior. Lifecycle callers
            // consume the exact edits from the single finished owner, without rescanning.
            let mut legacy_texts =
                PayloadVec::new(budget.clone()).map_err(RestoreError::capacity)?;
            if inspection.is_none() {
                for index in 0..frame.sites().len() {
                    let site = frame
                        .sites()
                        .site(index)
                        .ok_or(RestoreError::RestoreFailed)?;
                    let before = restored_occurrences;
                    let restored = super::restore_text(
                        site.text(),
                        lease,
                        &budget,
                        &mut output_tokens,
                        &mut restored_occurrences,
                    )?;
                    if site.constraint() == EditConstraint::Immutable
                        && restored_occurrences != before
                    {
                        return Err(RestoreError::RestoreFailed);
                    }
                    if restored.as_str() != site.text() {
                        legacy_texts
                            .push((site.native(), restored))
                            .map_err(RestoreError::capacity)?;
                    }
                }
                for (target, text) in legacy_texts.as_slice() {
                    restored_sites
                        .push((*target, text.as_str()))
                        .map_err(RestoreError::capacity)?;
                }
            }
            let frame_changed = !restored_sites.as_slice().is_empty();
            let mut edits = PayloadVec::new(budget.clone()).map_err(RestoreError::capacity)?;
            for (target, text) in restored_sites.as_slice() {
                edits
                    .push(JsonStringReplacement { target, text })
                    .map_err(RestoreError::capacity)?;
            }
            let candidate = PayloadJson::try_with_replacements(
                source,
                edits.as_slice(),
                budget.clone(),
                budget.limit(),
            )
            .map_err(RestoreError::capacity)?;
            drop(edits);
            drop(restored_sites);
            drop(legacy_texts);
            // Encoding is mandatory even when there were no mappings or sites.
            let data = serialize_json_with_budget(candidate.as_value(), &budget, wire_limit)?;
            NativeSseFrameObservation::charge_work(&mut remaining_work, data.len())?;
            let encoded = if frame_changed {
                let mut length = 0usize;
                visit_encoded_parts(frame.raw(), data.as_slice(), |part| {
                    length = length
                        .checked_add(part.len())
                        .filter(|length| *length <= wire_limit)
                        .ok_or_else(|| RestoreError::capacity(TokenError::CapacityExceeded))?;
                    Ok(())
                })?;
                let mut encoded = PayloadBuffer::with_capacity(length, budget.clone(), wire_limit)
                    .map_err(RestoreError::capacity)?;
                visit_encoded_parts(frame.raw(), data.as_slice(), |part| {
                    encoded.append(part).map_err(RestoreError::capacity)
                })?;
                encoded
            } else {
                PayloadBuffer::try_from_slice(frame.raw().as_bytes(), budget.clone(), wire_limit)
                    .map_err(RestoreError::capacity)?
            };
            changed |= frame_changed;
            (Some(candidate), encoded)
        } else {
            // Opaque DONE is retained verbatim, with no completion interpretation.
            let encoded =
                PayloadBuffer::try_from_slice(frame.raw().as_bytes(), budget.clone(), wire_limit)
                    .map_err(RestoreError::capacity)?;
            (None, encoded)
        };
        NativeSseFrameObservation::charge_work(&mut remaining_work, encoded.len())?;
        entity_bytes = entity_bytes
            .checked_add(encoded.len())
            .filter(|bytes| *bytes <= 1024 * 1024)
            .ok_or_else(|| RestoreError::capacity(TokenError::CapacityExceeded))?;
        frames
            .push(StagedNativeSseFrame {
                source: frame,
                candidate,
                encoded,
            })
            .map_err(RestoreError::capacity)?;
    }

    let mut selections = PayloadVec::new(budget.clone()).map_err(RestoreError::capacity)?;
    let mut projected = PayloadVec::new(budget.clone()).map_err(RestoreError::capacity)?;
    for selection in snapshots {
        let frame = frames
            .as_slice()
            .get(selection.frame_index)
            .ok_or(RestoreError::RestoreFailed)?;
        let source = frame.source.source().ok_or(RestoreError::RestoreFailed)?;
        let candidate = frame
            .candidate
            .as_ref()
            .ok_or(RestoreError::RestoreFailed)?;
        let value = candidate_node(
            source,
            selection.native,
            candidate.as_value(),
            0,
            &mut remaining_work,
        )?
        .ok_or(RestoreError::RestoreFailed)?;
        selections
            .push(NativeSseSnapshotSelection::new(
                selection.frame_index,
                selection.native,
            ))
            .map_err(RestoreError::capacity)?;
        projected
            .push(ProjectedItem {
                value,
                cached: None,
                missing: 0,
            })
            .map_err(RestoreError::capacity)?;
    }
    let delivered = PayloadJson::try_from_projection(
        JsonProjection::Input(projected.as_slice()),
        budget.clone(),
        budget.limit(),
    )
    .map_err(RestoreError::capacity)?;
    drop(projected);
    let mut digest = Sha256::new();
    digest.update(b"native-sse-staging-wire-v1\0");
    for frame in frames.as_slice() {
        digest.update((frame.encoded.len() as u64).to_le_bytes());
        digest.update(frame.encoded.as_slice());
    }
    lease
        .require_native_attempt_binding(observed.client(), original_binding)
        .map_err(RestoreError::scan_error)?;
    lease
        .require_native_attempt_binding(observed.client(), binding)
        .map_err(RestoreError::scan_error)?;
    Ok(StagedNativeSse {
        entity: observed.entity(),
        frames,
        _snapshot_selections: selections,
        delivered,
        restoration: match inspection {
            Some(owner) => NativeSseRestoration::Lifecycle(owner),
            None => NativeSseRestoration::Legacy {
                output_tokens,
                occurrences: restored_occurrences,
            },
        },
        changed,
        binding: original_binding,
        client: observed.client(),
        budget,
        release_digest: digest.finalize().into(),
    })
}

// Only the sole finalizer can construct this key; adapters cannot extract wire.
impl StagedNativeSse<'_> {
    pub(in crate::proxy::security) fn finalization_reader(
        &self,
        _key: &super::super::response_restoration::NativeSseFinalizationKey,
        max: usize,
    ) -> Result<impl std::io::Read + '_, RestoreError> {
        let total = self
            .frames
            .as_slice()
            .iter()
            .try_fold(0usize, |total, frame| {
                total
                    .checked_add(frame.encoded.len())
                    .filter(|total| *total <= max.min(1024 * 1024))
                    .ok_or(RestoreError::RestoreFailed)
            })?;
        if total == 0 {
            return Err(RestoreError::RestoreFailed);
        }
        struct Reader<'a> {
            frames: &'a [StagedNativeSseFrame<'a>],
            index: usize,
            offset: usize,
        }
        impl std::io::Read for Reader<'_> {
            fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
                if out.is_empty() {
                    return Ok(0);
                }
                while let Some(frame) = self.frames.get(self.index) {
                    let remaining = &frame.encoded.as_slice()[self.offset..];
                    if remaining.is_empty() {
                        self.index += 1;
                        self.offset = 0;
                        continue;
                    }
                    let count = remaining.len().min(out.len());
                    out[..count].copy_from_slice(&remaining[..count]);
                    self.offset += count;
                    return Ok(count);
                }
                Ok(0)
            }
        }
        Ok(Reader {
            frames: self.frames.as_slice(),
            index: 0,
            offset: 0,
        })
    }

    pub(in crate::proxy::security) fn into_finalization_frames(
        mut self,
        _key: &super::super::response_restoration::NativeSseFinalizationKey,
    ) -> Result<PayloadVec<super::super::payload_buffer::PayloadBuffer>, RestoreError> {
        let mut reversed = PayloadVec::new(self.budget.clone()).map_err(RestoreError::capacity)?;
        while let Some(frame) = self.frames.pop() {
            reversed
                .push(frame.encoded)
                .map_err(RestoreError::capacity)?;
        }
        Ok(reversed)
    }
}

#[cfg(test)]
#[path = "../tests/c5_native_sse_staging_tests.rs"]
mod c5_native_sse_staging_tests;
