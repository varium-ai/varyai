//! Optional, exact, memory-only delivered history proof.
//!
//! Capture never publishes authority. Only the final delivery owner may publish;
//! candidates and converter traces still require a live actual-binding recheck.
use super::{
    completed_context::ActualAttemptBinding,
    token_store_owners::{
        AllocationCharge, AllocationReservation, FallibleShared, SharedAllocation,
        StoreAllocationLedger, StoreText, StoreVec,
    },
    tokenization::{DlpRequestLease, DlpTokenStore, TokenError},
};
use serde_json::Value;
use std::sync::{Mutex, OnceLock};

const RESPONSE_BYTES: usize = 256 * 1024;
const NAMESPACE_BYTES: usize = 1024 * 1024;
pub(super) const GLOBAL_BYTES: usize = 8 * 1024 * 1024;
const MAX_NODES: usize = 4096;
const RECORD_SLOTS: usize = 3;
const TRACE_BYTES: usize = 16 * 1024;
const MAPPING_BYTES: usize = 32 * 1024;

// This optional plane is shared by every store; initialization failure stays closed.
static GLOBAL_ALLOCATION: OnceLock<Result<StoreAllocationLedger, TokenError>> = OnceLock::new();
pub(super) fn global_history_allocation() -> Result<StoreAllocationLedger, TokenError> {
    GLOBAL_ALLOCATION
        .get_or_init(|| StoreAllocationLedger::new(0, GLOBAL_BYTES))
        .as_ref()
        .cloned()
        .map_err(|error| *error)
}

// No raw namespace ledger escapes this owner. Its shared backing and the ledger's
// own backing stay charged until the last clone or reservation has been dropped.
#[derive(Clone)]
struct NamespaceLedger(SharedAllocation<NamespaceControl>);
struct NamespaceControl {
    ledger: StoreAllocationLedger,
    _backing: AllocationReservation,
    _control: AllocationReservation,
}
struct NamespaceReservation {
    // Release the raw reservation before the owner that keeps its ledger charged.
    _reservation: AllocationReservation,
    _owner: NamespaceLedger,
}
impl NamespaceLedger {
    fn new(global: StoreAllocationLedger) -> Option<Self> {
        let control = global
            .reserve(AllocationCharge {
                plaintext: 0,
                metadata: StoreAllocationLedger::control_bytes().ok()?,
            })
            .ok()?;
        let ledger = StoreAllocationLedger::new(0, NAMESPACE_BYTES).ok()?;
        let backing = ledger
            .reserve(AllocationCharge {
                plaintext: 0,
                metadata: super::token_store_owners::arc_backing_bytes::<NamespaceControl>()
                    .ok()?,
            })
            .ok()?;
        SharedAllocation::new(
            NamespaceControl {
                ledger,
                _backing: backing,
                _control: control,
            },
            global,
        )
        .ok()
        .map(Self)
    }
    fn reserve(&self, charge: AllocationCharge) -> Result<NamespaceReservation, TokenError> {
        Ok(NamespaceReservation {
            _reservation: self.0.get()?.ledger.reserve(charge)?,
            _owner: self.clone(),
        })
    }
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum DlpHistoryLeafField {
    OutputText,
    ReasoningSummary,
    ReasoningContent,
    CallReasoning,
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum DlpHistoryTarget {
    ChatContent {
        message_index: usize,
    },
    ChatReasoning {
        message_index: usize,
    },
    ResponsesLeaf {
        input_index: usize,
        field: DlpHistoryLeafField,
        part_index: usize,
    },
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum DlpHistorySeparator {
    None,
    Newline,
    DoubleNewline,
}
impl DlpHistorySeparator {
    fn text(self) -> &'static str {
        match self {
            Self::None => "",
            Self::Newline => "\n",
            Self::DoubleNewline => "\n\n",
        }
    }
}

/// Both ceilings are reserved before creating any response/trace-owned backing.
/// Children are declared before this owner so backing is freed before permits.
struct Permit {
    _namespace: NamespaceReservation,
    _global: AllocationReservation,
}
impl Permit {
    fn new(ns: &HistoryNamespace, bytes: usize) -> Option<Self> {
        let charge = AllocationCharge {
            plaintext: 0,
            metadata: bytes,
        };
        let global = ns.global.reserve(charge).ok()?;
        let namespace = ns.ledger.reserve(charge).ok()?;
        Some(Self {
            _namespace: namespace,
            _global: global,
        })
    }
}

pub(super) struct HistoryNamespace {
    records: Mutex<[Option<RecordPin>; RECORD_SLOTS]>,
    ledger: NamespaceLedger,
    global: StoreAllocationLedger,
}
impl HistoryNamespace {
    pub(super) fn new(global: StoreAllocationLedger) -> Option<SharedAllocation<Self>> {
        let ledger = NamespaceLedger::new(global.clone())?;
        SharedAllocation::new(
            Self {
                records: Mutex::new(std::array::from_fn(|_| None)),
                ledger,
                global: global.clone(),
            },
            global,
        )
        .ok()
    }
    pub(super) fn capture(
        &self,
        binding: &ActualAttemptBinding,
        response: &Value,
    ) -> Option<PendingDeliveredHistory> {
        let items = response.get("output")?.as_array()?;
        if items.is_empty() || items.len() > MAX_NODES || !items.iter().all(supported_item) {
            return None;
        }
        // Preflight reads the original tree without creating a serialized copy.
        let mut footprint = StoreAllocationLedger::control_bytes()
            .ok()?
            .checked_add(super::token_store_owners::arc_backing_bytes::<HistoryRecord>().ok()?)?
            .checked_add(
                items
                    .len()
                    .checked_mul(std::mem::size_of::<FrozenValue>())?,
            )?;
        let mut nodes = 0;
        for item in items {
            preflight(item, 0, &mut nodes, &mut footprint)?;
        }
        if footprint > RESPONSE_BYTES {
            return None;
        }
        let permit = Permit::new(self, RESPONSE_BYTES)?;
        let ledger = StoreAllocationLedger::new(RESPONSE_BYTES, RESPONSE_BYTES).ok()?;
        let mut frozen = StoreVec::with_capacity(items.len(), ledger.clone(), MAX_NODES).ok()?;
        for item in items {
            frozen
                .push_without_growth(FrozenValue::copy(item, &ledger)?)
                .ok()?;
        }
        let used = ledger.used();
        if used
            .plaintext
            .checked_add(used.metadata)?
            .checked_add(super::token_store_owners::arc_backing_bytes::<HistoryRecord>().ok()?)?
            > RESPONSE_BYTES
        {
            return None;
        }
        let record = RecordPin::new(HistoryRecord {
            items: frozen,
            binding: binding.clone(),
            _ledger: ledger,
            _permit: permit,
        })?;
        Some(PendingDeliveredHistory { record })
    }
    pub(super) fn publish(
        &self,
        pending: PendingDeliveredHistory,
    ) -> Result<RetiredHistory, PendingDeliveredHistory> {
        let mut records = match self.records.lock() {
            Ok(records) => records,
            Err(_) => return Err(pending),
        };
        // The caller destroys displaced backing after releasing store state.
        let displaced = records[RECORD_SLOTS - 1].take();
        for index in (1..RECORD_SLOTS).rev() {
            records[index] = records[index - 1].take();
        }
        records[0] = Some(pending.record);
        Ok(RetiredHistory { _record: displaced })
    }
    #[cfg(test)]
    pub(super) fn evict_records_for_test(&self) -> Result<(), TokenError> {
        let retired = {
            let mut records = self.records.lock().map_err(|_| TokenError::Unavailable)?;
            std::mem::replace(&mut *records, std::array::from_fn(|_| None))
        };
        drop(retired);
        Ok(())
    }

    pub(super) fn projection<'a>(
        &self,
        store: DlpTokenStore,
        binding: Option<&ActualAttemptBinding>,
        request: &'a Value,
    ) -> Option<DlpHistoryProjection<'a>> {
        let items = request.get("input")?.as_array()?;
        if items.len() > MAX_NODES {
            return None;
        }
        // This permit covers the inline pin array and its temporary copies too.
        let permit = Permit::new(self, 2048)?;
        let records = self.records.lock().ok()?;
        let mut pins = std::array::from_fn(|_| None);
        for (slot, record) in pins.iter_mut().zip(records.iter()) {
            if let Some(record) = record {
                let issued = &record.get().ok()?.binding;
                if binding.is_none_or(|actual| same_binding(issued, actual)) {
                    *slot = Some(record.clone());
                }
            }
        }
        drop(records);
        for pin in &mut pins {
            if pin.as_ref().is_some_and(|record| {
                !record
                    .get()
                    .is_ok_and(|record| store.history_binding_live(&record.binding))
            }) {
                *pin = None;
            }
        }
        // Exact items may prove inspection leaves. A recorded tool identity only
        // preserves completion dependencies; it never proves changed text.
        let matched = pins.iter().flatten().any(|pin| {
            pin.get().is_ok_and(|record| {
                record.items.as_slice().iter().any(|frozen| {
                    items.iter().any(|item| {
                        (supported_item(item) && frozen.matches(item))
                            || frozen.matches_call_identity(item)
                    })
                })
            })
        });
        if !matched {
            return None;
        }
        Some(DlpHistoryProjection {
            request,
            origin: None,
            store,
            records: pins,
            namespace: self.ledger.clone(),
            global: self.global.clone(),
            _permit: permit,
        })
    }
}

