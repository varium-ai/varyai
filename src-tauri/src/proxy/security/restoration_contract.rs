//! Internal inspection contracts. No mapping, completion, or route authority.
use std::ops::Range;

use serde_json::Value;

use crate::proxy::protocol_bridge::Protocol;

use super::{
    payload_budget::PayloadBudget, payload_metadata::PayloadVec, payload_text::PayloadText,
    tokenization::TokenError,
};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) struct ClientProtocol(Protocol);
impl ClientProtocol {
    pub(super) fn new(protocol: Protocol) -> Self {
        Self(protocol)
    }
    pub(super) fn protocol(self) -> Protocol {
        self.0
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) struct UpstreamProtocol(Protocol);
impl UpstreamProtocol {
    pub(super) fn new(protocol: Protocol) -> Self {
        Self(protocol)
    }
    pub(super) fn protocol(self) -> Protocol {
        self.0
    }
    pub(super) fn from_format(format: &str) -> Option<Self> {
        Protocol::from_format(format).map(Self)
    }
}

#[derive(Clone, Copy)]
pub(super) struct ProtocolPair {
    client: ClientProtocol,
    upstream: UpstreamProtocol,
}
impl ProtocolPair {
    pub(super) fn new(client: ClientProtocol, upstream: UpstreamProtocol) -> Self {
        Self { client, upstream }
    }
    pub(super) fn client(self) -> ClientProtocol {
        self.client
    }
    pub(super) fn upstream(self) -> UpstreamProtocol {
        self.upstream
    }
}
/// Internal namespace classification, not a route grant or a client-selectable hint.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum IngressNamespace {
    Direct,
    Gateway,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum ContinuityScope {
    LegacyResponses,
    Native {
        client: ClientProtocol,
        ingress: IngressNamespace,
    },
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Framing {
    Json,
    Sse,
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Operation {
    Responses,
    ResponsesCompact,
    ChatCompletions,
    Messages,
    GenerateContent,
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum SourceClass {
    CurrentInput,
    HistoricalText,
    ToolArguments,
    ToolResult,
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum EditConstraint {
    Editable,
    Immutable,
}

/// Fixed classifications only; never includes native values or locations.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ContractError {
    ForeignSite,
    ImmutableSite,
    InvalidRange,
    OverlappingPatches,
    DuplicateSite,
    NonTextSite,
    Resource(TokenError),
}
impl std::fmt::Display for ContractError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("DLP contract error")
    }
}
impl std::error::Error for ContractError {}
impl From<TokenError> for ContractError {
    fn from(error: TokenError) -> Self {
        Self::Resource(error)
    }
}

struct ObservedSite<'native> {
    native: &'native Value,
    text: &'native str,
    source: SourceClass,
    constraint: EditConstraint,
}

// Bound observation deduplication and patch sorting independently of byte limits.
const MAX_SITES_AND_PATCHES: usize = 4096;

pub(super) struct TextSites<'native> {
    sites: PayloadVec<ObservedSite<'native>>,
    budget: PayloadBudget,
}
impl<'native> TextSites<'native> {
    pub(super) fn new(budget: PayloadBudget) -> Result<Self, ContractError> {
        Ok(Self {
            sites: PayloadVec::new(budget.clone())?,
            budget,
        })
    }
    pub(super) fn observe(
        &mut self,
        native: &'native Value,
        source: SourceClass,
        constraint: EditConstraint,
    ) -> Result<(), ContractError> {
        if self.sites.len() >= MAX_SITES_AND_PATCHES {
            return Err(TokenError::CapacityExceeded.into());
        }
        let text = native.as_str().ok_or(ContractError::NonTextSite)?;
        if self
            .sites
            .as_slice()
            .iter()
            .any(|site| std::ptr::eq(site.native, native))
        {
            return Err(ContractError::DuplicateSite);
        }
        self.sites.push(ObservedSite {
            native,
            text,
            source,
            constraint,
        })?;
        Ok(())
    }
    pub(super) fn len(&self) -> usize {
        self.sites.len()
    }
    pub(super) fn site(&self, index: usize) -> Option<TextSite<'_, 'native>> {
        self.sites
            .as_slice()
            .get(index)
            .map(|_| TextSite { owner: self, index })
    }
    /// Validate the entire proposal before producing any budget-owned edits.
    /// The plan cannot mutate native input and conveys no mapping authority.
    pub(super) fn plan(
        &self,
        patches: &[TextPatch<'_, 'native, '_>],
    ) -> Result<AtomicPatchPlan<'native>, ContractError> {
        if patches.len() > MAX_SITES_AND_PATCHES {
            return Err(TokenError::CapacityExceeded.into());
        }
        let mut ordered = PayloadVec::new(self.budget.clone())?;
        for patch in patches {
            if !std::ptr::eq(patch.site.owner, self) {
                return Err(ContractError::ForeignSite);
            }
            let site = self
                .sites
                .as_slice()
                .get(patch.site.index)
                .ok_or(ContractError::ForeignSite)?;
            if site.constraint != EditConstraint::Editable {
                return Err(ContractError::ImmutableSite);
            }
            if patch.range.start > patch.range.end || site.text.get(patch.range.clone()).is_none() {
                return Err(ContractError::InvalidRange);
            }
            ordered.push(patch)?;
        }
        ordered.sort_unstable_by(|left, right| {
            (left.site.index, left.range.start, left.range.end).cmp(&(
                right.site.index,
                right.range.start,
                right.range.end,
            ))
        });
        for pair in ordered.as_slice().windows(2) {
            let (left, right) = (pair[0], pair[1]);
            if left.site.index == right.site.index
                && (left.range.end > right.range.start || left.range.start == right.range.start)
            {
                return Err(ContractError::OverlappingPatches);
            }
        }
        let mut edits = PayloadVec::new(self.budget.clone())?;
        let mut remaining = ordered.as_slice();
        while let Some(first) = remaining.first() {
            let count = remaining
                .iter()
                .take_while(|patch| patch.site.index == first.site.index)
                .count();
            let (group, rest) = remaining.split_at(count);
            let site = &self.sites.as_slice()[first.site.index];
            let mut length = site.text.len();
            for patch in group {
                length = length
                    .checked_sub(patch.range.end - patch.range.start)
                    .and_then(|length| length.checked_add(patch.replacement.len()))
                    .ok_or(TokenError::CapacityExceeded)?;
            }
            let mut text =
                PayloadText::with_capacity(length, self.budget.clone(), 64 * 1024 * 1024)?;
            let mut cursor = 0;
            for patch in group {
                text.append(&site.text[cursor..patch.range.start])?;
                text.append(patch.replacement)?;
                cursor = patch.range.end;
            }
            text.append(&site.text[cursor..])?;
            if text.as_str() != site.text {
                edits.push(NativeTextEdit {
                    native: site.native,
                    text,
                })?;
            }
            remaining = rest;
        }
        Ok(AtomicPatchPlan { edits })
    }
}

