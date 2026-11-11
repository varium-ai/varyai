//! Borrowed native JSON inspection and private staging; no completion or publication authority.
use serde_json::Value;

use super::super::{
    completed_context::ActualAttemptBinding,
    payload_budget::PayloadBudget,
    payload_json::PayloadJson,
    payload_metadata::PayloadVec,
    payload_text::PayloadText,
    response_restoration::RestoreError,
    restoration_contract::{ClientProtocol, SourceClass, TextPolicy, TextSites},
    tokenization::{DlpRequestLease, TokenError},
};

/// Bounded pointer-identity search; never compare native values as a substitute.
fn contains_native_node(
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
                if contains_native_node(item, target, depth + 1, remaining_work)? {
                    return Ok(true);
                }
            }
        }
        Value::Object(fields) => {
            for value in fields.values() {
                if contains_native_node(value, target, depth + 1, remaining_work)? {
                    return Ok(true);
                }
            }
        }
        _ => {}
    }
    Ok(false)
}

/// Adapter-selected native nodes are inspection claims, never proof of delivery.
/// Metadata is owned and charged; native strings and snapshots remain exact borrows.
pub(in crate::proxy::security) struct NativeJsonObservation<'native> {
    source: &'native Value,
    client: ClientProtocol,
    sites: TextSites<'native>,
    delivered_nodes: PayloadVec<&'native Value>,
    policies: Option<PayloadVec<TextPolicy>>,
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

        if client.protocol() == Protocol::Responses {
            return Err(RestoreError::RestoreUnsupported);
        }
        if delivered_nodes.is_empty() || delivered_nodes.len() > 128 {
            return Err(RestoreError::RestoreFailed);
        }
        let mut remaining_work = 4 * 1024 * 1024;
        for index in 0..sites.len() {
            let site = sites.site(index).ok_or(RestoreError::RestoreFailed)?;
            if !contains_native_node(source, site.native(), 0, &mut remaining_work)? {
                return Err(RestoreError::RestoreFailed);
            }
        }
        for node in delivered_nodes {
            if !contains_native_node(source, node, 0, &mut remaining_work)? {
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
            policies: None,
            budget,
        })
    }

    /// Explicit decoded-text policies in exact site order; metadata is charged.
    /// These are inspection claims, not completion or release authority.
    pub(in crate::proxy::security) fn new_with_policies(
        source: &'native Value,
        client: ClientProtocol,
        sites: TextSites<'native>,
        delivered_nodes: &[&'native Value],
        policies: &[TextPolicy],
        budget: PayloadBudget,
    ) -> Result<Self, RestoreError> {
        if policies.len() != sites.len() {
            return Err(RestoreError::RestoreFailed);
        }
        for (index, policy) in policies.iter().enumerate() {
            let site = sites.site(index).ok_or(RestoreError::RestoreFailed)?;
            if (site.source() == SourceClass::ToolArguments)
                != (*policy == TextPolicy::ToolArguments)
            {
                return Err(RestoreError::RestoreFailed);
            }
        }
        let mut observed = Self::new(source, client, sites, delivered_nodes, budget.clone())?;
        let mut retained = PayloadVec::new(budget).map_err(RestoreError::capacity)?;
        for policy in policies {
            retained.push(*policy).map_err(RestoreError::capacity)?;
        }
        observed.policies = Some(retained);
        Ok(observed)
    }

    fn policy(&self, index: usize) -> Result<TextPolicy, RestoreError> {
        if let Some(policies) = &self.policies {
            return policies
                .as_slice()
                .get(index)
                .copied()
                .ok_or(RestoreError::RestoreFailed);
        }
        let site = self.sites.site(index).ok_or(RestoreError::RestoreFailed)?;
        Ok(if site.source() == SourceClass::ToolArguments {
            TextPolicy::ToolArguments
        } else {
            TextPolicy::Output
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
    // Explicit policies and Gemini signed-unit checks require the lifecycle owner.
    if lifecycle.is_none()
        && (observed.policies.is_some()
            || observed.client().protocol() == crate::proxy::protocol_bridge::Protocol::Gemini)
    {
        return Err(RestoreError::RestoreUnsupported);
    }
    // Exactly one mechanics owner supplies both edits and dependencies. Finish is
    // inspection only; the finalizer still consumes the genuine HTTP completion proof.
    let restoration = if let Some((pair, attempt)) = lifecycle {
        use super::super::restoration_contract::{RestoreDecision, SourceClass};
        use crate::proxy::protocol_bridge::Protocol;

        if pair.client() != observed.client() {
            return Err(RestoreError::RestoreFailed);
        }
        // Shared JSON mechanics are distinct from lane registration and route enablement.
        let protocol = pair.client().protocol();
        if protocol != pair.upstream().protocol()
            || !matches!(
                protocol,
                Protocol::Chat | Protocol::Anthropic | Protocol::Gemini
            )
        {
            return Err(RestoreError::RestoreUnsupported);
        }
        if protocol == Protocol::Anthropic {
            let blocks = observed
                .source()
                .get("content")
                .and_then(Value::as_array)
                .ok_or(RestoreError::RestoreFailed)?;
            if blocks.len() != observed.delivered_nodes().len()
                || !blocks
                    .iter()
                    .zip(observed.delivered_nodes())
                    .all(|(block, delivered)| std::ptr::eq(block, *delivered))
            {
                return Err(RestoreError::RestoreFailed);
            }
        } else if protocol == Protocol::Gemini {
            let candidates = observed
                .source()
                .get("candidates")
                .and_then(Value::as_array)
                .ok_or(RestoreError::RestoreFailed)?;
            if candidates.len() != observed.delivered_nodes().len()
                || !candidates.iter().zip(observed.delivered_nodes()).all(
                    |(candidate, delivered)| {
                        candidate
                            .get("content")
                            .is_some_and(|content| std::ptr::eq(content, *delivered))
                    },
                )
            {
                return Err(RestoreError::RestoreFailed);
            }
            let mut remaining_work = 4 * 1024 * 1024;
            for candidate in candidates {
                let parts = candidate
                    .get("content")
                    .and_then(|content| content.get("parts"))
                    .and_then(Value::as_array)
                    .ok_or(RestoreError::RestoreFailed)?;
                for part in parts {
                    if part.get("thoughtSignature").is_none() {
                        continue;
                    }
                    for index in 0..observed.sites().len() {
                        let site = observed
                            .sites()
                            .site(index)
                            .ok_or(RestoreError::RestoreFailed)?;
                        if contains_native_node(part, site.native(), 0, &mut remaining_work)?
                            && site.constraint() != EditConstraint::Immutable
                        {
                            return Err(RestoreError::RestoreFailed);
                        }
                    }
                }
            }
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
            let before = owner.restored_occurrences();
            // Chat carries a complete JSON-object string; native object protocols
            // expose decoded string leaves and must never reparse/stringify a leaf.
            let decision =
                if site.source() == SourceClass::ToolArguments && protocol == Protocol::Chat {
                    owner.hold_all()?;
                    let restored = owner.arguments(site.text())?;
                    if restored.as_str() == site.text() {
                        RestoreDecision::Unchanged
                    } else {
                        RestoreDecision::Patch(restored)
                    }
                } else {
                    // Immutable native text must still detect authorized references;
                    // Reasoning's literal-preservation policy cannot waive that check.
                    let policy = if matches!(protocol, Protocol::Anthropic | Protocol::Gemini)
                        && site.constraint() == EditConstraint::Immutable
                    {
                        TextPolicy::Output
                    } else {
                        observed.policy(index)?
                    };
                    let id = owner.open_text(policy)?;
                    owner.done(&id, site.text())?
                };
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
        let Some(data_len) = data_len else {
            if source.is_some() || sites.len() != 0 {
                return Err(RestoreError::RestoreFailed);
            }
            // A no-data frame carries bytes only, never a native source or completion fact.
            return Ok(Self { raw, source, sites });
        };
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
    chat_choice: bool,
}

impl<'native> NativeSseSnapshotSelection<'native> {
    pub(in crate::proxy::security) fn new(frame_index: usize, native: &'native Value) -> Self {
        Self {
            frame_index,
            native,
            chat_choice: false,
        }
    }
    /// Explicit native Chat choice selection; carries no completion authority.
    pub(in crate::proxy::security) fn chat_choice(
        frame_index: usize,
        native: &'native Value,
    ) -> Self {
        Self {
            chat_choice: true,
            ..Self::new(frame_index, native)
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
    composed_chat_snapshots: bool,
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

    pub(in crate::proxy::security) fn composed_chat_snapshots(&self) -> bool {
        self.composed_chat_snapshots
    }

    pub(in crate::proxy::security) fn release_digest(&self) -> [u8; 32] {
        self.release_digest
    }
}

/// Adapter-declared fragments of one decoded native text stream; not completion evidence.
pub(in crate::proxy::security) struct NativeSseTextStream<'native> {
    fragments: &'native [&'native Value],
    policy: Option<TextPolicy>,
}
impl<'native> NativeSseTextStream<'native> {
    pub(in crate::proxy::security) fn new(fragments: &'native [&'native Value]) -> Self {
        Self {
            fragments,
            policy: None,
        }
    }

    /// One explicit policy for the entire decoded stream, including split references.
    pub(in crate::proxy::security) fn with_policy(
        fragments: &'native [&'native Value],
        policy: TextPolicy,
    ) -> Self {
        Self {
            fragments,
            policy: Some(policy),
        }
    }
}

// Derive stream identity only from exact original nodes, never adapter grouping claims.
#[derive(Clone, Copy, PartialEq, Eq)]
enum NativeSseStreamCarrier {
    ImmutableThinking,
    Text(&'static str),
    Arguments(u64),
    // Internal exact-leaf inspection identity, never a protocol/tool-call ID.
    NativeObjectLeaf(u64),
    // Per-observation paths isolate Gemini candidates and Parts. These ordinals are
    // never protocol Part IDs and cannot correlate separate whole-Content frames.
    GeminiText {
        observation: usize,
        part: usize,
        signed: bool,
    },
    GeminiArgumentLeaf {
        observation: usize,
        part: usize,
        leaf: u64,
        signed: bool,
    },
}

fn visit_chat_sse_carriers<'native>(
    frame: &NativeSseFrameObservation<'native>,
    remaining: &mut usize,
    mut visit: impl FnMut((u64, NativeSseStreamCarrier), &'native Value) -> Result<(), RestoreError>,
) -> Result<(), RestoreError> {
    let choices = frame
        .source()
        .and_then(|source| source.get("choices"))
        .and_then(Value::as_array)
        .filter(|choices| choices.len() <= 128)
        .ok_or(RestoreError::RestoreFailed)?;
    for (position, choice) in choices.iter().enumerate() {
        NativeSseFrameObservation::charge_work(remaining, 1 + position)?;
        let index = choice
            .get("index")
            .and_then(Value::as_u64)
            .filter(|index| *index < 128)
            .ok_or(RestoreError::RestoreFailed)?;
        if choices[..position]
            .iter()
            .any(|prior| prior.get("index").and_then(Value::as_u64) == Some(index))
        {
            return Err(RestoreError::RestoreFailed);
        }
        let delta = choice
            .get("delta")
            .and_then(Value::as_object)
            .ok_or(RestoreError::RestoreFailed)?;
        for field in ["content", "refusal", "reasoning", "reasoning_content"] {
            NativeSseFrameObservation::charge_work(remaining, 1 + field.len())?;
            if let Some(value) = delta.get(field).filter(|value| value.is_string()) {
                visit((index, NativeSseStreamCarrier::Text(field)), value)?;
            }
        }
        if let Some(tools) = delta.get("tool_calls").filter(|value| !value.is_null()) {
            let tools = tools
                .as_array()
                .filter(|tools| tools.len() <= 128)
                .ok_or(RestoreError::RestoreFailed)?;
            for (position, tool) in tools.iter().enumerate() {
                NativeSseFrameObservation::charge_work(remaining, 1 + position)?;
                let tool_index = tool
                    .get("index")
                    .and_then(Value::as_u64)
                    .filter(|index| *index < 128)
                    .ok_or(RestoreError::RestoreFailed)?;
                if tools[..position]
                    .iter()
                    .any(|prior| prior.get("index").and_then(Value::as_u64) == Some(tool_index))
                {
                    return Err(RestoreError::RestoreFailed);
                }
                if let Some(value) = tool
                    .get("function")
                    .and_then(|function| function.get("arguments"))
                    .filter(|value| value.is_string())
                {
                    visit(
                        (index, NativeSseStreamCarrier::Arguments(tool_index)),
                        value,
                    )?;
                }
            }
        }
    }
    Ok(())
}

/// Derive Anthropic stream identity from the protocol block index and exact carrier.
/// Signatures and redacted payloads are opaque, never decoded text or argument sites.
fn visit_anthropic_sse_carriers<'native>(
    frame: &NativeSseFrameObservation<'native>,
    remaining: &mut usize,
    mut visit: impl FnMut((u64, NativeSseStreamCarrier), &'native Value) -> Result<(), RestoreError>,
) -> Result<(), RestoreError> {
    let source = frame.source().ok_or(RestoreError::RestoreFailed)?;
    let kind = source
        .get("type")
        .and_then(Value::as_str)
        .ok_or(RestoreError::RestoreFailed)?;
    let (carrier, node) = match kind {
        "content_block_start" => match source
            .pointer("/content_block/type")
            .and_then(Value::as_str)
        {
            Some("text") => (
                NativeSseStreamCarrier::Text("text"),
                source.pointer("/content_block/text"),
            ),
            Some("thinking") => (
                NativeSseStreamCarrier::ImmutableThinking,
                source.pointer("/content_block/thinking"),
            ),
            Some("tool_use") => {
                fn leaves<'a>(
                    node: &'a Value,
                    ordinal: &mut u64,
                    depth: usize,
                    remaining: &mut usize,
                    visit: &mut impl FnMut(u64, &'a Value) -> Result<(), RestoreError>,
                ) -> Result<(), RestoreError> {
                    NativeSseFrameObservation::charge_work(remaining, 1)?;
                    if depth > 128 {
                        return Err(RestoreError::RestoreFailed);
                    }
                    match node {
                        Value::String(_) => {
                            visit(*ordinal, node)?;
                            *ordinal = ordinal.checked_add(1).ok_or(RestoreError::RestoreFailed)?;
                        }
                        Value::Array(values) => {
                            for value in values {
                                leaves(value, ordinal, depth + 1, remaining, visit)?;
                            }
                        }
                        Value::Object(values) => {
                            for value in values.values() {
                                leaves(value, ordinal, depth + 1, remaining, visit)?;
                            }
                        }
                        _ => {}
                    }
                    Ok(())
                }
                let index = source
                    .get("index")
                    .and_then(Value::as_u64)
                    .filter(|index| *index < 4096)
                    .ok_or(RestoreError::RestoreFailed)?;
                let input = source
                    .pointer("/content_block/input")
                    .filter(|value| value.is_object())
                    .ok_or(RestoreError::RestoreFailed)?;
                return leaves(input, &mut 0, 0, remaining, &mut |ordinal, node| {
                    visit(
                        (index, NativeSseStreamCarrier::NativeObjectLeaf(ordinal)),
                        node,
                    )
                });
            }
            Some("redacted_thinking") => return Ok(()),
            _ => return Err(RestoreError::RestoreUnsupported),
        },
        "content_block_delta" => match source.pointer("/delta/type").and_then(Value::as_str) {
            Some("text_delta") => (
                NativeSseStreamCarrier::Text("text"),
                source.pointer("/delta/text"),
            ),
            Some("thinking_delta") => (
                NativeSseStreamCarrier::ImmutableThinking,
                source.pointer("/delta/thinking"),
            ),
            Some("input_json_delta") => (
                NativeSseStreamCarrier::Arguments(0),
                source.pointer("/delta/partial_json"),
            ),
            Some("signature_delta") => return Ok(()),
            _ => return Err(RestoreError::RestoreUnsupported),
        },
        "message_start" | "content_block_stop" | "message_delta" | "message_stop" | "ping" => {
            return Ok(())
        }
        _ => return Err(RestoreError::RestoreUnsupported),
    };
    let index = source
        .get("index")
        .and_then(Value::as_u64)
        .filter(|index| *index < 4096)
        .ok_or(RestoreError::RestoreFailed)?;
    let node = node
        .filter(|node| node.is_string())
        .ok_or(RestoreError::RestoreFailed)?;
    NativeSseFrameObservation::charge_work(remaining, 1)?;
    visit((index, carrier), node)
}

/// Visit only the reviewed append-only whole-Content Gemini subset. The observation
/// ordinal is local inspection state, never a cross-frame Part identity.
fn visit_gemini_sse_carriers<'native>(
    observation: usize,
    frame: &NativeSseFrameObservation<'native>,
    remaining: &mut usize,
    mut visit: impl FnMut((u64, NativeSseStreamCarrier), &'native Value) -> Result<(), RestoreError>,
) -> Result<(), RestoreError> {
    #[derive(Clone, Copy)]
    struct LeafPath {
        observation: usize,
        part: usize,
        signed: bool,
        candidate: u64,
    }

    fn leaves<'a>(
        node: &'a Value,
        path: LeafPath,
        ordinal: &mut u64,
        depth: usize,
        remaining: &mut usize,
        visit: &mut impl FnMut((u64, NativeSseStreamCarrier), &'a Value) -> Result<(), RestoreError>,
    ) -> Result<(), RestoreError> {
        NativeSseFrameObservation::charge_work(remaining, 1)?;
        if depth > 128 {
            return Err(RestoreError::RestoreFailed);
        }
        match node {
            Value::String(_) => {
                visit(
                    (
                        path.candidate,
                        NativeSseStreamCarrier::GeminiArgumentLeaf {
                            observation: path.observation,
                            part: path.part,
                            leaf: *ordinal,
                            signed: path.signed,
                        },
                    ),
                    node,
                )?;
                *ordinal = ordinal.checked_add(1).ok_or(RestoreError::RestoreFailed)?;
            }
            Value::Array(values) => {
                for value in values {
                    leaves(value, path, ordinal, depth + 1, remaining, visit)?;
                }
            }
            Value::Object(values) => {
                for value in values.values() {
                    leaves(value, path, ordinal, depth + 1, remaining, visit)?;
                }
            }
            _ => {}
        }
        Ok(())
    }

    let source = frame.source().ok_or(RestoreError::RestoreUnsupported)?;
    let Some(candidates) = source.get("candidates") else {
        return Ok(());
    };
    let candidates = candidates
        .as_array()
        .filter(|candidates| candidates.len() <= 128)
        .ok_or(RestoreError::RestoreFailed)?;
    for (position, candidate) in candidates.iter().enumerate() {
        NativeSseFrameObservation::charge_work(remaining, 1 + position)?;
        let candidate_index = candidate
            .get("index")
            .and_then(Value::as_u64)
            .filter(|index| *index < 128)
            .ok_or(RestoreError::RestoreFailed)?;
        if candidates[..position]
            .iter()
            .any(|prior| prior.get("index").and_then(Value::as_u64) == Some(candidate_index))
        {
            return Err(RestoreError::RestoreFailed);
        }
        let Some(content) = candidate.get("content") else {
            if candidate.get("finishReason").and_then(Value::as_str) == Some("STOP") {
                continue;
            }
            return Err(RestoreError::RestoreFailed);
        };
        let mut unsupported = false;
        super::super::completion_json::validate_gemini_content(content, &mut unsupported).map_err(
            |_| {
                if unsupported {
                    RestoreError::RestoreUnsupported
                } else {
                    RestoreError::RestoreFailed
                }
            },
        )?;
        let parts = content
            .get("parts")
            .and_then(Value::as_array)
            .ok_or(RestoreError::RestoreFailed)?;
        for (part_index, part) in parts.iter().enumerate() {
            NativeSseFrameObservation::charge_work(remaining, 1 + part_index)?;
            let signed =
                part.get("thoughtSignature").is_some() || part.get("thought_signature").is_some();
            if let Some(text) = part.get("text") {
                visit(
                    (
                        candidate_index,
                        NativeSseStreamCarrier::GeminiText {
                            observation,
                            part: part_index,
                            signed,
                        },
                    ),
                    text,
                )?;
            }
            if let Some(arguments) = part.pointer("/functionCall/args") {
                leaves(
                    arguments,
                    LeafPath {
                        observation,
                        part: part_index,
                        signed,
                        candidate: candidate_index,
                    },
                    &mut 0,
                    0,
                    remaining,
                    &mut visit,
                )?;
            }
        }
    }
    Ok(())
}

fn visit_native_sse_carriers<'native>(
    observation: usize,
    frame: &NativeSseFrameObservation<'native>,
    client: ClientProtocol,
    remaining: &mut usize,
    visit: impl FnMut((u64, NativeSseStreamCarrier), &'native Value) -> Result<(), RestoreError>,
) -> Result<(), RestoreError> {
    use crate::proxy::protocol_bridge::Protocol;
    match client.protocol() {
        Protocol::Chat => visit_chat_sse_carriers(frame, remaining, visit),
        Protocol::Anthropic => visit_anthropic_sse_carriers(frame, remaining, visit),
        Protocol::Gemini => visit_gemini_sse_carriers(observation, frame, remaining, visit),
        _ => Err(RestoreError::RestoreUnsupported),
    }
}

fn native_sse_stream_key(
    observation: usize,
    frame: &NativeSseFrameObservation<'_>,
    client: ClientProtocol,
    target: &Value,
    remaining: &mut usize,
) -> Result<(u64, NativeSseStreamCarrier), RestoreError> {
    let mut key = None;
    visit_native_sse_carriers(observation, frame, client, remaining, |candidate, node| {
        if std::ptr::eq(node, target) && key.replace(candidate).is_some() {
            return Err(RestoreError::RestoreFailed);
        }
        Ok(())
    })?;
    key.ok_or(RestoreError::RestoreFailed)
}

/// Compose decoded native streams with exact protocol carrier isolation.
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
    use super::super::restoration_contract::{
        EditConstraint, RestoreDecision, SourceClass, TextPolicy,
    };
    use super::super::tokenization::TokenError;
    use crate::proxy::protocol_bridge::Protocol;
    if pair.client() != observed.client() {
        return Err(RestoreError::RestoreFailed);
    }
    let protocol = pair.client().protocol();
    if protocol != pair.upstream().protocol()
        || !matches!(
            protocol,
            Protocol::Chat | Protocol::Anthropic | Protocol::Gemini
        )
    {
        return Err(RestoreError::RestoreUnsupported);
    }
    let mut sites = PayloadVec::new(budget.clone()).map_err(RestoreError::capacity)?;
    let mut stream_keys = PayloadVec::new(budget.clone()).map_err(RestoreError::capacity)?;
    let mut remaining_work = 4 * 1024 * 1024;
    for index in 0..observed.frame_count() {
        let frame = observed.frame(index).ok_or(RestoreError::RestoreFailed)?;
        for site_index in 0..frame.sites().len() {
            if sites.len() >= 128 {
                return Err(RestoreError::RestoreFailed);
            }
            let site = frame
                .sites()
                .site(site_index)
                .ok_or(RestoreError::RestoreFailed)?;
            let key = native_sse_stream_key(
                index,
                frame,
                observed.client(),
                site.native(),
                &mut remaining_work,
            )?;
            if (site.source() == SourceClass::ToolArguments)
                != matches!(
                    key.1,
                    NativeSseStreamCarrier::Arguments(_)
                        | NativeSseStreamCarrier::NativeObjectLeaf(_)
                        | NativeSseStreamCarrier::GeminiArgumentLeaf { .. }
                )
            {
                return Err(RestoreError::RestoreFailed);
            }
            sites.push(site).map_err(RestoreError::capacity)?;
            stream_keys.push(key).map_err(RestoreError::capacity)?;
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
        let mut source = None;
        let mut stream_key = None;
        for native in stream.fragments {
            let index = sites
                .as_slice()
                .iter()
                .position(|site| std::ptr::eq(site.native(), *native))
                .ok_or(RestoreError::RestoreFailed)?;
            let classification = sites.as_slice()[index].source();
            if stream.policy.is_some_and(|policy| {
                (classification == SourceClass::ToolArguments)
                    != (policy == TextPolicy::ToolArguments)
            }) {
                return Err(RestoreError::RestoreFailed);
            }
            if stream_key.is_some_and(|key| key != stream_keys.as_slice()[index])
                || source.is_some_and(|source| source != classification)
                || previous.is_some_and(|previous| index <= previous)
                || selected.as_slice().contains(&index)
            {
                return Err(RestoreError::RestoreFailed);
            }
            selected.push(index).map_err(RestoreError::capacity)?;
            source = Some(classification);
            stream_key = Some(stream_keys.as_slice()[index]);
            previous = Some(index);
        }
        // Require the complete original carrier, including sites an adapter omitted.
        // Exact per-carrier order permits interleaving with other choices and sparse calls.
        let key = stream_key.ok_or(RestoreError::RestoreFailed)?;
        let mut fragment = 0usize;
        for frame_index in 0..observed.frame_count() {
            NativeSseFrameObservation::charge_work(&mut remaining_work, 1)?;
            let frame = observed
                .frame(frame_index)
                .ok_or(RestoreError::RestoreFailed)?;
            if frame.source().is_some() {
                visit_native_sse_carriers(
                    frame_index,
                    frame,
                    observed.client(),
                    &mut remaining_work,
                    |candidate_key, node| {
                        if candidate_key == key {
                            if !stream
                                .fragments
                                .get(fragment)
                                .is_some_and(|selected| std::ptr::eq(*selected, node))
                            {
                                return Err(RestoreError::RestoreFailed);
                            }
                            fragment += 1;
                        }
                        Ok(())
                    },
                )?;
            }
        }
        if fragment != stream.fragments.len() {
            return Err(RestoreError::RestoreFailed);
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
        let first = sites
            .as_slice()
            .iter()
            .position(|site| std::ptr::eq(site.native(), stream.fragments[0]))
            .ok_or(RestoreError::RestoreFailed)?;
        let argument_stream = matches!(
            stream_keys.as_slice()[first].1,
            NativeSseStreamCarrier::Arguments(_)
        );
        // Argument fragments never enter ordinary text matching: decode the complete
        // object through the existing lifecycle transaction, with one shared ledger.
        // Anthropic thinking and every signed Gemini Part are immutable independent
        // of adapter policy. Gemini's per-observation key cannot merge signed units.
        let immutable_stream = matches!(protocol, Protocol::Anthropic | Protocol::Gemini)
            && stream.fragments.iter().any(|node| {
                sites.as_slice().iter().enumerate().any(|(index, site)| {
                    if !std::ptr::eq(site.native(), *node) {
                        return false;
                    }
                    let carrier = stream_keys.as_slice()[index].1;
                    site.constraint() == EditConstraint::Immutable
                        || (protocol == Protocol::Anthropic
                            && carrier == NativeSseStreamCarrier::ImmutableThinking)
                        || (protocol == Protocol::Gemini
                            && matches!(
                                carrier,
                                NativeSseStreamCarrier::GeminiText { signed: true, .. }
                                    | NativeSseStreamCarrier::GeminiArgumentLeaf {
                                        signed: true,
                                        ..
                                    }
                            ))
                })
            });
        let id = if argument_stream {
            None
        } else {
            Some(owner.open_text(if immutable_stream {
                TextPolicy::Output
            } else {
                stream.policy.unwrap_or(TextPolicy::Output)
            })?)
        };
        let before = owner.restored_occurrences();
        for node in stream.fragments {
            let text = node.as_str().ok_or(RestoreError::RestoreFailed)?;
            raw.append(text).map_err(RestoreError::capacity)?;
            let decision = match &id {
                Some(id) => owner.delta(id, text)?,
                None => RestoreDecision::Hold,
            };
            let bytes = match decision {
                RestoreDecision::Unchanged => text.len(),
                RestoreDecision::Hold => 0,
                RestoreDecision::Patch(text) => text.as_str().len(),
            };
            emitted.push(bytes).map_err(RestoreError::capacity)?;
        }
        let decision = match &id {
            Some(id) => owner.done(id, raw.as_str())?,
            None => {
                let restored = owner.arguments(raw.as_str())?;
                if restored.as_str() == raw.as_str() {
                    RestoreDecision::Unchanged
                } else {
                    RestoreDecision::Patch(restored)
                }
            }
        };
        if immutable_stream && owner.restored_occurrences() != before {
            return Err(RestoreError::RestoreFailed);
        }
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
    if matches!(
        observed.client().protocol(),
        crate::proxy::protocol_bridge::Protocol::Anthropic
            | crate::proxy::protocol_bridge::Protocol::Gemini
    ) {
        return Err(RestoreError::RestoreUnsupported);
    }
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

    let composed_chat_snapshots = snapshots[0].chat_choice;
    if snapshots
        .iter()
        .any(|selection| selection.chat_choice != composed_chat_snapshots)
    {
        return Err(RestoreError::RestoreFailed);
    }
    if composed_chat_snapshots {
        use crate::proxy::protocol_bridge::Protocol;
        if lifecycle.is_none() || observed.client().protocol() != Protocol::Chat {
            return Err(RestoreError::RestoreUnsupported);
        }
        let mut selected = 0usize;
        for frame_index in 0..observed.frame_count() {
            let frame = observed
                .frame(frame_index)
                .ok_or(RestoreError::RestoreFailed)?;
            if let Some(source) = frame.source() {
                let choices = source
                    .get("choices")
                    .and_then(Value::as_array)
                    .ok_or(RestoreError::RestoreFailed)?;
                for choice in choices {
                    let selection = snapshots.get(selected).ok_or(RestoreError::RestoreFailed)?;
                    if selection.frame_index != frame_index
                        || !std::ptr::eq(selection.native, choice)
                    {
                        return Err(RestoreError::RestoreFailed);
                    }
                    selected += 1;
                }
            }
        }
        if selected != snapshots.len() {
            return Err(RestoreError::RestoreFailed);
        }
    }

    // Native Anthropic history is the complete ordered block sequence, not an
    // adapter-selected event slice. Require every data frame's exact root once.
    let composed_anthropic_snapshots = lifecycle.is_some()
        && observed.client().protocol() == crate::proxy::protocol_bridge::Protocol::Anthropic;
    if composed_anthropic_snapshots {
        let mut selected = 0usize;
        for frame_index in 0..observed.frame_count() {
            if let Some(source) = observed
                .frame(frame_index)
                .ok_or(RestoreError::RestoreFailed)?
                .source()
            {
                let selection = snapshots.get(selected).ok_or(RestoreError::RestoreFailed)?;
                if selection.chat_choice
                    || selection.frame_index != frame_index
                    || !std::ptr::eq(selection.native, source)
                {
                    return Err(RestoreError::RestoreFailed);
                }
                selected += 1;
            }
        }
        if selected != snapshots.len() {
            return Err(RestoreError::RestoreFailed);
        }
    }

    // Gemini history is the exact ordered candidate observation sequence. Usage-only
    // frames have no selection; every observed candidate root is selected exactly once.
    let composed_gemini_snapshots = lifecycle.is_some()
        && observed.client().protocol() == crate::proxy::protocol_bridge::Protocol::Gemini;
    if composed_gemini_snapshots {
        let mut selected = 0usize;
        for frame_index in 0..observed.frame_count() {
            let source = observed
                .frame(frame_index)
                .ok_or(RestoreError::RestoreFailed)?
                .source()
                .ok_or(RestoreError::RestoreUnsupported)?;
            let Some(candidates) = source.get("candidates") else {
                continue;
            };
            let candidates = candidates
                .as_array()
                .filter(|candidates| candidates.len() <= 128)
                .ok_or(RestoreError::RestoreFailed)?;
            for candidate in candidates {
                let selection = snapshots.get(selected).ok_or(RestoreError::RestoreFailed)?;
                if selection.chat_choice
                    || selection.frame_index != frame_index
                    || !std::ptr::eq(selection.native, candidate)
                {
                    return Err(RestoreError::RestoreFailed);
                }
                selected += 1;
            }
        }
        if selected != snapshots.len() {
            return Err(RestoreError::RestoreFailed);
        }
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
            .push(NativeSseSnapshotSelection {
                frame_index: selection.frame_index,
                native: selection.native,
                chat_choice: selection.chat_choice,
            })
            .map_err(RestoreError::capacity)?;
        projected
            .push(ProjectedItem {
                value,
                cached: None,
                missing: 0,
            })
            .map_err(RestoreError::capacity)?;
    }
    let delivered = if composed_chat_snapshots {
        let mut choices = PayloadVec::new(budget.clone()).map_err(RestoreError::capacity)?;
        for item in projected.as_slice() {
            choices.push(item.value).map_err(RestoreError::capacity)?;
        }
        compose_chat_snapshots(choices.as_slice(), &budget)?
    } else if composed_anthropic_snapshots {
        let mut events = PayloadVec::new(budget.clone()).map_err(RestoreError::capacity)?;
        for item in projected.as_slice() {
            events.push(item.value).map_err(RestoreError::capacity)?;
        }
        compose_anthropic_snapshots(events.as_slice(), &budget)?
    } else if composed_gemini_snapshots {
        let mut candidates = PayloadVec::new(budget.clone()).map_err(RestoreError::capacity)?;
        for item in projected.as_slice() {
            candidates
                .push(item.value)
                .map_err(RestoreError::capacity)?;
        }
        compose_gemini_snapshots(candidates.as_slice(), &budget)?
    } else {
        PayloadJson::try_from_projection(
            JsonProjection::Input(projected.as_slice()),
            budget.clone(),
            budget.limit(),
        )
        .map_err(RestoreError::capacity)?
    };
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
        composed_chat_snapshots,
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

/// Compose complete Chat assistant/call snapshots from bounded, ordered choice borrows.
/// Deterministic projection only; strict terminal validation remains independent.
pub(in crate::proxy::security) fn compose_chat_snapshots(
    choices: &[&Value],
    budget: &PayloadBudget,
) -> Result<PayloadJson, RestoreError> {
    use super::super::{payload_buffer::PayloadBuffer, tokenization::TokenError};
    use std::io::Write;

    struct Encoder(PayloadBuffer);
    impl Write for Encoder {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0
                .append(bytes)
                .map_err(|_| std::io::Error::other("Native snapshot capacity exceeded."))?;
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    impl Encoder {
        fn raw(&mut self, bytes: &[u8]) -> Result<(), RestoreError> {
            self.0.append(bytes).map_err(RestoreError::capacity)
        }
        fn json(&mut self, value: &impl serde::Serialize) -> Result<(), RestoreError> {
            serde_json::to_writer(self, value)
                .map_err(|_| RestoreError::capacity(TokenError::CapacityExceeded))
        }
    }
    fn index(value: &Value) -> Result<u64, RestoreError> {
        value
            .get("index")
            .and_then(Value::as_u64)
            .filter(|index| *index < 128)
            .ok_or(RestoreError::RestoreFailed)
    }
    fn fields<'a>(
        values: &[&'a Value],
        budget: &PayloadBudget,
    ) -> Result<PayloadVec<&'a str>, RestoreError> {
        let mut keys = PayloadVec::new(budget.clone()).map_err(RestoreError::capacity)?;
        for value in values {
            for key in value.as_object().ok_or(RestoreError::RestoreFailed)?.keys() {
                if !keys.as_slice().contains(&key.as_str()) {
                    if keys.len() >= 128 {
                        return Err(RestoreError::capacity(TokenError::CapacityExceeded));
                    }
                    keys.push(key.as_str()).map_err(RestoreError::capacity)?;
                }
            }
        }
        Ok(keys)
    }
    fn opaque<'a>(
        values: &[&'a Value],
        field: &str,
        remaining: &mut usize,
    ) -> Result<&'a Value, RestoreError> {
        let mut selected = None;
        for value in values {
            NativeSseFrameObservation::charge_work(remaining, 1)?;
            NativeSseFrameObservation::charge_work(remaining, field.len())?;
            if let Some(value) = value.get(field) {
                // Self-comparison bounds the first value too, without retaining a copy.
                let prior = selected.unwrap_or(value);
                if !NativeSseFrameObservation::same_value(prior, value, 0, remaining)? {
                    return Err(RestoreError::RestoreFailed);
                }
                if selected.is_none() {
                    selected = Some(value);
                }
            }
        }
        selected.ok_or(RestoreError::RestoreFailed)
    }
    fn identity<'a>(values: &[&'a Value], field: &str) -> Result<&'a Value, RestoreError> {
        let mut selected = None;
        for value in values
            .iter()
            .filter_map(|value| value.get(field))
            .filter(|value| !value.is_null())
        {
            if selected.is_some_and(|prior: &Value| prior != value) {
                return Err(RestoreError::RestoreFailed);
            }
            selected = Some(value);
        }
        selected.ok_or(RestoreError::RestoreFailed)
    }
    fn text_field(
        out: &mut Encoder,
        values: &[&Value],
        field: &str,
        budget: &PayloadBudget,
        remaining: &mut usize,
    ) -> Result<(), RestoreError> {
        // Join JSON-encoded string interiors, preserving every decoded fragment.
        let active = values
            .iter()
            .filter_map(|value| value.get(field))
            .filter(|value| !value.is_null());
        if active.clone().all(Value::is_string) && active.clone().next().is_some() {
            out.raw(b"\"")?;
            for value in active {
                let encoded = super::super::response_restoration::serialize_json_with_budget(
                    value,
                    budget,
                    1024 * 1024,
                )?;
                let end = encoded
                    .len()
                    .checked_sub(1)
                    .ok_or(RestoreError::RestoreFailed)?;
                let interior = encoded
                    .as_slice()
                    .get(1..end)
                    .ok_or(RestoreError::RestoreFailed)?;
                out.raw(interior)?;
            }
            out.raw(b"\"")
        } else if active.clone().next().is_none() {
            out.raw(b"null")
        } else if field == "reasoning" {
            out.json(opaque(values, field, remaining)?)
        } else {
            Err(RestoreError::RestoreFailed)
        }
    }
    fn call(
        out: &mut Encoder,
        values: &[&Value],
        budget: &PayloadBudget,
        remaining: &mut usize,
    ) -> Result<(), RestoreError> {
        out.raw(b"{")?;
        let keys = fields(values, budget)?;
        let mut first = true;
        for key in keys
            .as_slice()
            .iter()
            .copied()
            .filter(|key| *key != "index")
        {
            if !first {
                out.raw(b",")?;
            }
            first = false;
            out.json(&key)?;
            out.raw(b":")?;
            if key == "function" {
                let mut functions =
                    PayloadVec::new(budget.clone()).map_err(RestoreError::capacity)?;
                for value in values {
                    if let Some(function) = value.get("function") {
                        functions.push(function).map_err(RestoreError::capacity)?;
                    }
                }
                out.raw(b"{")?;
                let keys = fields(functions.as_slice(), budget)?;
                for (position, key) in keys.as_slice().iter().enumerate() {
                    if position != 0 {
                        out.raw(b",")?;
                    }
                    out.json(key)?;
                    out.raw(b":")?;
                    if *key == "arguments" {
                        text_field(out, functions.as_slice(), key, budget, remaining)?;
                    } else if *key == "name" {
                        out.json(identity(functions.as_slice(), key)?)?;
                    } else {
                        out.json(opaque(functions.as_slice(), key, remaining)?)?;
                    }
                }
                out.raw(b"}")?;
            } else if matches!(key, "id" | "type") {
                out.json(identity(values, key)?)?;
            } else {
                out.json(opaque(values, key, remaining)?)?;
            }
        }
        out.raw(b"}")
    }
    if choices.is_empty() || choices.len() > 128 {
        return Err(RestoreError::RestoreFailed);
    }
    let mut remaining_work = 4 * 1024 * 1024;
    let mut order = PayloadVec::new(budget.clone()).map_err(RestoreError::capacity)?;
    for choice in choices {
        let index = index(choice)?;
        if !order.as_slice().contains(&index) {
            order.push(index).map_err(RestoreError::capacity)?;
        }
    }
    let mut out =
        Encoder(PayloadBuffer::new(budget.clone(), 1024 * 1024).map_err(RestoreError::capacity)?);
    out.raw(b"[")?;
    let mut snapshot_count = 0usize;
    for choice_index in order.as_slice() {
        let mut deltas = PayloadVec::new(budget.clone()).map_err(RestoreError::capacity)?;
        for choice in choices {
            if index(choice)? == *choice_index {
                let delta = choice
                    .get("delta")
                    .filter(|delta| delta.is_object())
                    .ok_or(RestoreError::RestoreFailed)?;
                deltas.push(delta).map_err(RestoreError::capacity)?;
            }
        }
        let mut tool_order = PayloadVec::new(budget.clone()).map_err(RestoreError::capacity)?;
        let mut tools = PayloadVec::new(budget.clone()).map_err(RestoreError::capacity)?;
        for delta in deltas.as_slice() {
            if let Some(calls) = delta.get("tool_calls").filter(|value| !value.is_null()) {
                for tool in calls.as_array().ok_or(RestoreError::RestoreFailed)? {
                    let index = index(tool)?;
                    if !tool_order.as_slice().contains(&index) {
                        tool_order.push(index).map_err(RestoreError::capacity)?;
                    }
                    if tools.len() >= 128 {
                        return Err(RestoreError::capacity(TokenError::CapacityExceeded));
                    }
                    tools.push(tool).map_err(RestoreError::capacity)?;
                }
            }
        }
        if snapshot_count != 0 {
            out.raw(b",")?;
        }
        snapshot_count += 1 + tool_order.len();
        if snapshot_count > 128 {
            return Err(RestoreError::capacity(TokenError::CapacityExceeded));
        }
        out.raw(b"{\"role\":\"assistant\"")?;
        let keys = fields(deltas.as_slice(), budget)?;
        for key in keys
            .as_slice()
            .iter()
            .copied()
            .filter(|key| !matches!(*key, "role" | "tool_calls"))
        {
            out.raw(b",")?;
            out.json(&key)?;
            out.raw(b":")?;
            if matches!(
                key,
                "content" | "refusal" | "reasoning_content" | "reasoning"
            ) {
                text_field(
                    &mut out,
                    deltas.as_slice(),
                    key,
                    budget,
                    &mut remaining_work,
                )?;
            } else {
                out.json(opaque(deltas.as_slice(), key, &mut remaining_work)?)?;
            }
        }
        if !tool_order.as_slice().is_empty() {
            out.raw(b",\"tool_calls\":[")?;
            for (position, tool_index) in tool_order.as_slice().iter().enumerate() {
                if position != 0 {
                    out.raw(b",")?;
                }
                let mut fragments =
                    PayloadVec::new(budget.clone()).map_err(RestoreError::capacity)?;
                for tool in tools.as_slice() {
                    if index(tool)? == *tool_index {
                        fragments.push(*tool).map_err(RestoreError::capacity)?;
                    }
                }
                call(&mut out, fragments.as_slice(), budget, &mut remaining_work)?;
            }
            out.raw(b"]")?;
        }
        out.raw(b"}")?;
        for tool_index in tool_order.as_slice() {
            out.raw(b",")?;
            let mut fragments = PayloadVec::new(budget.clone()).map_err(RestoreError::capacity)?;
            for tool in tools.as_slice() {
                if index(tool)? == *tool_index {
                    fragments.push(*tool).map_err(RestoreError::capacity)?;
                }
            }
            call(&mut out, fragments.as_slice(), budget, &mut remaining_work)?;
        }
    }
    out.raw(b"]")?;
    let raw = std::str::from_utf8(out.0.as_slice()).map_err(|_| RestoreError::RestoreFailed)?;
    let assembled = PayloadJson::try_from_str(raw, budget.clone(), budget.limit())
        .map_err(RestoreError::capacity)?;
    let snapshots = assembled
        .as_value()
        .as_array()
        .ok_or(RestoreError::RestoreFailed)?;
    let mut position = 0usize;
    for choice_index in order.as_slice() {
        let reason = choices
            .iter()
            .rev()
            .filter(|choice| choice.get("index").and_then(Value::as_u64) == Some(*choice_index))
            .find_map(|choice| choice.get("finish_reason").and_then(Value::as_str));
        let message = snapshots.get(position).ok_or(RestoreError::RestoreFailed)?;
        let mut unsupported = false;
        super::super::completion_json::validate_chat_message(
            message,
            reason,
            budget,
            &mut unsupported,
            true,
        )
        .map_err(RestoreError::capacity)?;
        let calls = message.get("tool_calls").and_then(Value::as_array);
        position += 1;
        if let Some(calls) = calls {
            for call in calls {
                if snapshots.get(position) != Some(call) {
                    return Err(RestoreError::RestoreFailed);
                }
                position += 1;
            }
        }
    }
    if position != snapshots.len() {
        return Err(RestoreError::RestoreFailed);
    }
    Ok(assembled)
}

/// Compose append-only native Gemini Content observations by explicit candidate index.
/// No Part is merged across frames; a late signed Part remains in its own Content.
/// This charged deterministic projection grants neither terminal nor transport proof.
pub(in crate::proxy::security) fn compose_gemini_snapshots(
    candidates: &[&Value],
    budget: &PayloadBudget,
) -> Result<PayloadJson, RestoreError> {
    use super::super::{payload_buffer::PayloadBuffer, tokenization::TokenError};
    use std::io::Write;

    struct Encoder(PayloadBuffer);
    impl Write for Encoder {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0
                .append(bytes)
                .map_err(|_| std::io::Error::other("Native snapshot capacity exceeded."))?;
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    impl Encoder {
        fn raw(&mut self, bytes: &[u8]) -> Result<(), RestoreError> {
            self.0.append(bytes).map_err(RestoreError::capacity)
        }
        fn json(&mut self, value: &impl serde::Serialize) -> Result<(), RestoreError> {
            serde_json::to_writer(self, value)
                .map_err(|_| RestoreError::capacity(TokenError::CapacityExceeded))
        }
    }
    fn index(candidate: &Value) -> Result<u64, RestoreError> {
        candidate
            .get("index")
            .and_then(Value::as_u64)
            .filter(|index| *index < 128)
            .ok_or(RestoreError::RestoreFailed)
    }

    if candidates.is_empty() || candidates.len() > 1024 {
        return Err(RestoreError::RestoreFailed);
    }
    let mut order = PayloadVec::new(budget.clone()).map_err(RestoreError::capacity)?;
    let mut content_count = 0usize;
    for candidate in candidates {
        let candidate_index = index(candidate)?;
        if !order.as_slice().contains(&candidate_index) {
            if order.len() >= 128 {
                return Err(RestoreError::RestoreFailed);
            }
            order
                .push(candidate_index)
                .map_err(RestoreError::capacity)?;
        }
        match candidate.get("content") {
            Some(content) => {
                let mut unsupported = false;
                super::super::completion_json::validate_gemini_content(content, &mut unsupported)
                    .map_err(|_| {
                    if unsupported {
                        RestoreError::RestoreUnsupported
                    } else {
                        RestoreError::RestoreFailed
                    }
                })?;
                content_count += 1;
                if content_count > 1024 {
                    return Err(RestoreError::RestoreFailed);
                }
            }
            None if candidate.get("finishReason").is_some() => {}
            None => return Err(RestoreError::RestoreFailed),
        }
    }
    let mut out =
        Encoder(PayloadBuffer::new(budget.clone(), 1024 * 1024).map_err(RestoreError::capacity)?);
    out.raw(b"[")?;
    for (position, candidate_index) in order.as_slice().iter().enumerate() {
        if position != 0 {
            out.raw(b",")?;
        }
        out.raw(b"{\"index\":")?;
        out.json(candidate_index)?;
        out.raw(b",\"contents\":[")?;
        let mut first = true;
        for candidate in candidates {
            if index(candidate)? != *candidate_index {
                continue;
            }
            if let Some(content) = candidate.get("content") {
                if !first {
                    out.raw(b",")?;
                }
                first = false;
                out.json(content)?;
            }
        }
        out.raw(b"]}")?;
    }
    out.raw(b"]")?;
    let raw = std::str::from_utf8(out.0.as_slice()).map_err(|_| RestoreError::RestoreFailed)?;
    PayloadJson::try_from_str(raw, budget.clone(), budget.limit()).map_err(RestoreError::capacity)
}

/// Compose complete native blocks from exact ordered event roots. This is a
/// charged history projection, never transport EOF or a completion capability.
pub(in crate::proxy::security) fn compose_anthropic_snapshots(
    events: &[&Value],
    budget: &PayloadBudget,
) -> Result<PayloadJson, RestoreError> {
    use super::super::{payload_buffer::PayloadBuffer, tokenization::TokenError};
    use std::io::Write;

    struct Encoder(PayloadBuffer);
    impl Write for Encoder {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0
                .append(bytes)
                .map_err(|_| std::io::Error::other("Native block capacity exceeded."))?;
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    impl Encoder {
        fn raw(&mut self, bytes: &[u8]) -> Result<(), RestoreError> {
            self.0.append(bytes).map_err(RestoreError::capacity)
        }
        fn json(&mut self, value: &impl serde::Serialize) -> Result<(), RestoreError> {
            serde_json::to_writer(self, value)
                .map_err(|_| RestoreError::capacity(TokenError::CapacityExceeded))
        }
    }
    fn joined(
        start: Option<&Value>,
        deltas: &[&Value],
        field: &str,
        budget: &PayloadBudget,
    ) -> Result<PayloadText, RestoreError> {
        let mut length = start
            .map(|value| {
                value
                    .as_str()
                    .map(str::len)
                    .ok_or(RestoreError::RestoreFailed)
            })
            .transpose()?
            .unwrap_or(0);
        for fragment in deltas.iter().filter_map(|delta| delta.get(field)) {
            length = length
                .checked_add(fragment.as_str().ok_or(RestoreError::RestoreFailed)?.len())
                .filter(|length| *length <= 1024 * 1024)
                .ok_or_else(|| RestoreError::capacity(TokenError::CapacityExceeded))?;
        }
        let mut text = PayloadText::with_capacity(length, budget.clone(), 1024 * 1024)
            .map_err(RestoreError::capacity)?;
        if let Some(start) = start {
            text.append(start.as_str().ok_or(RestoreError::RestoreFailed)?)
                .map_err(RestoreError::capacity)?;
        }
        for delta in deltas {
            if let Some(fragment) = delta.get(field) {
                text.append(fragment.as_str().ok_or(RestoreError::RestoreFailed)?)
                    .map_err(RestoreError::capacity)?;
            }
        }
        Ok(text)
    }
    fn block(
        start: &Value,
        deltas: &[&Value],
        budget: &PayloadBudget,
    ) -> Result<PayloadJson, RestoreError> {
        let kind = start
            .get("type")
            .and_then(Value::as_str)
            .ok_or(RestoreError::RestoreFailed)?;
        for delta in deltas {
            let valid = matches!(
                (kind, delta.get("type").and_then(Value::as_str)),
                ("text", Some("text_delta"))
                    | ("thinking", Some("thinking_delta" | "signature_delta"))
                    | ("tool_use", Some("input_json_delta"))
            );
            if !valid {
                return Err(RestoreError::RestoreFailed);
            }
        }
        let mut out = Encoder(
            PayloadBuffer::new(budget.clone(), 1024 * 1024).map_err(RestoreError::capacity)?,
        );
        out.raw(b"{")?;
        for (index, (key, value)) in start
            .as_object()
            .ok_or(RestoreError::RestoreFailed)?
            .iter()
            .enumerate()
        {
            if index != 0 {
                out.raw(b",")?;
            }
            out.json(key)?;
            out.raw(b":")?;
            if (kind == "text" && key == "text")
                || (kind == "thinking" && matches!(key.as_str(), "thinking" | "signature"))
            {
                let text = joined(Some(value), deltas, key, budget)?;
                out.json(&text.as_str())?;
            } else if kind == "tool_use" && key == "input" && !deltas.is_empty() {
                if !value.as_object().is_some_and(serde_json::Map::is_empty) {
                    return Err(RestoreError::RestoreFailed);
                }
                let arguments = joined(None, deltas, "partial_json", budget)?;
                let parsed =
                    PayloadJson::try_from_str(arguments.as_str(), budget.clone(), 1024 * 1024)
                        .map_err(RestoreError::capacity)?;
                if !parsed.as_value().is_object() {
                    return Err(RestoreError::RestoreFailed);
                }
                out.json(parsed.as_value())?;
            } else {
                out.json(value)?;
            }
        }
        if kind == "thinking"
            && start.get("signature").is_none()
            && deltas.iter().any(|delta| delta.get("signature").is_some())
        {
            out.raw(b",\"signature\":")?;
            let signature = joined(None, deltas, "signature", budget)?;
            out.json(&signature.as_str())?;
        }
        out.raw(b"}")?;
        let raw = std::str::from_utf8(out.0.as_slice()).map_err(|_| RestoreError::RestoreFailed)?;
        PayloadJson::try_from_str(raw, budget.clone(), 1024 * 1024).map_err(RestoreError::capacity)
    }
    let mut blocks = PayloadVec::new(budget.clone()).map_err(RestoreError::capacity)?;
    let mut open = None;
    let mut fragments = PayloadVec::new(budget.clone()).map_err(RestoreError::capacity)?;
    let mut next = 0u64;
    for event in events {
        match event.get("type").and_then(Value::as_str) {
            Some("content_block_start") => {
                if open.is_some()
                    || next >= 128
                    || event.get("index").and_then(Value::as_u64) != Some(next)
                {
                    return Err(RestoreError::RestoreFailed);
                }
                open = Some(
                    event
                        .get("content_block")
                        .ok_or(RestoreError::RestoreFailed)?,
                );
            }
            Some("content_block_delta") => {
                if open.is_none() || event.get("index").and_then(Value::as_u64) != Some(next) {
                    return Err(RestoreError::RestoreFailed);
                }
                fragments
                    .push(event.get("delta").ok_or(RestoreError::RestoreFailed)?)
                    .map_err(RestoreError::capacity)?;
            }
            Some("content_block_stop") => {
                if event.get("index").and_then(Value::as_u64) != Some(next) {
                    return Err(RestoreError::RestoreFailed);
                }
                let start = open.take().ok_or(RestoreError::RestoreFailed)?;
                let completed = block(start, fragments.as_slice(), budget)?;
                blocks.push(completed).map_err(RestoreError::capacity)?;
                fragments = PayloadVec::new(budget.clone()).map_err(RestoreError::capacity)?;
                next += 1;
            }
            Some("message_start" | "message_delta" | "message_stop" | "ping") => {}
            _ => return Err(RestoreError::RestoreUnsupported),
        }
    }
    if open.is_some() {
        return Err(RestoreError::RestoreFailed);
    }
    let mut out =
        Encoder(PayloadBuffer::new(budget.clone(), 1024 * 1024).map_err(RestoreError::capacity)?);
    out.raw(b"[")?;
    for (index, block) in blocks.as_slice().iter().enumerate() {
        if index != 0 {
            out.raw(b",")?;
        }
        out.json(block.as_value())?;
    }
    out.raw(b"]")?;
    let raw = std::str::from_utf8(out.0.as_slice()).map_err(|_| RestoreError::RestoreFailed)?;
    PayloadJson::try_from_str(raw, budget.clone(), 1024 * 1024).map_err(RestoreError::capacity)
}