// A small owned tree avoids uncharged serde_json map growth and temporary clones.
// Every vector, key, decoded string and shared control block has its own owner.
enum FrozenValue {
    Null,
    Bool(bool),
    Number(serde_json::Number),
    String(StoreText),
    Array(StoreVec<FrozenValue>),
    Object(StoreVec<(StoreText, FrozenValue)>),
}
impl FrozenValue {
    fn copy(value: &Value, ledger: &StoreAllocationLedger) -> Option<Self> {
        Some(match value {
            Value::Null => Self::Null,
            Value::Bool(value) => Self::Bool(*value),
            Value::Number(value) => Self::Number(value.clone()),
            Value::String(value) => {
                Self::String(StoreText::try_from_str(value, ledger.clone(), RESPONSE_BYTES).ok()?)
            }
            Value::Array(values) => {
                let mut owned =
                    StoreVec::with_capacity(values.len(), ledger.clone(), MAX_NODES).ok()?;
                for value in values {
                    owned.push_without_growth(Self::copy(value, ledger)?).ok()?;
                }
                Self::Array(owned)
            }
            Value::Object(values) => {
                let mut owned =
                    StoreVec::with_capacity(values.len(), ledger.clone(), MAX_NODES).ok()?;
                for (key, value) in values {
                    owned
                        .push_without_growth((
                            StoreText::try_from_str(key, ledger.clone(), RESPONSE_BYTES).ok()?,
                            Self::copy(value, ledger)?,
                        ))
                        .ok()?;
                }
                Self::Object(owned)
            }
        })
    }
    fn matches(&self, value: &Value) -> bool {
        match (self, value) {
            (Self::Null, Value::Null) => true,
            (Self::Bool(a), Value::Bool(b)) => a == b,
            (Self::Number(a), Value::Number(b)) => a == b,
            (Self::String(a), Value::String(b)) => a.as_str() == b,
            (Self::Array(a), Value::Array(b)) => {
                a.len() == b.len() && a.as_slice().iter().zip(b).all(|(a, b)| a.matches(b))
            }
            (Self::Object(a), Value::Object(b)) => {
                a.len() == b.len()
                    && a.as_slice()
                        .iter()
                        .all(|(key, value)| b.get(key.as_str()).is_some_and(|v| value.matches(v)))
            }
            _ => false,
        }
    }
    fn matches_call_identity(&self, item: &Value) -> bool {
        if !matches!(
            self.get("type").and_then(Self::text),
            Some("function_call" | "custom_tool_call")
        ) || !matches!(
            item.get("type").and_then(Value::as_str),
            Some("function_call" | "custom_tool_call")
        ) {
            return false;
        }
        ["call_id", "id"].iter().any(|key| {
            item.get(*key)
                .and_then(Value::as_str)
                .is_some_and(|id| self.get(key).and_then(Self::text) == Some(id))
        })
    }
    fn get(&self, key: &str) -> Option<&Self> {
        let Self::Object(fields) = self else {
            return None;
        };
        fields
            .as_slice()
            .iter()
            .find_map(|(k, v)| (k.as_str() == key).then_some(v))
    }
    fn part(&self, index: usize) -> Option<&Self> {
        let Self::Array(parts) = self else {
            return None;
        };
        parts.as_slice().get(index)
    }
    fn text(&self) -> Option<&str> {
        let Self::String(text) = self else {
            return None;
        };
        Some(text.as_str())
    }
}
fn preflight(value: &Value, depth: usize, nodes: &mut usize, bytes: &mut usize) -> Option<()> {
    *nodes = nodes.checked_add(1)?;
    if depth > 64 || *nodes > MAX_NODES {
        return None;
    }
    let extra = match value {
        Value::String(text) => text.len(),
        Value::Array(values) => values
            .len()
            .checked_mul(std::mem::size_of::<FrozenValue>())?,
        Value::Object(values) => values
            .len()
            .checked_mul(std::mem::size_of::<(StoreText, FrozenValue)>())?,
        _ => 0,
    };
    *bytes = bytes.checked_add(extra)?;
    match value {
        Value::Array(values) => {
            for value in values {
                preflight(value, depth + 1, nodes, bytes)?;
            }
        }
        Value::Object(values) => {
            for (key, value) in values {
                *bytes = bytes.checked_add(key.len())?;
                preflight(value, depth + 1, nodes, bytes)?;
            }
        }
        _ => (),
    }
    (*bytes <= RESPONSE_BYTES).then_some(())
}
fn supported_item(item: &Value) -> bool {
    let Some(kind) = item.get("type").and_then(Value::as_str) else {
        return false;
    };
    match kind {
        "message" => {
            item.get("role").and_then(Value::as_str) == Some("assistant")
                && typed_parts(item.get("content"), "output_text")
        }
        "reasoning" => {
            let summary = item.get("summary");
            let content = item.get("content");
            (summary.is_some() || content.is_some())
                && summary.is_none_or(|v| typed_parts(Some(v), "summary_text"))
                && content.is_none_or(|v| typed_parts(Some(v), "reasoning_text"))
        }
        "function_call" | "custom_tool_call" => {
            item.get("call_id").is_some_and(Value::is_string)
                && item.get("name").is_some_and(Value::is_string)
                && item
                    .get(if kind == "function_call" {
                        "arguments"
                    } else {
                        "input"
                    })
                    .is_some_and(Value::is_string)
                && item.get("reasoning_content").is_none_or(Value::is_string)
        }
        _ => false,
    }
}
fn typed_parts(value: Option<&Value>, kind: &str) -> bool {
    value.and_then(Value::as_array).is_some_and(|parts| {
        parts.iter().all(|part| {
            part.get("type").and_then(Value::as_str) == Some(kind)
                && part.get("text").is_some_and(Value::is_string)
        })
    })
}
fn same_binding(a: &ActualAttemptBinding, b: &ActualAttemptBinding) -> bool {
    a.store_id == b.store_id
        && a.namespace == b.namespace
        && a.generation == b.generation
        && a.policy_revision == b.policy_revision
        && a.provider_fingerprint == b.provider_fingerprint
        && a.reusable_principal
        && b.reusable_principal
}
struct HistoryRecord {
    items: StoreVec<FrozenValue>,
    binding: ActualAttemptBinding,
    _ledger: StoreAllocationLedger,
    _permit: Permit,
}
// The final strong owner frees shared backing before returning the payload.
// Drop frees the tree and local ledger before releasing either outer permit.
#[derive(Clone)]
struct RecordPin(Option<FallibleShared<HistoryRecord>>);
impl RecordPin {
    fn new(record: HistoryRecord) -> Option<Self> {
        #[cfg(test)]
        super::token_store_owners::test_support::emit(
            super::token_store_owners::test_support::Kind::Shared,
            super::token_store_owners::test_support::Phase::BeforeAllocation,
            super::token_store_owners::arc_backing_bytes::<HistoryRecord>().ok()?,
            &record._ledger,
        )
        .ok()?;
        let owner = Self(Some(FallibleShared::new(record).ok()?));
        #[cfg(test)]
        super::token_store_owners::test_support::emit(
            super::token_store_owners::test_support::Kind::Shared,
            super::token_store_owners::test_support::Phase::AfterAllocation,
            super::token_store_owners::arc_backing_bytes::<HistoryRecord>().ok()?,
            &owner.get().ok()?._ledger,
        )
        .ok()?;
        Some(owner)
    }
    fn get(&self) -> Result<&HistoryRecord, TokenError> {
        self.0.as_deref().ok_or(TokenError::Unavailable)
    }
}
impl Drop for RecordPin {
    fn drop(&mut self) {
        if let Some(record) = self.0.take().and_then(FallibleShared::into_inner) {
            drop(record);
        }
    }
}
pub(super) struct RetiredHistory {
    _record: Option<RecordPin>,
}
pub(crate) struct PendingDeliveredHistory {
    record: RecordPin,
}
impl PendingDeliveredHistory {
    pub(super) fn binding(&self) -> Option<&ActualAttemptBinding> {
        Some(&self.record.get().ok()?.binding)
    }
}
pub(crate) struct DlpHistoryProjection<'a> {
    request: &'a Value,
    origin: Option<&'a Value>,
    store: DlpTokenStore,
    records: [Option<RecordPin>; RECORD_SLOTS],
    namespace: NamespaceLedger,
    global: StoreAllocationLedger,
    _permit: Permit,
}
#[derive(Clone)]
pub(crate) struct DlpHistorySource {
    record: RecordPin,
    input_index: usize,
    item_index: usize,
    field: DlpHistoryLeafField,
    part_index: usize,
}
impl DlpHistorySource {
    pub(crate) fn field(&self) -> DlpHistoryLeafField {
        self.field
    }
    pub(crate) fn is_reasoning(&self) -> bool {
        self.field != DlpHistoryLeafField::OutputText
    }
    fn item(&self) -> Option<&FrozenValue> {
        self.record
            .get()
            .ok()?
            .items
            .as_slice()
            .get(self.item_index)
    }
    fn text(&self) -> Option<&str> {
        let item = self.item()?;
        let (container, part_type) = match self.field {
            DlpHistoryLeafField::OutputText
                if item.get("type")?.text()? == "message"
                    && item.get("role")?.text()? == "assistant" =>
            {
                ("content", "output_text")
            }
            DlpHistoryLeafField::ReasoningSummary if item.get("type")?.text()? == "reasoning" => {
                ("summary", "summary_text")
            }
            DlpHistoryLeafField::ReasoningContent if item.get("type")?.text()? == "reasoning" => {
                ("content", "reasoning_text")
            }
            DlpHistoryLeafField::CallReasoning
                if self.part_index == 0
                    && matches!(
                        item.get("type")?.text()?,
                        "function_call" | "custom_tool_call"
                    ) =>
            {
                return item.get("reasoning_content")?.text()
            }
            _ => return None,
        };
        let part = item.get(container)?.part(self.part_index)?;
        (part.get("type")?.text()? == part_type).then_some(())?;
        part.get("text")?.text()
    }
    fn coordinate(&self) -> (usize, DlpHistoryLeafField, usize) {
        (self.input_index, self.field, self.part_index)
    }
}
impl DlpHistoryProjection<'_> {
    pub(crate) fn consumed_body_matches(&self, body: &Value) -> bool {
        self.request == body
    }
    /// A recorded tool identity keeps its mandatory completion checks even if its text changes.
    /// Message-only inspection provenance does not create a dependency for unrelated calls.
    pub(super) fn has_recorded_call_identity(&self, item: &Value) -> bool {
        self.records.iter().flatten().any(|record| {
            record.get().is_ok_and(|record| {
                self.store.history_binding_live(&record.binding)
                    && record
                        .items
                        .as_slice()
                        .iter()
                        .any(|frozen| frozen.matches_call_identity(item))
            })
        })
    }

    pub(crate) fn source_for_responses_leaf(
        &self,
        index: usize,
        field: DlpHistoryLeafField,
        part: usize,
    ) -> Option<DlpHistorySource> {
        let item = self.request.get("input")?.as_array()?.get(index)?;
        if self.origin.is_some_and(|origin| {
            origin
                .get("input")
                .and_then(Value::as_array)
                .and_then(|items| items.get(index))
                != Some(item)
        }) {
            return None;
        }
        for record in self.records.iter().flatten() {
            if !self.store.history_binding_live(&record.get().ok()?.binding) {
                continue;
            }
            for (item_index, frozen) in record.get().ok()?.items.as_slice().iter().enumerate() {
                if frozen.matches(item) {
                    let source = DlpHistorySource {
                        record: record.clone(),
                        input_index: index,
                        item_index,
                        field,
                        part_index: part,
                    };
                    source.text()?;
                    return Some(source);
                }
            }
        }
        None
    }
    pub(crate) fn trace(&self) -> Option<DlpHistoryTrace> {
        DlpHistoryTrace::new(self.namespace.clone(), self.global.clone())
    }
    pub(crate) fn mapping(&self) -> Option<DlpHistoryMapping> {
        DlpHistoryMapping::new(self.namespace.clone(), self.global.clone())
    }
}