/// The borrowed table and ordinal identify one native node; neither is authority.
#[derive(Clone, Copy)]
pub(super) struct TextSite<'sites, 'native> {
    owner: &'sites TextSites<'native>,
    index: usize,
}
impl<'native> TextSite<'_, 'native> {
    pub(super) fn text(&self) -> &'native str {
        self.owner.sites.as_slice()[self.index].text
    }
    pub(super) fn native(&self) -> &'native Value {
        self.owner.sites.as_slice()[self.index].native
    }
    pub(super) fn source(&self) -> SourceClass {
        self.owner.sites.as_slice()[self.index].source
    }
    pub(super) fn constraint(&self) -> EditConstraint {
        self.owner.sites.as_slice()[self.index].constraint
    }
}
pub(super) struct TextPatch<'sites, 'native, 'replacement> {
    site: TextSite<'sites, 'native>,
    range: Range<usize>,
    replacement: &'replacement str,
}
impl<'sites, 'native, 'replacement> TextPatch<'sites, 'native, 'replacement> {
    pub(super) fn new(
        site: TextSite<'sites, 'native>,
        range: Range<usize>,
        replacement: &'replacement str,
    ) -> Self {
        Self {
            site,
            range,
            replacement,
        }
    }
}

pub(super) struct NativeTextEdit<'native> {
    native: &'native Value,
    text: PayloadText,
}
impl NativeTextEdit<'_> {
    pub(super) fn native(&self) -> &Value {
        self.native
    }
    pub(super) fn text(&self) -> &str {
        self.text.as_str()
    }
}
pub(super) struct AtomicPatchPlan<'native> {
    edits: PayloadVec<NativeTextEdit<'native>>,
}
impl<'native> AtomicPatchPlan<'native> {
    pub(super) fn edits(&self) -> &[NativeTextEdit<'native>] {
        self.edits.as_slice()
    }
}