struct MetadataOwner {
    ledger: StoreAllocationLedger,
    _permit: Permit,
}
impl MetadataOwner {
    fn new(
        namespace: NamespaceLedger,
        global: StoreAllocationLedger,
        bytes: usize,
    ) -> Option<Self> {
        Self::with_plaintext_limit(namespace, global, 0, bytes)
    }
    fn with_plaintext_limit(
        namespace: NamespaceLedger,
        global: StoreAllocationLedger,
        plaintext_limit: usize,
        bytes: usize,
    ) -> Option<Self> {
        #[cfg(test)]
        if !history_allocation_test_support::owner_allowed() {
            return None;
        }
        let charge = AllocationCharge {
            plaintext: 0,
            metadata: bytes,
        };
        let global_charge = global.reserve(charge).ok()?;
        let ns_charge = namespace.reserve(charge).ok()?;
        let permit = Permit {
            _namespace: ns_charge,
            _global: global_charge,
        };
        let owner = Self {
            ledger: StoreAllocationLedger::new(plaintext_limit, bytes).ok()?,
            _permit: permit,
        };
        #[cfg(test)]
        history_allocation_test_support::owner_created();
        Some(owner)
    }
}
struct TraceEntry {
    source: DlpHistorySource,
    separator: DlpHistorySeparator,
}
pub(crate) struct DlpHistoryTrace {
    entries: StoreVec<TraceEntry>,
    valid: bool,
    failed: bool,
    _owner: MetadataOwner,
}
impl DlpHistoryTrace {
    fn new(namespace: NamespaceLedger, global: StoreAllocationLedger) -> Option<Self> {
        let owner = MetadataOwner::new(namespace, global, TRACE_BYTES)?;
        Some(Self {
            entries: StoreVec::new(owner.ledger.clone(), MAX_NODES),
            valid: true,
            failed: false,
            _owner: owner,
        })
    }
    pub(crate) fn invalidate(&mut self) {
        self.valid = false;
    }
    pub(crate) fn failed(&self) -> bool {
        self.failed
    }
    pub(crate) fn valid(&self) -> bool {
        self.valid && !self.failed
    }
    pub(crate) fn push(
        &mut self,
        source: &DlpHistorySource,
        separator_before: DlpHistorySeparator,
    ) -> bool {
        if !self.valid || source.text().is_none() {
            self.invalidate();
            return false;
        }
        if let Some(previous) = self.entries.as_slice().last() {
            let compatible = previous
                .source
                .record
                .get()
                .ok()
                .zip(source.record.get().ok())
                .is_some_and(|(a, b)| same_binding(&a.binding, &b.binding));
            if previous.source.coordinate() >= source.coordinate() || !compatible {
                self.invalidate();
                return false;
            }
        } else if separator_before != DlpHistorySeparator::None {
            self.invalidate();
            return false;
        }
        if self
            .entries
            .push(TraceEntry {
                source: source.clone(),
                separator: separator_before,
            })
            .is_err()
        {
            self.failed = true;
            self.invalidate();
            return false;
        }
        true
    }
    pub(crate) fn append(&mut self, other: Self, separator_before: DlpHistorySeparator) -> bool {
        if other.failed {
            self.failed = true;
        }
        if !other.valid || other.entries.is_empty() {
            self.invalidate();
            return false;
        }
        for (index, entry) in other.entries.as_slice().iter().enumerate() {
            if !self.push(
                &entry.source,
                if index == 0 {
                    separator_before
                } else {
                    entry.separator
                },
            ) {
                return false;
            }
        }
        true
    }
    fn supports(&self, target: DlpHistoryTarget) -> bool {
        self.valid
            && !self.entries.is_empty()
            && self.entries.as_slice().iter().all(|entry| match target {
                DlpHistoryTarget::ChatContent { .. } => !entry.source.is_reasoning(),
                DlpHistoryTarget::ChatReasoning { .. } => entry.source.is_reasoning(),
                DlpHistoryTarget::ResponsesLeaf {
                    field, part_index, ..
                } => {
                    self.entries.len() == 1
                        && entry.source.field == field
                        && entry.source.part_index == part_index
                }
            })
    }
    fn matches_text(&self, mut text: &str) -> bool {
        for entry in self.entries.as_slice() {
            let Some(rest) = text
                .strip_prefix(entry.separator.text())
                .and_then(|v| v.strip_prefix(entry.source.text()?))
            else {
                return false;
            };
            text = rest;
        }
        text.is_empty()
    }
}
// Final converter targets are copied structurally, never inferred from text or IDs.
struct ChatTargetSnapshot {
    messages: StoreVec<(usize, FrozenValue)>,
    _owner: MetadataOwner,
}
impl ChatTargetSnapshot {
    fn matches(&self, index: usize, message: &Value) -> bool {
        self.messages
            .as_slice()
            .iter()
            .any(|(stored_index, frozen)| *stored_index == index && frozen.matches(message))
    }
}

pub(crate) struct DlpHistoryMapping {
    entries: StoreVec<Option<(DlpHistoryTarget, DlpHistoryTrace)>>,
    chat_targets: Option<ChatTargetSnapshot>,
    namespace: NamespaceLedger,
    global: StoreAllocationLedger,
    failed: bool,
    _owner: MetadataOwner,
}
impl DlpHistoryMapping {
    fn new(namespace: NamespaceLedger, global: StoreAllocationLedger) -> Option<Self> {
        let owner = MetadataOwner::new(namespace.clone(), global.clone(), MAPPING_BYTES)?;
        Some(Self {
            entries: StoreVec::new(owner.ledger.clone(), MAX_NODES),
            chat_targets: None,
            namespace,
            global,
            failed: false,
            _owner: owner,
        })
    }
    /// Seal converter-owned targets before any caller can apply body overrides.
    pub(crate) fn seal_chat_targets(&mut self, outbound: &Value) -> bool {
        if self.failed || self.chat_targets.is_some() {
            self.fail();
            return false;
        }
        let captured = self.capture_chat_targets(outbound);
        match captured {
            Some(snapshot) => {
                self.chat_targets = Some(snapshot);
                true
            }
            None => {
                self.fail();
                false
            }
        }
    }
    fn capture_chat_targets(&self, outbound: &Value) -> Option<ChatTargetSnapshot> {
        let messages = outbound.get("messages")?.as_array()?;
        if messages.len() > MAX_NODES {
            return None;
        }
        let selected = |index: usize| {
            self.entries.as_slice().iter().flatten().any(|(target, _)| {
                matches!(*target,
                    DlpHistoryTarget::ChatContent { message_index }
                    | DlpHistoryTarget::ChatReasoning { message_index } if message_index == index)
            })
        };
        let count = messages
            .iter()
            .enumerate()
            .filter(|(index, _)| selected(*index))
            .count();
        let mut bytes = StoreAllocationLedger::control_bytes()
            .ok()?
            .checked_add(count.checked_mul(std::mem::size_of::<(usize, FrozenValue)>())?)?;
        let mut nodes = 0;
        for (_, message) in messages
            .iter()
            .enumerate()
            .filter(|(index, _)| selected(*index))
        {
            if message.get("role").and_then(Value::as_str) != Some("assistant") {
                return None;
            }
            preflight(message, 0, &mut nodes, &mut bytes)?;
        }
        let owner = MetadataOwner::with_plaintext_limit(
            self.namespace.clone(),
            self.global.clone(),
            RESPONSE_BYTES,
            RESPONSE_BYTES,
        )?;
        let ledger = owner.ledger.clone();
        let mut frozen = StoreVec::with_capacity(count, ledger.clone(), MAX_NODES).ok()?;
        for (index, message) in messages
            .iter()
            .enumerate()
            .filter(|(index, _)| selected(*index))
        {
            frozen
                .push_without_growth((index, FrozenValue::copy(message, &ledger)?))
                .ok()?;
        }
        let used = ledger.used();
        if used.plaintext.checked_add(used.metadata)? > RESPONSE_BYTES {
            return None;
        }
        Some(ChatTargetSnapshot {
            messages: frozen,
            _owner: owner,
        })
    }
    pub(crate) fn failed(&self) -> bool {
        self.failed
    }
    pub(crate) fn valid(&self) -> bool {
        !self.failed
    }
    fn fail(&mut self) {
        self.failed = true;
        self.chat_targets = None;
        for entry in self.entries.as_mut_slice() {
            *entry = None;
        }
    }
    pub(crate) fn record(
        &mut self,
        target: DlpHistoryTarget,
        sources: &[DlpHistorySource],
        separator: DlpHistorySeparator,
    ) -> bool {
        let Some(mut trace) = DlpHistoryTrace::new(self.namespace.clone(), self.global.clone())
        else {
            self.fail();
            return false;
        };
        for (index, source) in sources.iter().enumerate() {
            if !trace.push(
                source,
                if index == 0 {
                    DlpHistorySeparator::None
                } else {
                    separator
                },
            ) {
                if trace.failed() {
                    self.fail();
                } else {
                    self.remove(target);
                }
                return false;
            }
        }
        self.record_trace(target, trace)
    }
    pub(crate) fn record_trace(
        &mut self,
        target: DlpHistoryTarget,
        trace: DlpHistoryTrace,
    ) -> bool {
        if trace.failed() {
            self.fail();
        }
        if self.failed {
            return false;
        }
        self.remove(target);
        if !trace.supports(target) {
            return false;
        }
        if let Some(slot) = self
            .entries
            .as_mut_slice()
            .iter_mut()
            .find(|slot| slot.is_none())
        {
            *slot = Some((target, trace));
            return true;
        }
        if self.entries.push(Some((target, trace))).is_err() {
            self.fail();
            return false;
        }
        true
    }
    pub(crate) fn take_trace(&mut self, target: DlpHistoryTarget) -> Option<DlpHistoryTrace> {
        self.entries
            .as_mut_slice()
            .iter_mut()
            .find(|slot| slot.as_ref().is_some_and(|(key, _)| *key == target))?
            .take()
            .map(|(_, trace)| trace)
    }
    pub(crate) fn remove(&mut self, target: DlpHistoryTarget) {
        drop(self.take_trace(target));
    }
    pub(crate) fn remap_chat_targets(&mut self, remap: impl Fn(usize) -> Option<usize>) {
        for slot in self.entries.as_mut_slice() {
            let Some((target, _)) = slot.as_mut() else {
                continue;
            };
            let mapped = match *target {
                DlpHistoryTarget::ChatContent { message_index } => remap(message_index)
                    .map(|message_index| DlpHistoryTarget::ChatContent { message_index }),
                DlpHistoryTarget::ChatReasoning { message_index } => remap(message_index)
                    .map(|message_index| DlpHistoryTarget::ChatReasoning { message_index }),
                other => Some(other),
            };
            if let Some(mapped) = mapped {
                *target = mapped;
            } else {
                *slot = None;
            }
        }
        // Ambiguous remaps authorize neither target, not whichever entry won last.
        for index in 0..self.entries.len() {
            let collision = self
                .entries
                .as_slice()
                .get(index)
                .and_then(Option::as_ref)
                .is_some_and(|(target, _)| {
                    self.entries
                        .as_slice()
                        .iter()
                        .flatten()
                        .filter(|(other, _)| other == target)
                        .count()
                        > 1
                });
            if collision {
                let target = self.entries.as_slice()[index]
                    .as_ref()
                    .map(|(target, _)| *target);
                for slot in self.entries.as_mut_slice() {
                    if slot
                        .as_ref()
                        .is_some_and(|(other, _)| Some(*other) == target)
                    {
                        *slot = None;
                    }
                }
            }
        }
    }
    pub(crate) fn validates_target(
        &self,
        lease: &DlpRequestLease,
        binding: &ActualAttemptBinding,
        outbound: &Value,
        target: DlpHistoryTarget,
    ) -> bool {
        if self.failed || !lease.history_binding_live(binding) {
            return false;
        }
        let Some((_, trace)) = self
            .entries
            .as_slice()
            .iter()
            .flatten()
            .find(|(key, _)| *key == target)
        else {
            return false;
        };
        if !trace.supports(target)
            || !trace.entries.as_slice().iter().all(|entry| {
                entry
                    .source
                    .record
                    .get()
                    .is_ok_and(|record| same_binding(&record.binding, binding))
            })
        {
            return false;
        }
        let text = match target {
            DlpHistoryTarget::ChatContent { message_index }
            | DlpHistoryTarget::ChatReasoning { message_index } => {
                let Some(message) = outbound
                    .get("messages")
                    .and_then(Value::as_array)
                    .and_then(|messages| messages.get(message_index))
                else {
                    return false;
                };
                if message.get("role").and_then(Value::as_str) != Some("assistant")
                    || !self
                        .chat_targets
                        .as_ref()
                        .is_some_and(|snapshot| snapshot.matches(message_index, message))
                {
                    return false;
                }
                message
                    .get(if matches!(target, DlpHistoryTarget::ChatContent { .. }) {
                        "content"
                    } else {
                        "reasoning_content"
                    })
                    .and_then(Value::as_str)
            }
            DlpHistoryTarget::ResponsesLeaf { input_index, .. } => {
                let Some(item) = outbound
                    .get("input")
                    .and_then(Value::as_array)
                    .and_then(|items| items.get(input_index))
                else {
                    return false;
                };
                let Some(entry) = trace.entries.as_slice().first() else {
                    return false;
                };
                return entry
                    .source
                    .item()
                    .is_some_and(|frozen| frozen.matches(item));
            }
        };
        text.is_some_and(|text| trace.matches_text(text))
    }
}