/// Borrowed inspection claims, never proof of delivery, execution, or mapping authority.
/// Every node must belong to the exact request owner before transaction preparation.
pub(super) enum HistoryClaim<'native> {
    /// Client-replayed assistant history, not an attested delivered snapshot.
    AssistantItem { native: &'native Value },
    /// Structural pairing only. The client result remains untrusted current input.
    CallResultPair {
        call: &'native Value,
        result: &'native Value,
    },
    OpaqueItem {
        native: &'native Value,
        source: SourceClass,
    },
    /// Responses-only inspection syntax; never a native cursor or continuity grant.
    ResponsesCursor { native: &'native Value },
}

/// A borrowed native owner with budgeted inspection metadata, not a protocol IR.
/// No Debug/Serialize/Clone: native values and history can contain private text.
pub(super) struct RequestObservation<'native> {
    native: &'native Value,
    client: ClientProtocol,
    sites: TextSites<'native>,
    history: PayloadVec<HistoryClaim<'native>>,
    budget: PayloadBudget,
}
impl<'native> RequestObservation<'native> {
    pub(super) fn new(
        native: &'native Value,
        client: ClientProtocol,
        budget: PayloadBudget,
    ) -> Result<Self, ContractError> {
        Ok(Self {
            native,
            client,
            sites: TextSites::new(budget.clone())?,
            history: PayloadVec::new(budget.clone())?,
            budget,
        })
    }
    /// Fix source and editability before any patch proposal can be prepared.
    /// Preparation must validate source-node membership; value equality is insufficient.
    pub(super) fn observe_text(
        &mut self,
        native: &'native Value,
        source: SourceClass,
        constraint: EditConstraint,
    ) -> Result<(), ContractError> {
        self.sites.observe(native, source, constraint)
    }
    pub(super) fn claim_history(
        &mut self,
        claim: HistoryClaim<'native>,
    ) -> Result<(), ContractError> {
        if self.history.len() >= MAX_SITES_AND_PATCHES {
            return Err(TokenError::CapacityExceeded.into());
        }
        self.history.push(claim)?;
        Ok(())
    }
    pub(super) fn native(&self) -> &'native Value {
        self.native
    }
    pub(super) fn client(&self) -> ClientProtocol {
        self.client
    }
    pub(super) fn sites(&self) -> &TextSites<'native> {
        &self.sites
    }
    pub(super) fn history(&self) -> &[HistoryClaim<'native>] {
        self.history.as_slice()
    }
    pub(super) fn budget(&self) -> PayloadBudget {
        self.budget.clone()
    }
}

/// Adapter-selected decoded text handling. Native item kinds and indices stay in adapters.
/// Keep reasoning policy distinct: readable does not imply token restoration is allowed.
#[derive(Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub(super) enum TextPolicy {
    Output,
    Refusal,
    Reasoning,
    ToolArguments,
}

/// A decision about one decoded text operation, not a normalized response representation.
/// For delta, Patch replaces that operation's emission (including a previously held tail).
/// For done, Patch is a reconciled full snapshot, not another delta to append.
/// Failure is the Result error; no completion/release authority is carried by a variant.
pub(super) enum RestoreDecision {
    Unchanged,
    Patch(PayloadText),
    Hold,
}

/// Exact native borrows only. A frame may end anywhere, including inside UTF-8.
/// No event kind, terminal flag or EOF claim is inferred from these bytes or values.
#[derive(Clone, Copy)]
pub(super) enum OriginalResponseSource<'native> {
    Json(&'native Value),
    Frame(&'native [u8]),
}

// Bound all descendant identity searches without allocating a native-value index.
const MAX_ORIGINAL_CLAIM_DEPTH: usize = 64;
const MAX_ORIGINAL_CLAIM_WORK: usize = 4 * 1024 * 1024;
const MAX_ORIGINAL_CLAIMS: usize = 4096;

/// Original upstream inspection claims, never verifier output or completion evidence.
/// Native data remains borrowed; retained claim and owner metadata must be charged.
/// No Debug/Serialize/Clone or conversion into a proof/spec/resume capability.
pub(super) struct OriginalResponseObservation<'native> {
    source: OriginalResponseSource<'native>,
    upstream: UpstreamProtocol,
    attempt: super::restoration_core::AttemptProvenance,
    claims: PayloadVec<&'native Value>,
    metadata: super::payload_budget::Reservation,
    budget: PayloadBudget,
    remaining_claim_work: usize,
}

impl<'native> OriginalResponseObservation<'native> {
    pub(super) fn json(
        native: &'native Value,
        upstream: UpstreamProtocol,
        attempt: super::restoration_core::AttemptProvenance,
        budget: PayloadBudget,
    ) -> Result<Self, super::response_restoration::RestoreError> {
        Self::new(
            OriginalResponseSource::Json(native),
            upstream,
            attempt,
            budget,
        )
    }

    pub(super) fn frame(
        bytes: &'native [u8],
        upstream: UpstreamProtocol,
        attempt: super::restoration_core::AttemptProvenance,
        budget: PayloadBudget,
    ) -> Result<Self, super::response_restoration::RestoreError> {
        Self::new(
            OriginalResponseSource::Frame(bytes),
            upstream,
            attempt,
            budget,
        )
    }

    fn new(
        source: OriginalResponseSource<'native>,
        upstream: UpstreamProtocol,
        attempt: super::restoration_core::AttemptProvenance,
        budget: PayloadBudget,
    ) -> Result<Self, super::response_restoration::RestoreError> {
        use super::response_restoration::RestoreError;

        // Charge retained owner metadata, not a copy of the borrowed native payload.
        let metadata = budget
            .reserve(std::mem::size_of::<Self>())
            .map_err(RestoreError::capacity)?;
        let claims = PayloadVec::new(budget.clone()).map_err(RestoreError::capacity)?;
        Ok(Self {
            source,
            upstream,
            attempt,
            claims,
            metadata,
            budget,
            remaining_claim_work: MAX_ORIGINAL_CLAIM_WORK,
        })
    }

    /// A JSON node must belong by identity to this exact source, never merely equal it.
    /// Frame observations cannot acquire JSON claims; append failure must be atomic.
    pub(super) fn claim_node(
        &mut self,
        node: &'native Value,
    ) -> Result<(), super::response_restoration::RestoreError> {
        use super::response_restoration::RestoreError;

        fn contains_node(
            source: &Value,
            target: &Value,
            depth: usize,
            remaining_work: &mut usize,
        ) -> Result<bool, RestoreError> {
            if depth > MAX_ORIGINAL_CLAIM_DEPTH {
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

        let OriginalResponseSource::Json(source) = self.source else {
            return Err(RestoreError::RestoreFailed);
        };
        if self.claims.len() >= MAX_ORIGINAL_CLAIMS {
            return Err(RestoreError::capacity(TokenError::CapacityExceeded));
        }
        // Failed searches still consume the fixed work allowance, but never claim metadata.
        if !contains_node(source, node, 0, &mut self.remaining_claim_work)? {
            return Err(RestoreError::RestoreFailed);
        }
        self.claims.push(node).map_err(RestoreError::capacity)
    }

    pub(super) fn source(&self) -> OriginalResponseSource<'native> {
        self.source
    }

    pub(super) fn upstream(&self) -> UpstreamProtocol {
        self.upstream
    }

    pub(super) fn attempt(&self) -> &super::restoration_core::AttemptProvenance {
        &self.attempt
    }

    pub(super) fn claims(&self) -> &[&'native Value] {
        self.claims.as_slice()
    }
}