#[cfg(test)]
pub(crate) mod history_allocation_test_support {
    use std::{cell::Cell, marker::PhantomData, rc::Rc};

    thread_local! {
        static METADATA_REMAINING: Cell<Option<usize>> = const { Cell::new(None) };
    }

    pub(super) fn owner_allowed() -> bool {
        METADATA_REMAINING.with(|remaining| remaining.get() != Some(0))
    }
    pub(super) fn owner_created() {
        METADATA_REMAINING.with(|remaining| {
            remaining.set(remaining.get().map(|count| count.saturating_sub(1)));
        });
    }

    pub(crate) struct Guard {
        previous: Option<usize>,
        _thread_bound: PhantomData<Rc<()>>,
    }
    impl Drop for Guard {
        fn drop(&mut self) {
            METADATA_REMAINING.with(|remaining| remaining.set(self.previous));
        }
    }
    pub(crate) fn fail_metadata_after(successful_owners: usize) -> Guard {
        Guard {
            previous: METADATA_REMAINING
                .with(|remaining| remaining.replace(Some(successful_owners))),
            _thread_bound: PhantomData,
        }
    }
}

#[cfg(test)]
mod allocation_tests {
    use super::{
        history_allocation_test_support::fail_metadata_after, MetadataOwner, NamespaceLedger,
        StoreAllocationLedger, TokenError, GLOBAL_BYTES, TRACE_BYTES,
    };

    #[test]
    fn history_provenance_metadata_failure_hook_reserves_nothing_and_restores_scope(
    ) -> Result<(), TokenError> {
        let global = StoreAllocationLedger::new(0, GLOBAL_BYTES)?;
        let namespace = NamespaceLedger::new(global.clone()).ok_or(TokenError::Unavailable)?;
        let baseline = global.used();
        {
            let _guard = fail_metadata_after(1);
            let owner = MetadataOwner::new(namespace.clone(), global.clone(), TRACE_BYTES)
                .ok_or(TokenError::Unavailable)?;
            let held = global.used();
            assert!(MetadataOwner::new(namespace.clone(), global.clone(), TRACE_BYTES).is_none());
            assert_eq!(global.used(), held);
            drop(owner);
            assert_eq!(global.used(), baseline);
        }
        assert!(MetadataOwner::new(namespace.clone(), global.clone(), TRACE_BYTES).is_some());
        assert_eq!(global.used(), baseline);
        Ok(())
    }

    #[test]
    fn history_provenance_metadata_failure_hook_is_nested_and_thread_local(
    ) -> Result<(), TokenError> {
        let global = StoreAllocationLedger::new(0, GLOBAL_BYTES)?;
        let namespace = NamespaceLedger::new(global.clone()).ok_or(TokenError::Unavailable)?;
        let outer = fail_metadata_after(0);
        {
            let _inner = fail_metadata_after(1);
            assert!(MetadataOwner::new(namespace.clone(), global.clone(), TRACE_BYTES).is_some());
            assert!(MetadataOwner::new(namespace.clone(), global.clone(), TRACE_BYTES).is_none());
        }
        assert!(MetadataOwner::new(namespace.clone(), global.clone(), TRACE_BYTES).is_none());
        let thread_namespace = namespace.clone();
        let thread_global = global.clone();
        let allocated = std::thread::spawn(move || {
            MetadataOwner::new(thread_namespace, thread_global, TRACE_BYTES).is_some()
        })
        .join()
        .map_err(|_| TokenError::Unavailable)?;
        assert!(allocated);
        drop(outer);
        assert!(MetadataOwner::new(namespace, global, TRACE_BYTES).is_some());
        Ok(())
    }

    #[test]
    fn history_provenance_record_allocator_null_discards_proof_and_releases_all_charges(
    ) -> Result<(), TokenError> {
        let Some(_completion) = crate::proxy::security::history_test_process::enter()? else {
            return Ok(());
        };
        use super::super::token_store_owners::allocator_test_support::fail_next_shared;
        use super::super::tokenization::DlpTokenStore;
        let store = DlpTokenStore::new();
        let lease = store.admit("codex", Some("synthetic-record-allocation"), 1)?;
        let binding = lease.bind_actual_attempt_extracted(
            "synthetic-provider",
            "responses",
            "https://synthetic.invalid/v1/responses",
            Some("synthetic-principal"),
        )?;
        let response = serde_json::json!({"output":[{"type":"reasoning","summary":[
        {"type":"summary_text","text":"synthetic allocator record"}]}]});
        let warmup = lease
            .capture_delivered_history(&binding, &response)
            .ok_or(TokenError::Unavailable)?;
        drop(warmup);
        let baseline = store.history_bytes_for_test()?;
        let mandatory = store.allocation_ledger()?.used();
        let guard = fail_next_shared::<super::HistoryRecord>();
        let result = lease.capture_delivered_history(&binding, &response);
        let denied = guard.denied();
        drop(guard);
        assert!(denied && result.is_none());
        assert!(store.history_bytes_for_test()? == baseline);
        assert_eq!(store.allocation_ledger()?.used(), mandatory);
        assert!(lease
            .capture_delivered_history(&binding, &response)
            .is_some());
        Ok(())
    }
}

/// Private inspection mode, derived only from an exact live delivered source.
#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub(super) enum HistoryLeafPolicy {
    #[default]
    Strict,
    UnprovenText,
    DeliveredOutput,
    DeliveredReasoning,
}
impl HistoryLeafPolicy {
    /// Request compatibility changes recognition only, never inspection provenance.
    pub(super) fn for_request_literals(self) -> Self {
        if self == Self::Strict {
            Self::UnprovenText
        } else {
            self
        }
    }

    pub(super) fn visit_references(
        self,
        text: &str,
        visitor: impl FnMut(std::ops::Range<usize>) -> Result<(), TokenError>,
    ) -> Result<(), TokenError> {
        match self {
            Self::Strict => super::tokenization::visit_token_references(text, visitor),
            Self::UnprovenText | Self::DeliveredOutput => {
                super::output_text_matching::visit_complete_references(text, visitor)
            }
            Self::DeliveredReasoning => Ok(()),
        }
    }
}

#[derive(Clone, Copy)]
pub(super) enum HistoryPathStep<'a> {
    Field(&'a str),
    Index(usize),
}
pub(super) fn history_target_for_path(path: &[HistoryPathStep<'_>]) -> Option<DlpHistoryTarget> {
    use HistoryPathStep::{Field, Index};
    let (input_index, field, part_index) = match path {
        [Field("input"), Index(index), Field("content"), Index(part), Field("text")] => {
            (*index, DlpHistoryLeafField::OutputText, *part)
        }
        [Field("input"), Index(index), Field("summary"), Index(part), Field("text")] => {
            (*index, DlpHistoryLeafField::ReasoningSummary, *part)
        }
        [Field("input"), Index(index), Field("reasoning_content")] => {
            (*index, DlpHistoryLeafField::CallReasoning, 0)
        }
        [Field("messages"), Index(index), Field("content")] => {
            return Some(DlpHistoryTarget::ChatContent {
                message_index: *index,
            })
        }
        [Field("messages"), Index(index), Field("reasoning_content")] => {
            return Some(DlpHistoryTarget::ChatReasoning {
                message_index: *index,
            })
        }
        _ => return None,
    };
    Some(DlpHistoryTarget::ResponsesLeaf {
        input_index,
        field,
        part_index,
    })
}
impl DlpHistoryProjection<'_> {
    pub(super) fn policy_for_target(
        &self,
        body: &Value,
        target: DlpHistoryTarget,
    ) -> HistoryLeafPolicy {
        let DlpHistoryTarget::ResponsesLeaf {
            input_index,
            mut field,
            part_index,
        } = target
        else {
            return HistoryLeafPolicy::Strict;
        };
        // content[] is shared by messages and reasoning, but only exact supported
        // typed sources decide which one can carry an exception.
        if field == DlpHistoryLeafField::OutputText
            && body
                .get("input")
                .and_then(Value::as_array)
                .and_then(|a| a.get(input_index))
                .and_then(|i| i.get("type"))
                .and_then(Value::as_str)
                == Some("reasoning")
        {
            field = DlpHistoryLeafField::ReasoningContent;
        }
        let Some(source) = self.source_for_responses_leaf(input_index, field, part_index) else {
            return HistoryLeafPolicy::Strict;
        };
        if !body
            .get("input")
            .and_then(Value::as_array)
            .and_then(|a| a.get(input_index))
            .is_some_and(|item| source.item().is_some_and(|frozen| frozen.matches(item)))
        {
            return HistoryLeafPolicy::Strict;
        }
        if source.is_reasoning() {
            HistoryLeafPolicy::DeliveredReasoning
        } else {
            HistoryLeafPolicy::DeliveredOutput
        }
    }
}
impl DlpHistoryMapping {
    pub(super) fn policy_for_target(
        &self,
        lease: &DlpRequestLease,
        binding: &ActualAttemptBinding,
        body: &Value,
        target: DlpHistoryTarget,
    ) -> HistoryLeafPolicy {
        let target = match target {
            DlpHistoryTarget::ResponsesLeaf {
                input_index,
                field: DlpHistoryLeafField::OutputText,
                part_index,
            } if body
                .get("input")
                .and_then(Value::as_array)
                .and_then(|a| a.get(input_index))
                .and_then(|i| i.get("type"))
                .and_then(Value::as_str)
                == Some("reasoning") =>
            {
                DlpHistoryTarget::ResponsesLeaf {
                    input_index,
                    field: DlpHistoryLeafField::ReasoningContent,
                    part_index,
                }
            }
            other => other,
        };
        if self.validates_target(lease, binding, body, target) {
            match target {
                DlpHistoryTarget::ChatContent { .. }
                | DlpHistoryTarget::ResponsesLeaf {
                    field: DlpHistoryLeafField::OutputText,
                    ..
                } => HistoryLeafPolicy::DeliveredOutput,
                _ => HistoryLeafPolicy::DeliveredReasoning,
            }
        } else {
            HistoryLeafPolicy::Strict
        }
    }
}

pub(super) fn visit_request_references<'a>(
    body: &'a Value,
    projection: Option<&DlpHistoryProjection<'_>>,
    visitor: impl FnMut(&'a str) -> Result<(), TokenError>,
) -> Result<(), TokenError> {
    visit_references_with_policy(
        body,
        |path| {
            history_target_for_path(path).map_or(HistoryLeafPolicy::Strict, |target| {
                projection.map_or(HistoryLeafPolicy::Strict, |p| {
                    p.policy_for_target(body, target)
                })
            })
        },
        visitor,
    )
}

pub(super) fn visit_references_with_policy<'a>(
    body: &'a Value,
    mut policy: impl FnMut(&[HistoryPathStep<'_>]) -> HistoryLeafPolicy,
    mut visitor: impl FnMut(&'a str) -> Result<(), TokenError>,
) -> Result<(), TokenError> {
    fn walk<'a>(
        value: &'a Value,
        path: &mut [HistoryPathStep<'a>; 129],
        depth: usize,
        policy: &mut impl FnMut(&[HistoryPathStep<'_>]) -> HistoryLeafPolicy,
        visitor: &mut impl FnMut(&'a str) -> Result<(), TokenError>,
    ) -> Result<(), TokenError> {
        if depth >= path.len() {
            return Err(TokenError::CapacityExceeded);
        }
        match value {
            Value::String(text) => policy(&path[..depth])
                .for_request_literals()
                .visit_references(text, |r| visitor(&text[r])),
            Value::Array(items) => {
                for (index, item) in items.iter().enumerate() {
                    path[depth] = HistoryPathStep::Index(index);
                    walk(item, path, depth + 1, policy, visitor)?;
                }
                Ok(())
            }
            Value::Object(fields) => {
                for (key, value) in fields {
                    // Keys never inherit a text-leaf exception.
                    super::output_text_matching::visit_complete_references(key, |r| {
                        visitor(&key[r])
                    })?;
                    path[depth] = HistoryPathStep::Field(key);
                    walk(value, path, depth + 1, policy, visitor)?;
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }
    walk(
        body,
        &mut [HistoryPathStep::Field(""); 129],
        0,
        &mut policy,
        &mut visitor,
    )
}

impl DlpHistoryProjection<'_> {
    /// Identity conversion records only exact source coordinates; allocation failure
    /// discards the whole optional map, never the request or a partial proof.
    pub(super) fn responses_mapping(&self, body: &Value) -> Option<DlpHistoryMapping> {
        let mut mapping = self.mapping()?;
        let items = body.get("input")?.as_array()?;
        for (input_index, item) in items.iter().enumerate() {
            for (field, container) in [
                (DlpHistoryLeafField::OutputText, "content"),
                (DlpHistoryLeafField::ReasoningSummary, "summary"),
                (DlpHistoryLeafField::ReasoningContent, "content"),
                (DlpHistoryLeafField::CallReasoning, "reasoning_content"),
            ] {
                let count = if field == DlpHistoryLeafField::CallReasoning {
                    usize::from(item.get(container).is_some())
                } else {
                    item.get(container)
                        .and_then(Value::as_array)
                        .map_or(0, Vec::len)
                };
                for part_index in 0..count {
                    let target = DlpHistoryTarget::ResponsesLeaf {
                        input_index,
                        field,
                        part_index,
                    };
                    if self.policy_for_target(body, target) == HistoryLeafPolicy::Strict {
                        continue;
                    }
                    let Some(source) =
                        self.source_for_responses_leaf(input_index, field, part_index)
                    else {
                        continue;
                    };
                    mapping.record(target, &[source], DlpHistorySeparator::None);
                    if mapping.failed() {
                        return None;
                    }
                }
            }
        }
        Some(mapping)
    }
}

impl<'a> DlpHistoryProjection<'a> {
    pub(super) fn restrict_to_origin(&mut self, origin: &'a Value) {
        self.origin = Some(origin);
    }
}
impl DlpHistoryMapping {
    pub(super) fn validates_all(
        &self,
        lease: &DlpRequestLease,
        binding: &ActualAttemptBinding,
        body: &Value,
    ) -> bool {
        !self.failed
            && self
                .entries
                .as_slice()
                .iter()
                .flatten()
                .all(|(target, _)| self.validates_target(lease, binding, body, *target))
    }
}
