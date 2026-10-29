//! Private immutable JSON copies with conservative, pre-build payload accounting.
//!
//! Rebuild by logical length: `Value::clone` can inherit spare IndexMap capacity.
//! Node/entry charges cover the pinned serde_json/indexmap representation, not
//! allocator bookkeeping, process RSS, physical OOM recovery or zeroization.
use super::{
    payload_budget::{PayloadBudget, Reservation},
    payload_metadata::PayloadVec,
    tokenization::TokenError,
};
use serde::de::{self, Deserialize, Deserializer, MapAccess, SeqAccess, Visitor};
use serde_json::{Map, Value};
use std::fmt;
use std::io::{self, Write};

const MAX_DEPTH: usize = 64;
const MAX_NODES: usize = 65_536;
const NODE_BYTES: usize = 128;
// For locked indexmap 2.13: bucket storage plus rounded hash indices/control
// bytes, even for small maps. Never clone the source's potentially sparse table.
const ENTRY_BYTES: usize = 512;
const _: () = assert!(std::mem::size_of::<Value>() <= NODE_BYTES);
const _: () = assert!(4 * std::mem::size_of::<Value>() <= 512);

// No Debug/Serialize/Clone, mutable borrow or unguarded extraction.
pub(super) struct PayloadJson {
    // Field order releases all JSON allocations before their reservation.
    value: Value,
    reservation: Reservation,
    budget: PayloadBudget,
}

// A frozen, allocation-free projection of the final enriched request.
// Missing fields are selected before sizing; both passes traverse identical borrows.
pub(crate) const CHAT_ENRICHMENT_FIELDS: [&str; 8] = [
    "name",
    "namespace",
    "arguments",
    "input",
    "status",
    "execution",
    "reasoning_content",
    "reasoning",
];
#[derive(Clone, Copy)]
pub(crate) struct ProjectedItem<'a> {
    pub(crate) value: &'a Value,
    pub(crate) cached: Option<&'a Value>,
    pub(crate) missing: u8,
}
#[derive(Clone, Copy)]
pub(super) enum JsonProjection<'a> {
    Value(&'a Value),
    Namespace(NamespaceProjection<'a>),
    NamespaceTools(&'a NamespacePlan<'a>),
    FlatText(StackToolName),
    Text(&'a str),
    PromptCache {
        body: &'a Value,
        key: &'a str,
    },
    Body {
        body: &'a Value,
        items: &'a [ProjectedItem<'a>],
    },
    Input(&'a [ProjectedItem<'a>]),
    Item(ProjectedItem<'a>),
}
impl<'a> JsonProjection<'a> {
    fn fields(
        self,
        mut visit: impl FnMut(&'a str, Self) -> Result<(), TokenError>,
    ) -> Result<usize, TokenError> {
        if let Self::Namespace(projected) = self {
            return projected.fields(visit);
        }
        let (value, additions) = match self {
            Self::Body { body, .. } | Self::PromptCache { body, .. } => (body, None),
            Self::Item(item) => (item.value, Some(item)),
            _ => return Err(TokenError::RedactionFailed),
        };
        let fields = value.as_object().ok_or(TokenError::RedactionFailed)?;
        let mut count = 0usize;
        for (key, value) in fields {
            let projected = if let Self::PromptCache { key: text, .. } = self {
                if key == "prompt_cache_key" {
                    Self::Text(text)
                } else {
                    Self::Value(value)
                }
            } else if let Self::Body { items, .. } = self {
                if key == "input" {
                    Self::Input(items)
                } else {
                    Self::Value(value)
                }
            } else if let Some(item) = additions {
                let selected = CHAT_ENRICHMENT_FIELDS
                    .iter()
                    .position(|field| *field == key)
                    .filter(|index| item.missing & (1u8 << index) != 0);
                Self::Value(
                    selected
                        .and_then(|_| item.cached.and_then(|cached| cached.get(key)))
                        .unwrap_or(value),
                )
            } else {
                Self::Value(value)
            };
            visit(key, projected)?;
            count = count.checked_add(1).ok_or(TokenError::CapacityExceeded)?;
        }
        if let Self::PromptCache { key, .. } = self {
            if !fields.contains_key("prompt_cache_key") {
                visit("prompt_cache_key", Self::Text(key))?;
                count = count.checked_add(1).ok_or(TokenError::CapacityExceeded)?;
            }
        }
        if let Some(item) = additions {
            if let Some(cached) = item.cached.and_then(Value::as_object) {
                // Match the cache field order used by the legacy retain/extend path.
                for (key, value) in cached {
                    if !fields.contains_key(key)
                        && CHAT_ENRICHMENT_FIELDS
                            .iter()
                            .position(|field| *field == key)
                            .is_some_and(|index| item.missing & (1u8 << index) != 0)
                    {
                        visit(key, Self::Value(value))?;
                        count = count.checked_add(1).ok_or(TokenError::CapacityExceeded)?;
                    }
                }
            }
        }
        Ok(count)
    }
}

// Borrowed request-only namespace plans. Container backing is charged by PayloadVec;
// stack names contain no heap allocation or restoration authority.
use crate::proxy::providers::transform_codex_responses_namespace::StackToolName;
struct NamespaceTool<'a> {
    value: &'a Value,
    flat: Option<StackToolName>,
    keep: bool,
}
struct NamespaceOwner<'a> {
    namespace: &'a str,
    name: &'a str,
    flat: StackToolName,
}
pub(super) struct NamespacePlan<'a> {
    tools: PayloadVec<NamespaceTool<'a>>,
    owners: PayloadVec<NamespaceOwner<'a>>,
}
impl<'a> NamespacePlan<'a> {
    fn new(body: &'a Value, budget: PayloadBudget) -> Result<Option<Self>, TokenError> {
        let Some(tools) = body.get("tools").and_then(Value::as_array) else {
            return Ok(None);
        };
        if !tools
            .iter()
            .any(|tool| tool.get("type").and_then(Value::as_str) == Some("namespace"))
        {
            return Ok(None);
        }
        let mut top = PayloadVec::new(budget.clone())?;
        let mut plan = Self {
            tools: PayloadVec::new(budget.clone())?,
            owners: PayloadVec::new(budget.clone())?,
        };
        for tool in tools {
            if matches!(
                tool.get("type").and_then(Value::as_str),
                Some("function" | "custom")
            ) {
                if let Some(name) = tool
                    .get("name")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|name| !name.is_empty())
                {
                    top.push(name)?;
                }
            }
        }
        top.sort_unstable_by(|a, b| a.cmp(b));
        let mut indices = PayloadVec::new(budget)?;
        for tool in tools {
            if tool.get("type").and_then(Value::as_str) != Some("namespace") {
                plan.tools.push(NamespaceTool {
                    value: tool,
                    flat: None,
                    keep: true,
                })?;
                continue;
            }
            let Some(namespace) = tool.get("name").and_then(Value::as_str).map(str::trim) else {
                continue;
            };
            let Some(children) = tool
                .get("tools")
                .or_else(|| tool.get("children"))
                .and_then(Value::as_array)
            else {
                continue;
            };
            for child in children {
                if child.get("type").and_then(Value::as_str) != Some("function") {
                    continue;
                }
                let Some(name) = child
                    .get("name")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|name| !name.is_empty())
                else {
                    continue;
                };
                let flat =
                    StackToolName::new(namespace, name).ok_or(TokenError::CapacityExceeded)?;
                if !namespace.is_empty() {
                    if top.as_slice().binary_search(&flat.as_str()).is_ok() {
                        return Err(TokenError::RedactionFailed);
                    }
                    plan.owners.push(NamespaceOwner {
                        namespace,
                        name,
                        flat,
                    })?;
                }
                indices.push(plan.tools.len())?;
                plan.tools.push(NamespaceTool {
                    value: child,
                    flat: Some(flat),
                    keep: true,
                })?;
            }
        }
        plan.owners
            .sort_unstable_by(|a, b| a.flat.as_str().cmp(b.flat.as_str()));
        for pair in plan.owners.as_slice().windows(2) {
            if pair[0].flat.as_str() == pair[1].flat.as_str()
                && (pair[0].namespace != pair[1].namespace || pair[0].name != pair[1].name)
            {
                return Err(TokenError::RedactionFailed);
            }
        }
        let key = |index: usize| {
            plan.tools.as_slice()[index]
                .flat
                .as_ref()
                .map(StackToolName::as_str)
                .unwrap_or("")
        };
        indices.sort_unstable_by(|a, b| key(*a).cmp(key(*b)).then(a.cmp(b)));
        for pair in indices.as_slice().windows(2) {
            let a = plan.tools.as_slice()[pair[0]]
                .flat
                .as_ref()
                .map(StackToolName::as_str);
            let b = plan.tools.as_slice()[pair[1]]
                .flat
                .as_ref()
                .map(StackToolName::as_str);
            if a == b {
                if let Some(tool) = plan.tools.get_mut(pair[1]) {
                    tool.keep = false;
                }
            }
        }
        Ok(Some(plan))
    }
    fn rewrite(&self, value: &Value) -> Option<StackToolName> {
        let namespace = value.get("namespace")?.as_str()?.trim();
        let name = value.get("name")?.as_str()?.trim();
        if namespace.is_empty() || name.is_empty() {
            return None;
        }
        let flat = StackToolName::new(namespace, name)?;
        let index = self
            .owners
            .as_slice()
            .binary_search_by(|owner| owner.flat.as_str().cmp(flat.as_str()))
            .ok()?;
        let owner = &self.owners.as_slice()[index];
        (owner.namespace == namespace && owner.name == name).then_some(flat)
    }
}
#[derive(Clone, Copy)]
enum NamespaceMode {
    Root,
    History,
    Call,
    Lift(StackToolName),
}
#[derive(Clone, Copy)]
pub(super) struct NamespaceProjection<'a> {
    value: &'a Value,
    plan: &'a NamespacePlan<'a>,
    mode: NamespaceMode,
}
impl<'a> NamespaceProjection<'a> {
    fn fields(
        self,
        mut visit: impl FnMut(&'a str, JsonProjection<'a>) -> Result<(), TokenError>,
    ) -> Result<usize, TokenError> {
        let fields = self.value.as_object().ok_or(TokenError::RedactionFailed)?;
        let recursive = matches!(self.mode, NamespaceMode::History)
            && self.value.get("type").and_then(Value::as_str) != Some("function_call");
        let rewrite = match self.mode {
            NamespaceMode::Lift(flat) => Some(flat),
            NamespaceMode::Call | NamespaceMode::History if !recursive => {
                self.plan.rewrite(self.value)
            }
            _ => None,
        };
        let remove_namespace = rewrite.is_some() && !matches!(self.mode, NamespaceMode::Lift(_));
        let mut count = 0;
        // serde_json's legacy remove uses swap_remove: the last field moves
        // into the removed namespace slot. Preserve that observable byte order.
        let removed = remove_namespace
            .then(|| fields.keys().position(|key| key == "namespace"))
            .flatten();
        for (index, (key, value)) in fields.iter().enumerate() {
            let (key, value) = if let Some(removed) = removed {
                if index == fields.len() - 1 {
                    continue;
                }
                if index == removed {
                    fields
                        .iter()
                        .next_back()
                        .ok_or(TokenError::RedactionFailed)?
                } else {
                    (key, value)
                }
            } else {
                (key, value)
            };
            let projected = if key == "name" && rewrite.is_some() {
                JsonProjection::FlatText(rewrite.ok_or(TokenError::RedactionFailed)?)
            } else if matches!(self.mode, NamespaceMode::Root) && key == "tools" {
                JsonProjection::NamespaceTools(self.plan)
            } else if matches!(self.mode, NamespaceMode::Root) && key == "tool_choice" {
                if value.get("type").and_then(Value::as_str) == Some("namespace") {
                    JsonProjection::Text("auto")
                } else {
                    JsonProjection::Namespace(Self {
                        value,
                        mode: NamespaceMode::Call,
                        ..self
                    })
                }
            } else if (matches!(self.mode, NamespaceMode::Root) && key == "input") || recursive {
                JsonProjection::Namespace(Self {
                    value,
                    mode: NamespaceMode::History,
                    ..self
                })
            } else {
                JsonProjection::Value(value)
            };
            visit(key, projected)?;
            count += 1;
        }
        Ok(count)
    }
}

// Frozen tail semantics: P(merge(P(source), patch)). The first filter is
// represented by visible source membership, not by an intermediate tree.
// Arrays retain the nearest object-key context, just like the legacy filter.
#[derive(Clone, Copy)]
struct OutboundTail<'a> {
    source: &'a Value,
    patch: Option<&'a Value>,
    schema_names: bool,
    root: bool,
}

impl<'a> OutboundTail<'a> {
    fn selected(self) -> Result<Self, TokenError> {
        match self.patch {
            Some(patch) if !(self.source.is_object() && patch.is_object()) => {
                // Legacy replacement compares the already-filtered source with
                // the raw patch. Value equality preserves signed-zero bytes on
                // no-op replacements, including inside whole-array patches.
                let mut nodes = 0;
                let unchanged = self.filtered_equals(patch, 0, &mut nodes)?;
                Ok(Self {
                    source: if unchanged { self.source } else { patch },
                    patch: None,
                    ..self
                })
            }
            _ => Ok(self),
        }
    }

    fn filtered_equals(
        self,
        raw: &Value,
        depth: usize,
        nodes: &mut usize,
    ) -> Result<bool, TokenError> {
        if depth > MAX_DEPTH || *nodes >= MAX_NODES {
            return Err(TokenError::CapacityExceeded);
        }
        *nodes += 1;
        match (self.source, raw) {
            (Value::Object(source), Value::Object(patch)) => {
                let mut visible = 0;
                for (key, value) in source {
                    if !self.visible(key) {
                        continue;
                    }
                    visible += 1;
                    let Some(raw) = patch.get(key) else {
                        return Ok(false);
                    };
                    let child = Self {
                        source: value,
                        patch: None,
                        schema_names: matches!(
                            key.as_str(),
                            "properties" | "patternProperties" | "definitions" | "$defs"
                        ),
                        root: false,
                    };
                    if !child.filtered_equals(raw, depth + 1, nodes)? {
                        return Ok(false);
                    }
                }
                Ok(visible == patch.len())
            }
            (Value::Array(source), Value::Array(patch)) => {
                if source.len() != patch.len() {
                    return Ok(false);
                }
                for (source, raw) in source.iter().zip(patch) {
                    if !(Self {
                        source,
                        patch: None,
                        root: false,
                        ..self
                    })
                    .filtered_equals(raw, depth + 1, nodes)?
                    {
                        return Ok(false);
                    }
                }
                Ok(true)
            }
            // Container mismatches must not recurse via unbounded Value equality.
            (Value::Object(_) | Value::Array(_), _) | (_, Value::Object(_) | Value::Array(_)) => {
                Ok(false)
            }
            (source, raw) => Ok(source == raw),
        }
    }

    fn visible(self, key: &str) -> bool {
        !key.starts_with('_') || self.schema_names
    }

    fn patch_field(self, key: &str) -> Option<&'a Value> {
        if self.root && key == "stream" {
            None
        } else {
            self.patch.and_then(|patch| patch.get(key))
        }
    }

    fn child(self, key: &str) -> Result<Self, TokenError> {
        let source = self.source.get(key).filter(|_| self.visible(key));
        let patch = self.patch_field(key);
        let (source, patch) = match (source, patch) {
            (Some(source), patch) => (source, patch),
            (None, Some(patch)) => (patch, None),
            (None, None) => return Err(TokenError::RedactionFailed),
        };
        Ok(Self {
            source,
            patch,
            schema_names: matches!(
                key,
                "properties" | "patternProperties" | "definitions" | "$defs"
            ),
            root: false,
        })
    }

    // Sorting uses a separately precharged plan of borrowed keys. No owned
    // path/key copies, uncharged sorting Vec, or arbitrary comparison limit.
    fn keys(self, budget: &PayloadBudget) -> Result<PayloadVec<&'a str>, TokenError> {
        let source = self.source.as_object().ok_or(TokenError::RedactionFailed)?;
        let patch = self.patch.and_then(Value::as_object);
        let mut keys = PayloadVec::new(budget.clone())?;
        for key in source
            .keys()
            .chain(patch.into_iter().flat_map(|map| map.keys()))
        {
            if self.visible(key) && !(self.root && key == "stream" && !source.contains_key(key)) {
                keys.push(key.as_str())?;
            }
        }
        keys.sort_unstable_by(|left, right| left.cmp(right));
        keys.dedup_by(|left, right| *left == *right);
        Ok(keys)
    }
}

/// A borrowed string-value target in one exact immutable source tree.
/// No owned text, formatting, serialization or mutable authority is exposed.
pub(super) struct JsonStringReplacement<'a> {
    pub target: &'a Value,
    pub text: &'a str,
}

const MAX_REPLACEMENTS: usize = 4096;
const MAX_REPLACEMENT_COMPARISONS: usize = 4 * 1024 * 1024;

// Fixed stack state: membership/duplicate checks allocate no uncharged index.
struct Replacements<'a, 'b> {
    edits: &'a [JsonStringReplacement<'b>],
    seen: [u64; MAX_REPLACEMENTS / 64],
    matched: usize,
    comparisons: usize,
}

impl<'a, 'b> Replacements<'a, 'b> {
    fn new(edits: &'a [JsonStringReplacement<'b>]) -> Result<Self, TokenError> {
        if edits.len() > MAX_REPLACEMENTS {
            return Err(TokenError::CapacityExceeded);
        }
        if edits.iter().any(|edit| !edit.target.is_string()) {
            return Err(TokenError::RedactionFailed);
        }
        Ok(Self {
            edits,
            seen: [0; MAX_REPLACEMENTS / 64],
            matched: 0,
            comparisons: 0,
        })
    }

    fn text<'v>(&mut self, value: &'v Value, original: &'v str) -> Result<&'v str, TokenError>
    where
        'b: 'v,
    {
        self.comparisons = self
            .comparisons
            .checked_add(self.edits.len())
            .filter(|work| *work <= MAX_REPLACEMENT_COMPARISONS)
            .ok_or(TokenError::CapacityExceeded)?;
        let mut replacement = None;
        for (index, edit) in self.edits.iter().enumerate() {
            if std::ptr::eq(edit.target, value) {
                let mask = 1u64 << (index % 64);
                if replacement.is_some() || self.seen[index / 64] & mask != 0 {
                    return Err(TokenError::RedactionFailed);
                }
                self.seen[index / 64] |= mask;
                self.matched += 1;
                replacement = Some(edit.text);
            }
        }
        Ok(replacement.unwrap_or(original))
    }

    fn complete(&self) -> Result<(), TokenError> {
        if self.matched != self.edits.len() {
            return Err(TokenError::RedactionFailed);
        }
        Ok(())
    }
}

struct Size {
    bytes: usize,
    nodes: usize,
    limit: usize,
}

impl Size {
    fn add(&mut self, bytes: usize) -> Result<(), TokenError> {
        self.bytes = self
            .bytes
            .checked_add(bytes)
            .filter(|next| *next <= self.limit && *next <= isize::MAX as usize)
            .ok_or(TokenError::CapacityExceeded)?;
        Ok(())
    }

    fn value(
        &mut self,
        value: &Value,
        depth: usize,
        replacements: &mut Replacements<'_, '_>,
    ) -> Result<(), TokenError> {
        if depth > MAX_DEPTH || self.nodes >= MAX_NODES {
            return Err(TokenError::CapacityExceeded);
        }
        self.nodes += 1;
        self.add(NODE_BYTES)?;
        match value {
            Value::String(text) => self.add(replacements.text(value, text)?.len())?,
            // Count the encoded length without allocating, including if a future
            // feature-unified Number stores its representation in a String.
            Value::Number(number) => {
                serde_json::to_writer(self, number).map_err(|_| TokenError::CapacityExceeded)?
            }
            Value::Array(values) => {
                for value in values {
                    self.value(value, depth + 1, replacements)?;
                }
            }
            Value::Object(fields) => {
                self.add(
                    fields
                        .len()
                        .checked_mul(ENTRY_BYTES)
                        .ok_or(TokenError::CapacityExceeded)?,
                )?;
                for (key, value) in fields {
                    self.add(key.len())?;
                    self.value(value, depth + 1, replacements)?;
                }
            }
            _ => {}
        }
        Ok(())
    }
}

impl Write for Size {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.add(bytes.len())
            .map_err(|_| io::Error::other("JSON payload capacity exceeded"))?;
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl PayloadJson {
    /// Build the filtered, canonical, overridden final tree directly. Invalid
    /// non-object override roots are ignored, matching the legacy entry point.
    pub(super) fn try_outbound_tail(
        source: &Value,
        patch: Option<&Value>,
        budget: PayloadBudget,
        local_limit: usize,
    ) -> Result<Self, TokenError> {
        fn size(
            tail: OutboundTail<'_>,
            state: &mut Size,
            depth: usize,
            budget: &PayloadBudget,
        ) -> Result<(), TokenError> {
            let tail = tail.selected()?;
            if depth > MAX_DEPTH || state.nodes >= MAX_NODES {
                return Err(TokenError::CapacityExceeded);
            }
            match tail.source {
                Value::Object(_) => {
                    state.nodes += 1;
                    state.add(NODE_BYTES)?;
                    let keys = tail.keys(budget)?;
                    for &key in keys.as_slice() {
                        state.add(ENTRY_BYTES)?;
                        state.add(key.len())?;
                        size(tail.child(key)?, state, depth + 1, budget)?;
                    }
                }
                Value::Array(values) => {
                    state.nodes += 1;
                    state.add(NODE_BYTES)?;
                    for value in values {
                        size(
                            OutboundTail {
                                source: value,
                                patch: None,
                                root: false,
                                ..tail
                            },
                            state,
                            depth + 1,
                            budget,
                        )?;
                    }
                }
                value => state.value(value, depth, &mut Replacements::new(&[])?)?,
            }
            Ok(())
        }
        let tail = OutboundTail {
            source,
            patch: patch.filter(|patch| patch.is_object()),
            schema_names: false,
            root: true,
        };
        let mut sizing = Size {
            bytes: 0,
            nodes: 0,
            limit: local_limit,
        };
        size(tail, &mut sizing, 0, &budget)?;
        let mut owner = Self {
            value: Value::Null,
            reservation: budget.reserve(sizing.bytes)?,
            budget,
        };
        #[cfg(test)]
        owner.observe(test_support::Phase::BeforeBuild)?;
        owner.value = owner.rebuild_outbound_tail(tail)?;
        #[cfg(test)]
        owner.observe(test_support::Phase::AfterBuild)?;
        Ok(owner)
    }

    fn rebuild_outbound_tail(&self, tail: OutboundTail<'_>) -> Result<Value, TokenError> {
        let tail = tail.selected()?;
        match tail.source {
            Value::Object(_) => {
                let keys = tail.keys(&self.budget)?;
                #[cfg(test)]
                self.observe(test_support::Phase::BeforeAllocation)?;
                let mut fields = Map::with_capacity(keys.len());
                for &key in keys.as_slice() {
                    fields.insert(
                        self.text(key)?,
                        self.rebuild_outbound_tail(tail.child(key)?)?,
                    );
                }
                Ok(Value::Object(fields))
            }
            Value::Array(values) => {
                #[cfg(test)]
                self.observe(test_support::Phase::BeforeAllocation)?;
                let mut output = Vec::new();
                output
                    .try_reserve_exact(values.len())
                    .map_err(|_| TokenError::Unavailable)?;
                if output.capacity() > values.len() {
                    return Err(TokenError::CapacityExceeded);
                }
                for value in values {
                    output.push(self.rebuild_outbound_tail(OutboundTail {
                        source: value,
                        patch: None,
                        root: false,
                        ..tail
                    })?);
                }
                Ok(Value::Array(output))
            }
            value => self.rebuild(value, &mut Replacements::new(&[])?),
        }
    }

    /// Rebuild only existing string values selected by source-node identity.
    /// Edits are frozen borrows; their metadata/owner accounting belongs to caller.
    pub(super) fn try_with_replacements(
        value: &Value,
        edits: &[JsonStringReplacement<'_>],
        budget: PayloadBudget,
        local_limit: usize,
    ) -> Result<Self, TokenError> {
        let mut replacements = Replacements::new(edits)?;
        let mut size = Size {
            bytes: 0,
            nodes: 0,
            limit: local_limit,
        };
        size.value(value, 0, &mut replacements)?;
        replacements.complete()?;
        // Preflight validated every target and the complete final-tree charge.
        // No dynamic lookup table or candidate allocation precedes this point.
        let mut owner = Self {
            value: Value::Null,
            reservation: budget.reserve(size.bytes)?,
            budget,
        };
        #[cfg(test)]
        owner.observe(test_support::Phase::BeforeBuild)?;
        let mut replacements = Replacements::new(edits)?;
        owner.value = owner.rebuild(value, &mut replacements)?;
        replacements.complete()?;
        #[cfg(test)]
        owner.observe(test_support::Phase::AfterBuild)?;
        Ok(owner)
    }

    /// Freeze the final key and reserve the whole independent candidate before building.
    /// Preserve source field order on replacement and append a missing field last.
    pub(super) fn try_with_prompt_cache_key(
        body: &Value,
        key: &str,
        budget: PayloadBudget,
        local_limit: usize,
    ) -> Result<Self, TokenError> {
        Self::try_from_projection(
            JsonProjection::PromptCache { body, key },
            budget,
            local_limit,
        )
    }

    pub(super) fn try_namespace_body(
        body: &Value,
        budget: PayloadBudget,
        local_limit: usize,
    ) -> Result<Option<Self>, TokenError> {
        let Some(plan) = NamespacePlan::new(body, budget.clone())? else {
            return Ok(None);
        };
        Self::try_from_projection(
            JsonProjection::Namespace(NamespaceProjection {
                value: body,
                plan: &plan,
                mode: NamespaceMode::Root,
            }),
            budget,
            local_limit,
        )
        .map(Some)
    }

    pub(super) fn try_from_projection(
        projection: JsonProjection<'_>,
        budget: PayloadBudget,
        local_limit: usize,
    ) -> Result<Self, TokenError> {
        fn size(
            projection: JsonProjection<'_>,
            state: &mut Size,
            depth: usize,
        ) -> Result<(), TokenError> {
            if let JsonProjection::Value(value) = projection {
                return state.value(value, depth, &mut Replacements::new(&[])?);
            }
            if let JsonProjection::Namespace(projected) = projection {
                if !projected.value.is_object()
                    && (!projected.value.is_array()
                        || !matches!(projected.mode, NamespaceMode::History))
                {
                    return state.value(projected.value, depth, &mut Replacements::new(&[])?);
                }
            }
            if let JsonProjection::Item(item) = projection {
                if item.cached.is_none() {
                    return state.value(item.value, depth, &mut Replacements::new(&[])?);
                }
            }
            if depth > MAX_DEPTH || state.nodes >= MAX_NODES {
                return Err(TokenError::CapacityExceeded);
            }
            state.nodes += 1;
            state.add(NODE_BYTES)?;
            if let JsonProjection::Text(text) = projection {
                return state.add(text.len());
            }
            if let JsonProjection::FlatText(text) = projection {
                return state.add(text.as_str().len());
            }
            if let JsonProjection::NamespaceTools(plan) = projection {
                for tool in plan.tools.as_slice().iter().filter(|tool| tool.keep) {
                    let item = match tool.flat {
                        Some(flat) => JsonProjection::Namespace(NamespaceProjection {
                            value: tool.value,
                            plan,
                            mode: NamespaceMode::Lift(flat),
                        }),
                        None => JsonProjection::Value(tool.value),
                    };
                    size(item, state, depth + 1)?;
                }
            } else if let JsonProjection::Namespace(projected) = projection {
                if let Some(items) = projected.value.as_array() {
                    for value in items {
                        size(
                            JsonProjection::Namespace(NamespaceProjection { value, ..projected }),
                            state,
                            depth + 1,
                        )?;
                    }
                } else {
                    projection.fields(|key, value| {
                        state.add(ENTRY_BYTES)?;
                        state.add(key.len())?;
                        size(value, state, depth + 1)
                    })?;
                }
            } else if let JsonProjection::Input(items) = projection {
                for item in items {
                    size(JsonProjection::Item(*item), state, depth + 1)?;
                }
            } else {
                projection.fields(|key, value| {
                    state.add(ENTRY_BYTES)?;
                    state.add(key.len())?;
                    size(value, state, depth + 1)
                })?;
            }
            Ok(())
        }
        let mut sizing = Size {
            bytes: 0,
            nodes: 0,
            limit: local_limit,
        };
        size(projection, &mut sizing, 0)?;
        let mut owner = Self {
            value: Value::Null,
            reservation: budget.reserve(sizing.bytes)?,
            budget,
        };
        #[cfg(test)]
        owner.observe(test_support::Phase::BeforeBuild)?;
        owner.value = owner.rebuild_projection(projection)?;
        #[cfg(test)]
        owner.observe(test_support::Phase::AfterBuild)?;
        Ok(owner)
    }

    fn rebuild_projection(&self, projection: JsonProjection<'_>) -> Result<Value, TokenError> {
        if let JsonProjection::Value(value) = projection {
            return self.rebuild(value, &mut Replacements::new(&[])?);
        }
        if let JsonProjection::Text(text) = projection {
            return Ok(Value::String(self.text(text)?));
        }
        if let JsonProjection::FlatText(text) = projection {
            return Ok(Value::String(self.text(text.as_str())?));
        }
        if let JsonProjection::Namespace(projected) = projection {
            if !projected.value.is_object()
                && (!projected.value.is_array()
                    || !matches!(projected.mode, NamespaceMode::History))
            {
                return self.rebuild(projected.value, &mut Replacements::new(&[])?);
            }
        }
        if let JsonProjection::Item(item) = projection {
            if item.cached.is_none() {
                return self.rebuild(item.value, &mut Replacements::new(&[])?);
            }
        }
        #[cfg(test)]
        self.observe(test_support::Phase::BeforeAllocation)?;
        if let JsonProjection::NamespaceTools(plan) = projection {
            let count = plan
                .tools
                .as_slice()
                .iter()
                .filter(|tool| tool.keep)
                .count();
            let mut values = Vec::new();
            values
                .try_reserve_exact(count)
                .map_err(|_| TokenError::Unavailable)?;
            if values.capacity() > count {
                return Err(TokenError::CapacityExceeded);
            }
            for tool in plan.tools.as_slice().iter().filter(|tool| tool.keep) {
                let item = match tool.flat {
                    Some(flat) => JsonProjection::Namespace(NamespaceProjection {
                        value: tool.value,
                        plan,
                        mode: NamespaceMode::Lift(flat),
                    }),
                    None => JsonProjection::Value(tool.value),
                };
                values.push(self.rebuild_projection(item)?);
            }
            return Ok(Value::Array(values));
        }
        if let JsonProjection::Namespace(projected) = projection {
            if let Some(items) = projected.value.as_array() {
                let mut values = Vec::new();
                values
                    .try_reserve_exact(items.len())
                    .map_err(|_| TokenError::Unavailable)?;
                if values.capacity() > items.len() {
                    return Err(TokenError::CapacityExceeded);
                }
                for value in items {
                    values.push(self.rebuild_projection(JsonProjection::Namespace(
                        NamespaceProjection { value, ..projected },
                    ))?);
                }
                return Ok(Value::Array(values));
            }
        }
        if let JsonProjection::Input(items) = projection {
            let mut values = Vec::new();
            values
                .try_reserve_exact(items.len())
                .map_err(|_| TokenError::Unavailable)?;
            if values.capacity() > items.len() {
                return Err(TokenError::CapacityExceeded);
            }
            for item in items {
                values.push(self.rebuild_projection(JsonProjection::Item(*item))?);
            }
            Ok(Value::Array(values))
        } else {
            let count = projection.fields(|_, _| Ok(()))?;
            let mut fields = Map::with_capacity(count);
            projection.fields(|key, value| {
                fields.insert(self.text(key)?, self.rebuild_projection(value)?);
                Ok(())
            })?;
            Ok(Value::Object(fields))
        }
    }

    pub(super) fn try_from_value(
        value: &Value,
        budget: PayloadBudget,
        local_limit: usize,
    ) -> Result<Self, TokenError> {
        Self::try_with_replacements(value, &[], budget, local_limit)
    }

    pub(super) fn try_from_str(
        encoded: &str,
        budget: PayloadBudget,
        local_limit: usize,
    ) -> Result<Self, TokenError> {
        let (retained, scratch_bytes) = decoded_charges(encoded, local_limit)?;
        let mut owner = Self {
            value: Value::Null,
            reservation: budget.reserve(retained)?,
            budget,
        };
        // Declared after owner: parser locals and scratch are destroyed before
        // owner on error. Completed JSON is retained under its own reservation.
        let scratch = owner.budget.reserve(scratch_bytes)?;
        #[cfg(test)]
        owner.observe(test_support::Phase::BeforeParse)?;
        {
            // Reject duplicate keys before Value's decoder can overwrite them.
            let mut parser = serde_json::Deserializer::from_str(encoded);
            UniqueJson::deserialize(&mut parser)
                .and_then(|_| parser.end())
                .map_err(|_| TokenError::RedactionFailed)?;
        }
        owner.value = serde_json::from_str::<PlainJson>(encoded)
            .map_err(|_| TokenError::RedactionFailed)?
            .0;
        #[cfg(test)]
        owner.observe(test_support::Phase::AfterParse)?;
        drop(scratch);
        Ok(owner)
    }

    /// Transfer the tree and its charge together to a private lifetime owner.
    /// The caller must release the tree before dropping the reservation.
    pub(super) fn into_parts(self) -> (Value, Reservation) {
        let Self {
            value, reservation, ..
        } = self;
        (value, reservation)
    }

    pub(super) fn as_value(&self) -> &Value {
        &self.value
    }

    fn text(&self, text: &str) -> Result<String, TokenError> {
        #[cfg(test)]
        self.observe(test_support::Phase::BeforeAllocation)?;
        let mut copy = String::new();
        copy.try_reserve_exact(text.len())
            .map_err(|_| TokenError::Unavailable)?;
        if copy.capacity() > text.len() {
            return Err(TokenError::CapacityExceeded);
        }
        copy.push_str(text);
        Ok(copy)
    }

    fn rebuild(
        &self,
        value: &Value,
        replacements: &mut Replacements<'_, '_>,
    ) -> Result<Value, TokenError> {
        Ok(match value {
            Value::Null => Value::Null,
            Value::Bool(value) => Value::Bool(*value),
            Value::Number(value) => {
                #[cfg(test)]
                self.observe(test_support::Phase::BeforeAllocation)?;
                Value::Number(value.clone())
            }
            Value::String(text) => Value::String(self.text(replacements.text(value, text)?)?),
            Value::Array(values) => {
                #[cfg(test)]
                self.observe(test_support::Phase::BeforeAllocation)?;
                let mut copy = Vec::new();
                copy.try_reserve_exact(values.len())
                    .map_err(|_| TokenError::Unavailable)?;
                if copy.capacity() > values.len() {
                    return Err(TokenError::CapacityExceeded);
                }
                for value in values {
                    copy.push(self.rebuild(value, replacements)?);
                }
                Value::Array(copy)
            }
            Value::Object(fields) => {
                #[cfg(test)]
                self.observe(test_support::Phase::BeforeAllocation)?;
                // serde_json exposes no fallible map reservation. Its allocation
                // remains bounded and fully precharged, but OOM may still abort.
                let mut copy = Map::with_capacity(fields.len());
                for (key, value) in fields {
                    copy.insert(self.text(key)?, self.rebuild(value, replacements)?);
                }
                Value::Object(copy)
            }
        })
    }

    #[cfg(test)]
    fn observe(&self, phase: test_support::Phase) -> Result<(), TokenError> {
        test_support::emit(test_support::Observation {
            phase,
            charged: self.reservation.bytes(),
            used: self.budget.used(),
        })
    }
}

#[cfg(test)]
pub(super) mod test_support {
    use std::{cell::RefCell, marker::PhantomData, rc::Rc};

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub(in super::super) enum Phase {
        BeforeBuild,
        BeforeAllocation,
        AfterBuild,
        BeforeParse,
        AfterParse,
    }

    #[derive(Clone, Copy, Debug)]
    pub(in super::super) struct Observation {
        pub phase: Phase,
        pub charged: usize,
        pub used: usize,
    }

    pub(in super::super) enum Action {
        Continue,
        Fail,
    }

    type Hook = Box<dyn FnMut(Observation) -> Action>;
    thread_local! {
        static HOOK: RefCell<Option<Hook>> = const { RefCell::new(None) };
    }

    pub(in super::super) struct HookGuard {
        previous: Option<Hook>,
        _thread_bound: PhantomData<Rc<()>>,
    }

    impl Drop for HookGuard {
        fn drop(&mut self) {
            HOOK.with(|slot| slot.replace(self.previous.take()));
        }
    }

    pub(in super::super) fn install(
        hook: impl FnMut(Observation) -> Action + 'static,
    ) -> HookGuard {
        HookGuard {
            previous: HOOK.with(|slot| slot.replace(Some(Box::new(hook)))),
            _thread_bound: PhantomData,
        }
    }
    pub(in super::super) fn emit(observation: Observation) -> Result<(), super::TokenError> {
        HOOK.with(
            |slot| match slot.borrow_mut().as_mut().map(|hook| hook(observation)) {
                Some(Action::Fail) => Err(super::TokenError::Unavailable),
                _ => Ok(()),
            },
        )
    }
}

#[cfg(test)]
mod outbound_tail_tests {
    use super::*;
    use serde_json::json;
    use std::{cell::Cell, rc::Rc};

    fn equivalent(source: &Value, patch: Option<&Value>) {
        let source_before = serde_json::to_vec(source).expect("synthetic source");
        let patch_before = patch.map(|value| serde_json::to_vec(value).expect("synthetic patch"));
        let expected =
            crate::proxy::forwarder::legacy_outbound_tail_for_test(source.clone(), patch);
        let budget = PayloadBudget::with_limit(8 * 1024 * 1024);
        let output = PayloadJson::try_outbound_tail(source, patch, budget.clone(), budget.limit())
            .expect("synthetic tail");
        assert_eq!(output.as_value(), &expected);
        assert_eq!(
            serde_json::to_vec(output.as_value()).expect("synthetic output"),
            serde_json::to_vec(&expected).expect("synthetic legacy")
        );
        assert_eq!(
            serde_json::to_vec(source).expect("synthetic source"),
            source_before
        );
        assert_eq!(
            patch.map(|value| serde_json::to_vec(value).expect("synthetic patch")),
            patch_before
        );
        drop(output);
        assert_eq!(budget.used(), 0);
    }

    #[test]
    fn guarded_outbound_tail_equal_scalar_keeps_original_signed_zero_bytes() {
        equivalent(
            &json!({"temperature":-0.0}),
            Some(&json!({"temperature":0.0})),
        );
        equivalent(
            &json!({"temperature":0.0}),
            Some(&json!({"temperature":-0.0})),
        );
    }

    #[test]
    fn guarded_outbound_tail_equal_arrays_compare_filtered_source_to_raw_patch() {
        equivalent(
            &json!({"data":[{"z":-0.0,"_private":1}]}),
            Some(&json!({"data":[{"z":0.0}]})),
        );
        equivalent(
            &json!({"data":[{"z":-0.0}]}),
            Some(&json!({"data":[{"z":0.0,"_private":1}]})),
        );
        equivalent(
            &json!({"properties":[[{"_name":{"z":-0.0,"_private":1}}]]}),
            Some(&json!({"properties":[[{"_name":{"z":0.0}}]]})),
        );
        equivalent(
            &json!({"data":[[{"z":-0.0,"nested":{"old":true}}]]}),
            Some(&json!({"data":[[{"z":0.0,"nested":{"new":true}}]]})),
        );
        equivalent(
            &json!({"data":[{"z":-0.0,"nested":{"_private":1,"keep":2}}],"sibling":1}),
            Some(&json!({"data":[{"z":0.0,"nested":{"keep":2}}],"sibling":2})),
        );
        for name in ["properties", "patternProperties", "definitions", "$defs"] {
            equivalent(
                &json!({name:[[{"_name":{"z":-0.0,"_private":1}}]]}),
                Some(&json!({name:[[{"_name":{"z":0.0}}]]})),
            );
            equivalent(
                &json!({name:[[{"_name":{"z":-0.0}}]]}),
                Some(&json!({name:[[{"_name":{"z":0.0,"_private":1}}]]})),
            );
        }
    }

    #[test]
    fn guarded_outbound_tail_matches_actual_legacy_values_and_bytes() {
        let values = [
            Value::Null,
            json!(false),
            json!(7),
            json!("synthetic"),
            json!([{"_private":1,"z":2,"a":3},[null]]),
            json!({"z":0,"a":{"z":1,"a":2,"_private":3},"stream":false,"_private":{"old":1}}),
            json!({"array":[{"stream":false,"_private":1}],"nested":{"stream":false}}),
        ];
        let patches = [
            json!(null),
            json!(false),
            json!([]),
            json!({}),
            json!({"stream":true}),
            json!({"_private":{"new":2}}),
            json!({"z":null,"a":{"new":1,"_private":4},"array":{},"nested":{"stream":true}}),
            json!({"a":[{"_private":1,"z":3,"a":2}],"array":null}),
        ];
        for source in &values {
            equivalent(source, None);
            for patch in &patches {
                equivalent(source, Some(patch));
            }
        }
        let source = json!({"_private":{"inherited":1},"nested":{"_private":{"inherited":2}}});
        let patch = json!({"_private":{"new":3},"nested":{"_private":{"new":4}}});
        equivalent(&source, Some(&patch));
        let budget = PayloadBudget::with_limit(1024 * 1024);
        let output = PayloadJson::try_outbound_tail(
            &json!(null),
            Some(&json!({"stream":true})),
            budget.clone(),
            budget.limit(),
        )
        .expect("root replacement");
        assert_eq!(output.as_value()["stream"], true);
        let output = PayloadJson::try_outbound_tail(
            &json!({"stream":false}),
            Some(&json!({"stream":true})),
            budget.clone(),
            budget.limit(),
        )
        .expect("protected root");
        assert_eq!(output.as_value()["stream"], false);
    }

    #[test]
    fn guarded_outbound_tail_preserves_schema_names_through_nested_arrays() {
        for name in ["properties", "patternProperties", "definitions", "$defs"] {
            let source = json!({name:[[{
                "_named":{"_private":1,"z":2,"properties":{"_inner":{"_private":3,"type":"string"}}},
                "normal":{"_private":4,"type":"object"}
            }]],"outside":[[{"_private":5,"visible":6}]]});
            let patch = json!({name:[[{
                "_patched":{"_private":7,"type":"number"},"b":{"type":"boolean"}
            }]],"outside":{"stream":true,"_private":8}});
            equivalent(&source, None);
            equivalent(&source, Some(&patch));
            equivalent(
                &json!({name:{"_named":{"old":true}}}),
                Some(&json!({name:{"_named":{"new":true,"_private":0}}})),
            );
        }
    }

    fn capacity_fixture() -> (Value, Value) {
        (
            json!({"a":1,"b":2,"c":3,"d":4,"e":5,"nested":{"old":"synthetic"}}),
            json!({"a":6,"b":7,"c":8,"d":9,"e":0,"nested":{"new":"p".repeat(4096)}}),
        )
    }

    #[test]
    fn guarded_outbound_tail_exact_capacity_includes_temporary_key_plans() {
        let (source, patch) = capacity_fixture();
        let budget = PayloadBudget::with_limit(1024 * 1024);
        let source_owner = PayloadJson::try_from_value(&source, budget.clone(), budget.limit())
            .expect("source owner");
        let source_charge = budget.used();
        let output = PayloadJson::try_outbound_tail(
            source_owner.as_value(),
            Some(&patch),
            budget.clone(),
            budget.limit(),
        )
        .expect("tail");
        let output_charge = budget.used() - source_charge;
        let independent =
            PayloadJson::try_from_value(output.as_value(), budget.clone(), budget.limit())
                .expect("independent measure");
        assert_eq!(budget.used(), source_charge + 2 * output_charge);
        drop(independent);
        drop(output);
        assert_eq!(budget.used(), source_charge);
        drop(source_owner);
        assert_eq!(budget.used(), 0);
        // Twelve borrowed source/patch keys: growth 4 -> 8 -> 16. The old
        // eight-slot allocation overlaps the new sixteen-slot allocation.
        let key_peak = (8 + 16) * std::mem::size_of::<&str>();
        for (limit, success) in [
            (output_charge + key_peak, true),
            (output_charge + key_peak - 1, false),
        ] {
            let budget = PayloadBudget::with_limit(limit);
            let result =
                PayloadJson::try_outbound_tail(&source, Some(&patch), budget.clone(), limit);
            assert_eq!(result.is_ok(), success, "exact tail/key-plan peak");
            if let Ok(output) = result {
                assert_eq!(
                    budget.used(),
                    output_charge,
                    "temporary plans must be released"
                );
                drop(output);
            }
            assert_eq!(budget.used(), 0);
        }
    }

    #[test]
    fn guarded_outbound_tail_nested_key_plans_and_local_limit_are_exact() {
        let source = json!({"outer":{"inner":{"leaf":"synthetic"}}});
        let measure = PayloadBudget::with_limit(1024 * 1024);
        let output =
            PayloadJson::try_outbound_tail(&source, None, measure.clone(), measure.limit())
                .expect("nested measure");
        let charge = measure.used();
        drop(output);
        // Three simultaneous four-slot borrowed-key plans, one at each object.
        let peak = 3 * 4 * std::mem::size_of::<&str>();
        for (limit, local_limit, success) in [
            (charge + peak, charge, true),
            (charge + peak - 1, charge, false),
            (charge + peak, charge - 1, false),
        ] {
            let budget = PayloadBudget::with_limit(limit);
            let result = PayloadJson::try_outbound_tail(&source, None, budget.clone(), local_limit);
            assert_eq!(result.is_ok(), success);
            drop(result);
            assert_eq!(budget.used(), 0);
        }
        // No key plan is allocated for primitive roots.
        let primitive = json!("synthetic");
        let charge = NODE_BYTES + "synthetic".len();
        let budget = PayloadBudget::with_limit(charge);
        let output = PayloadJson::try_outbound_tail(&primitive, None, budget.clone(), charge)
            .expect("exact primitive");
        assert_eq!(budget.used(), charge);
        drop(output);
        assert_eq!(budget.used(), 0);
    }

    #[test]
    fn guarded_outbound_tail_refuses_before_output_construction_and_rolls_back() {
        use test_support::{Action, Phase};
        let (source, patch) = capacity_fixture();
        let source_before = source.to_string();
        let patch_before = patch.to_string();
        let measure = PayloadBudget::with_limit(1024 * 1024);
        let output =
            PayloadJson::try_outbound_tail(&source, Some(&patch), measure.clone(), measure.limit())
                .expect("measure");
        let charge = measure.used();
        drop(output);
        let allocations = Rc::new(Cell::new(0));
        for local_refusal in [false, true] {
            let budget = PayloadBudget::with_limit(if local_refusal {
                charge * 2
            } else {
                charge - 1
            });
            let observed = allocations.clone();
            let hook = test_support::install(move |event| {
                if event.phase == Phase::BeforeAllocation {
                    observed.set(observed.get() + 1);
                }
                Action::Continue
            });
            let local_limit = if local_refusal {
                charge - 1
            } else {
                charge * 2
            };
            let result =
                PayloadJson::try_outbound_tail(&source, Some(&patch), budget.clone(), local_limit);
            assert!(matches!(result, Err(TokenError::CapacityExceeded)));
            assert_eq!(allocations.get(), 0);
            assert_eq!(budget.used(), 0);
            drop(hook);
        }
        // Fail every individual output allocation, including after partial trees.
        let calls = Rc::new(Cell::new(0));
        let observed = calls.clone();
        let hook = test_support::install(move |event| {
            if event.phase == Phase::BeforeAllocation {
                observed.set(observed.get() + 1);
            }
            Action::Continue
        });
        let output =
            PayloadJson::try_outbound_tail(&source, Some(&patch), measure.clone(), measure.limit())
                .expect("count");
        drop(output);
        drop(hook);
        assert!(calls.get() > 5);
        for fail_at in 0..=calls.get() {
            let seen = Rc::new(Cell::new(0));
            let observed = seen.clone();
            let hook = test_support::install(move |event| {
                if event.phase == Phase::BeforeBuild && fail_at == 0 {
                    return Action::Fail;
                }
                if event.phase == Phase::BeforeAllocation {
                    observed.set(observed.get() + 1);
                    assert!(event.used >= event.charged);
                    if observed.get() == fail_at {
                        return Action::Fail;
                    }
                }
                Action::Continue
            });
            assert!(matches!(
                PayloadJson::try_outbound_tail(
                    &source,
                    Some(&patch),
                    measure.clone(),
                    measure.limit()
                ),
                Err(TokenError::Unavailable)
            ));
            assert_eq!(measure.used(), 0);
            assert_eq!(source.to_string(), source_before);
            assert_eq!(patch.to_string(), patch_before);
            drop(hook);
        }
    }
}

// Conservative charge for pinned serde_json parsing, not a JSON grammar parser.
// Keys count as lexical nodes too. 512/node covers Vec's minimum four Value
// slots and geometric growth; 1024/entry covers indexmap bucket/hash growth.
// The scratch charge covers one live parser, escaped-string scratch, duplicate
// key strings/HashSet tables, and realloc old/new peaks. No parser is concurrent.
fn decoded_charges(encoded: &str, local_limit: usize) -> Result<(usize, usize), TokenError> {
    if encoded.len() > 1024 * 1024 {
        return Err(TokenError::CapacityExceeded);
    }
    let (mut nodes, mut entries, mut depth) = (0usize, 0usize, 0usize);
    let (mut quoted, mut escaped, mut primitive) = (false, false, false);
    for byte in encoded.bytes() {
        if quoted {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                quoted = false;
            }
            continue;
        }
        match byte {
            b'"' => {
                nodes += 1;
                quoted = true;
                primitive = false;
            }
            b'[' | b'{' => {
                nodes += 1;
                depth += 1;
                if depth > MAX_DEPTH {
                    return Err(TokenError::CapacityExceeded);
                }
                primitive = false;
            }
            b']' | b'}' => {
                depth = depth.saturating_sub(1);
                primitive = false;
            }
            b':' => {
                entries += 1;
                primitive = false;
            }
            b',' | b' ' | b'\t' | b'\r' | b'\n' => {
                primitive = false;
            }
            _ if !primitive => {
                nodes += 1;
                primitive = true;
            }
            _ => {}
        }
        if nodes > MAX_NODES || entries > MAX_NODES {
            return Err(TokenError::CapacityExceeded);
        }
    }
    let retained = nodes
        .checked_mul(512)
        .and_then(|n| {
            entries
                .checked_mul(1024)
                .and_then(|entries| n.checked_add(entries))
        })
        .and_then(|n| n.checked_add(encoded.len()))
        .ok_or(TokenError::CapacityExceeded)?;
    let scratch = encoded
        .len()
        .checked_mul(4)
        .and_then(|n| {
            entries
                .checked_mul(512)
                .and_then(|entries| n.checked_add(entries))
        })
        .and_then(|n| n.checked_add(1024))
        .ok_or(TokenError::CapacityExceeded)?;
    retained
        .checked_add(scratch)
        .filter(|peak| *peak <= local_limit && *peak <= isize::MAX as usize)
        .ok_or(TokenError::CapacityExceeded)?;
    Ok((retained, scratch))
}

// Decode only ordinary JSON: Value::deserialize recognizes private serde sentinel
// map keys under raw_value and can invisibly reparse a string outside preflight.
// Recurse through this visitor instead, keeping all keys and string values literal.
struct PlainJson(Value);

impl<'de> Deserialize<'de> for PlainJson {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct PlainVisitor;
        impl<'de> Visitor<'de> for PlainVisitor {
            type Value = PlainJson;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("ordinary JSON")
            }

            fn visit_bool<E: de::Error>(self, value: bool) -> Result<PlainJson, E> {
                Ok(PlainJson(Value::Bool(value)))
            }
            fn visit_i64<E: de::Error>(self, value: i64) -> Result<PlainJson, E> {
                Ok(PlainJson(Value::Number(value.into())))
            }
            fn visit_u64<E: de::Error>(self, value: u64) -> Result<PlainJson, E> {
                Ok(PlainJson(Value::Number(value.into())))
            }
            fn visit_f64<E: de::Error>(self, value: f64) -> Result<PlainJson, E> {
                serde_json::Number::from_f64(value)
                    .map(|number| PlainJson(Value::Number(number)))
                    .ok_or_else(|| de::Error::custom("invalid JSON number"))
            }
            fn visit_str<E: de::Error>(self, value: &str) -> Result<PlainJson, E> {
                Ok(PlainJson(Value::String(value.to_owned())))
            }
            fn visit_string<E: de::Error>(self, value: String) -> Result<PlainJson, E> {
                Ok(PlainJson(Value::String(value)))
            }
            fn visit_unit<E: de::Error>(self) -> Result<PlainJson, E> {
                Ok(PlainJson(Value::Null))
            }

            fn visit_seq<A: SeqAccess<'de>>(self, mut sequence: A) -> Result<PlainJson, A::Error> {
                let mut values = Vec::new();
                while let Some(value) = sequence.next_element::<PlainJson>()? {
                    values.push(value.0);
                }
                Ok(PlainJson(Value::Array(values)))
            }

            fn visit_map<A: MapAccess<'de>>(self, mut object: A) -> Result<PlainJson, A::Error> {
                let mut fields = Map::new();
                while let Some(key) = object.next_key::<String>()? {
                    fields.insert(key, object.next_value::<PlainJson>()?.0);
                }
                Ok(PlainJson(Value::Object(fields)))
            }
        }
        deserializer.deserialize_any(PlainVisitor)
    }
}

// Validate uniqueness without materializing numeric values. Rewrites preserve
// original numeric lexemes rather than serializing decoded Value numbers.
struct UniqueJson;

impl<'de> Deserialize<'de> for UniqueJson {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct UniqueVisitor;
        impl<'de> Visitor<'de> for UniqueVisitor {
            type Value = UniqueJson;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("JSON without duplicate object keys")
            }

            fn visit_bool<E: de::Error>(self, _: bool) -> Result<UniqueJson, E> {
                Ok(UniqueJson)
            }
            fn visit_i64<E: de::Error>(self, _: i64) -> Result<UniqueJson, E> {
                Ok(UniqueJson)
            }
            fn visit_u64<E: de::Error>(self, _: u64) -> Result<UniqueJson, E> {
                Ok(UniqueJson)
            }
            fn visit_f64<E: de::Error>(self, _: f64) -> Result<UniqueJson, E> {
                Ok(UniqueJson)
            }
            fn visit_str<E: de::Error>(self, _: &str) -> Result<UniqueJson, E> {
                Ok(UniqueJson)
            }
            fn visit_unit<E: de::Error>(self) -> Result<UniqueJson, E> {
                Ok(UniqueJson)
            }

            fn visit_seq<A: SeqAccess<'de>>(self, mut sequence: A) -> Result<UniqueJson, A::Error> {
                while sequence.next_element::<UniqueJson>()?.is_some() {}
                Ok(UniqueJson)
            }

            fn visit_map<A: MapAccess<'de>>(self, mut object: A) -> Result<UniqueJson, A::Error> {
                let mut keys = std::collections::HashSet::new();
                while let Some(key) = object.next_key::<String>()? {
                    if !keys.insert(key) {
                        return Err(de::Error::custom("duplicate JSON key"));
                    }
                    object.next_value::<UniqueJson>()?;
                }
                Ok(UniqueJson)
            }
        }
        deserializer.deserialize_any(UniqueVisitor)
    }
}

#[cfg(test)]
mod prompt_cache_tests {
    use super::*;
    use serde_json::json;
    use std::{cell::Cell, rc::Rc};

    #[test]
    fn guarded_prompt_cache_projection_matches_legacy_exact_bytes_and_order() {
        for source in [
            json!({"model":"synthetic", "nested":[{"z":-0.0,"a":"quoted\ntext"}]}),
            json!({"first":1,"prompt_cache_key":"old","last":2}),
            json!({"prompt_cache_key":{"replaced":[1,2]},"last":-0.0}),
        ] {
            for key in ["", "synthetic-key", "Unicode-缓存-\"\\\n"] {
                let before = serde_json::to_vec(&source).expect("synthetic source");
                let mut legacy = source.clone();
                legacy["prompt_cache_key"] = json!(key);
                let budget = PayloadBudget::with_limit(1024 * 1024);
                let output = PayloadJson::try_with_prompt_cache_key(
                    &source,
                    key,
                    budget.clone(),
                    budget.limit(),
                )
                .expect("synthetic candidate");
                assert_eq!(
                    serde_json::to_vec(output.as_value()).expect("output"),
                    serde_json::to_vec(&legacy).expect("legacy")
                );
                assert_eq!(serde_json::to_vec(&source).expect("unchanged"), before);
                drop(output);
                assert_eq!(budget.used(), 0);
            }
        }
    }

    #[test]
    fn guarded_prompt_cache_exact_capacity_overlap_and_preconstruction_refusal() {
        let source = json!({"nested":[{"text":"p".repeat(1024)}],"prompt_cache_key":"old"});
        let key = "synthetic-key".repeat(64);
        let measure = PayloadBudget::with_limit(1024 * 1024);
        let output =
            PayloadJson::try_with_prompt_cache_key(&source, &key, measure.clone(), measure.limit())
                .expect("measure");
        let charge = measure.used();
        drop(output);
        let mut expected = source.clone();
        expected["prompt_cache_key"] = json!(key);
        let copy = PayloadJson::try_from_value(&expected, measure.clone(), measure.limit())
            .expect("independent measure");
        assert_eq!(
            measure.used(),
            charge,
            "same full-tree accounting, not a string-only charge"
        );
        drop(copy);
        for (limit, local_limit, success) in [
            (charge, charge, true),
            (charge - 1, charge, false),
            (charge, charge - 1, false),
        ] {
            let budget = PayloadBudget::with_limit(limit);
            let builds = Rc::new(Cell::new(0));
            let seen = builds.clone();
            let hook = test_support::install(move |event| {
                if event.phase == test_support::Phase::BeforeBuild {
                    seen.set(seen.get() + 1);
                }
                test_support::Action::Continue
            });
            let result =
                PayloadJson::try_with_prompt_cache_key(&source, &key, budget.clone(), local_limit);
            assert_eq!(result.is_ok(), success);
            assert_eq!(
                builds.get(),
                usize::from(success),
                "refuse before candidate construction"
            );
            drop(result);
            drop(hook);
            assert_eq!(budget.used(), 0);
        }
        let source_owner = PayloadJson::try_from_value(&source, measure.clone(), measure.limit())
            .expect("source owner");
        let source_charge = measure.used();
        let output = PayloadJson::try_with_prompt_cache_key(
            source_owner.as_value(),
            &key,
            measure.clone(),
            measure.limit(),
        )
        .expect("overlap");
        assert_eq!(measure.used(), source_charge + charge);
        drop(output);
        assert_eq!(measure.used(), source_charge);
        drop(source_owner);
        assert_eq!(measure.used(), 0);
        for invalid in [json!(null), json!("not an object"), json!([])] {
            assert!(matches!(
                PayloadJson::try_with_prompt_cache_key(
                    &invalid,
                    &key,
                    measure.clone(),
                    measure.limit()
                ),
                Err(TokenError::RedactionFailed)
            ));
            assert_eq!(measure.used(), 0);
        }
    }

    #[test]
    fn guarded_prompt_cache_every_candidate_allocation_failure_rolls_back() {
        use test_support::{Action, Phase};
        let source = json!({"a":["text",{"b":"nested"}],"prompt_cache_key":"old", "last":true});
        let before = source.to_string();
        let budget = PayloadBudget::with_limit(1024 * 1024);
        let calls = Rc::new(Cell::new(0));
        let seen = calls.clone();
        let hook = test_support::install(move |event| {
            if event.phase == Phase::BeforeAllocation {
                seen.set(seen.get() + 1);
            }
            Action::Continue
        });
        let output = PayloadJson::try_with_prompt_cache_key(
            &source,
            "new-key",
            budget.clone(),
            budget.limit(),
        )
        .expect("count");
        let charge = budget.used();
        drop(output);
        drop(hook);
        assert!(calls.get() > 5);
        for fail_at in 0..=calls.get() + 1 {
            let calls_count = calls.get();
            let seen = Rc::new(Cell::new(0));
            let observed = seen.clone();
            let hook = test_support::install(move |event| {
                assert_eq!(event.charged, charge);
                assert_eq!(event.used, charge);
                if event.phase == Phase::BeforeBuild && fail_at == 0 {
                    return Action::Fail;
                }
                if event.phase == Phase::AfterBuild && fail_at == calls_count + 1 {
                    return Action::Fail;
                }
                if event.phase == Phase::BeforeAllocation {
                    observed.set(observed.get() + 1);
                    if observed.get() == fail_at {
                        return Action::Fail;
                    }
                }
                Action::Continue
            });
            assert!(matches!(
                PayloadJson::try_with_prompt_cache_key(
                    &source,
                    "new-key",
                    budget.clone(),
                    budget.limit()
                ),
                Err(TokenError::Unavailable)
            ));
            drop(hook);
            assert_eq!(budget.used(), 0);
            assert_eq!(source.to_string(), before);
        }
    }
}

#[cfg(test)]
mod namespace_tests {
    use super::*;
    use serde_json::json;
    use std::{cell::Cell, rc::Rc};
    fn fixture() -> Value {
        json!({"model":"synthetic", "tools":[{"type":"namespace","name":" ns ","tools":[
            {"description":"first","type":"function","name":" work ","parameters":{"z":-0.0,"a":[1,2]}}
        ]}], "input":[{"namespace":"ns","type":"function_call","name":"work","last":"tail"}],
        "tool_choice":{"namespace":"ns","name":"work","last":"tail"}})
    }
    fn compare(source: &Value) {
        let before = serde_json::to_vec(source).expect("source");
        let mut legacy = source.clone();
        let changed = crate::proxy::providers::transform_codex_responses_namespace::flatten_request_namespaces(&mut legacy).expect("legacy");
        let budget = PayloadBudget::with_limit(16 * 1024 * 1024);
        let output = PayloadJson::try_namespace_body(source, budget.clone(), budget.limit())
            .expect("candidate");
        assert_eq!(output.is_some(), changed);
        if let Some(output) = output {
            assert_eq!(
                serde_json::to_vec(output.as_value()).expect("output"),
                serde_json::to_vec(&legacy).expect("legacy"),
                "source={source}"
            );
        }
        assert_eq!(serde_json::to_vec(source).expect("unchanged"), before);
        assert_eq!(budget.used(), 0);
    }
    #[test]
    fn guarded_namespace_projection_matches_legacy_exact_bytes_and_quirks() {
        compare(&fixture());
        for (namespace, name) in [
            ("n", "f"),
            ("", "f"),
            ("  ", "f"),
            ("中文空间", "工具名称"),
            ("x", "y"),
        ] {
            for long in [false, true] {
                let namespace = if long {
                    namespace.repeat(40)
                } else {
                    namespace.into()
                };
                let name = if long { name.repeat(40) } else { name.into() };
                let child = json!({"name":name,"type":"function","schema":{"signed":-0.0}});
                let mut source = json!({"tools":[{"name":namespace,"type":"namespace","children":[child.clone(),child]}],
                    "input":{"wrapper":[{"namespace":namespace,"name":name,"type":"function_call","nested":{"namespace":namespace,"name":name,"type":"function_call"}}]},
                    "tool_choice":{"namespace":namespace,"name":name,"type":"function"}});
                compare(&source);
                source["tool_choice"] = json!({"type":"namespace","secret":"synthetic"});
                compare(&source);
                source["tool_choice"] = json!([{"namespace":namespace,"name":name}]);
                compare(&source);
                source["tools"][0]["tools"] = json!(false);
                compare(&source);
            }
        }
        for tools in [
            json!([]),
            json!([{"type":"function","name":"plain"}]),
            json!(null),
        ] {
            compare(&json!({"tools":tools}));
        }
        compare(
            &json!({"tools":[{"type":"namespace","name":"n","tools":[{}, {"type":"function","name":""}, {"type":"custom","name":"f"}]}]}),
        );
        compare(
            &json!({"tools":[{"type":"function","name":"plain"},{"type":"namespace","name":"n","tools":[{"type":"function","name":"f","first":1},{"type":"function","name":"f","second":2}]},{"type":"custom","name":"last"}]}),
        );
        compare(
            &json!({"tools":[{"type":"function","name":"__f"},{"type":"namespace","name":"","tools":[{"type":"function","name":"f"}]}]}),
        );
    }
    #[test]
    fn guarded_namespace_stack_names_match_legacy_bytes() {
        for namespace in ["", "x", "中文空间", "𐀀", &"n".repeat(80)] {
            for length in 0..100 {
                for unit in ["y", "界", "𐀀"] {
                    let name = unit.repeat(length);
                    let stack = StackToolName::new(namespace, &name).expect("name");
                    assert_eq!(
                        stack.as_str(),
                        crate::proxy::providers::transform_codex_chat::flatten_namespace_tool_name(
                            namespace, &name
                        )
                    );
                    assert!(stack.as_str().len() <= 64);
                }
            }
        }
    }
    #[test]
    fn guarded_namespace_collisions_are_transactional_and_fixed() {
        for tools in [
            json!([{"type":"function","name":" n__f "},{"type":"namespace","name":"n","tools":[{"type":"function","name":"f"}]}]),
            json!([{"type":"namespace","name":"a__b","tools":[{"type":"function","name":"c"}]},{"type":"namespace","name":"a","tools":[{"type":"function","name":"b__c"}]}]),
        ] {
            let source = json!({"tools":tools});
            let before = source.to_string();
            let mut legacy = source.clone();
            assert!(crate::proxy::providers::transform_codex_responses_namespace::flatten_request_namespaces(&mut legacy).is_err());
            let budget = PayloadBudget::with_limit(1024 * 1024);
            assert!(matches!(
                PayloadJson::try_namespace_body(&source, budget.clone(), budget.limit()),
                Err(TokenError::RedactionFailed)
            ));
            assert_eq!(source.to_string(), before);
            assert_eq!(budget.used(), 0);
        }
    }
    #[test]
    fn guarded_namespace_exact_capacity_plan_overlap_and_independent_candidates() {
        let source = fixture();
        let budget = PayloadBudget::with_limit(1024 * 1024);
        let plan = NamespacePlan::new(&source, budget.clone())
            .expect("plan")
            .expect("namespace");
        let plan_charge = budget.used();
        assert!(plan_charge > 0);
        let output = PayloadJson::try_namespace_body(&source, budget.clone(), budget.limit())
            .expect("measure")
            .expect("candidate");
        let charge = budget.used() - plan_charge;
        drop(output);
        drop(plan);
        assert_eq!(budget.used(), 0);
        for (limit, local, success) in [
            (plan_charge + charge, charge, true),
            (plan_charge + charge - 1, charge, false),
            (plan_charge + charge, charge - 1, false),
        ] {
            let budget = PayloadBudget::with_limit(limit);
            let builds = Rc::new(Cell::new(0));
            let seen = builds.clone();
            let hook = test_support::install(move |event| {
                if event.phase == test_support::Phase::BeforeBuild {
                    seen.set(seen.get() + 1);
                }
                test_support::Action::Continue
            });
            let result = PayloadJson::try_namespace_body(&source, budget.clone(), local);
            assert_eq!(result.is_ok(), success);
            assert_eq!(builds.get(), usize::from(success));
            drop(result);
            drop(hook);
            assert_eq!(budget.used(), 0);
        }
        let original = PayloadJson::try_from_value(&source, budget.clone(), budget.limit())
            .expect("owned source");
        let original_charge = budget.used();
        let first =
            PayloadJson::try_namespace_body(original.as_value(), budget.clone(), budget.limit())
                .expect("first")
                .expect("candidate");
        let second =
            PayloadJson::try_namespace_body(original.as_value(), budget.clone(), budget.limit())
                .expect("second")
                .expect("candidate");
        assert_eq!(budget.used(), original_charge + 2 * charge);
        assert_ne!(
            first.as_value()["model"].as_str().expect("model").as_ptr(),
            second.as_value()["model"].as_str().expect("model").as_ptr()
        );
        drop(first);
        assert_eq!(budget.used(), original_charge + charge);
        drop(second);
        drop(original);
        assert_eq!(budget.used(), 0);
    }
    #[test]
    fn guarded_namespace_all_candidate_allocations_roll_back() {
        use test_support::{Action, Phase};
        let source = fixture();
        let before = source.to_string();
        let budget = PayloadBudget::with_limit(1024 * 1024);
        let calls = Rc::new(Cell::new(0));
        let seen = calls.clone();
        let hook = test_support::install(move |event| {
            if event.phase == Phase::BeforeAllocation {
                seen.set(seen.get() + 1);
            }
            Action::Continue
        });
        drop(
            PayloadJson::try_namespace_body(&source, budget.clone(), budget.limit())
                .expect("count"),
        );
        drop(hook);
        assert!(calls.get() > 10);
        for fail_at in 0..=calls.get() + 1 {
            let count = calls.get();
            let seen = Rc::new(Cell::new(0));
            let observed = seen.clone();
            let hook = test_support::install(move |event| {
                assert!(
                    event.used > event.charged,
                    "plan remains charged while candidate builds"
                );
                if (event.phase == Phase::BeforeBuild && fail_at == 0)
                    || (event.phase == Phase::AfterBuild && fail_at == count + 1)
                {
                    return Action::Fail;
                }
                if event.phase == Phase::BeforeAllocation {
                    observed.set(observed.get() + 1);
                    if observed.get() == fail_at {
                        return Action::Fail;
                    }
                }
                Action::Continue
            });
            assert!(matches!(
                PayloadJson::try_namespace_body(&source, budget.clone(), budget.limit()),
                Err(TokenError::Unavailable)
            ));
            drop(hook);
            assert_eq!(budget.used(), 0);
            assert_eq!(source.to_string(), before);
        }
    }
    #[test]
    fn guarded_namespace_no_namespace_has_no_plan_or_candidate() {
        let budget = PayloadBudget::with_limit(0);
        for source in [
            json!({}),
            json!({"tools":[{"type":"function","name":"plain"}]}),
            json!(null),
        ] {
            assert!(PayloadJson::try_namespace_body(&source, budget.clone(), 0)
                .expect("no namespace")
                .is_none());
            assert_eq!(budget.used(), 0);
        }
    }
}

#[cfg(test)]
mod namespace_plan_rollback_tests {
    use super::*;
    use serde_json::json;
    use std::{cell::Cell, rc::Rc};
    #[test]
    fn guarded_namespace_plan_each_backing_allocation_refuses_without_candidate() {
        use super::super::payload_metadata::test_support::{self as metadata, Action, Phase};
        let source = json!({"tools":[{"type":"function","name":"plain"},{"type":"namespace","name":"ns","tools":(0..19).map(|index|json!({"type":"function","name":format!("f{index}")})).collect::<Vec<_>>()}]});
        let budget = PayloadBudget::with_limit(1024 * 1024);
        let calls = Rc::new(Cell::new(0));
        let seen = calls.clone();
        let hook = metadata::install(move |event| {
            if event.phase == Phase::BeforeAllocation {
                seen.set(seen.get() + 1);
            }
            Action::Continue
        });
        drop(
            PayloadJson::try_namespace_body(&source, budget.clone(), budget.limit())
                .expect("count"),
        );
        drop(hook);
        assert!(calls.get() >= 10);
        for fail_at in 1..=calls.get() {
            let seen = Rc::new(Cell::new(0));
            let observed = seen.clone();
            let hook = metadata::install(move |event| {
                if event.phase == Phase::BeforeAllocation {
                    observed.set(observed.get() + 1);
                    if observed.get() == fail_at {
                        return Action::Fail;
                    }
                }
                Action::Continue
            });
            let builds = Rc::new(Cell::new(0));
            let observed = builds.clone();
            let build_hook = test_support::install(move |event| {
                if event.phase == test_support::Phase::BeforeBuild {
                    observed.set(observed.get() + 1);
                }
                test_support::Action::Continue
            });
            assert!(matches!(
                PayloadJson::try_namespace_body(&source, budget.clone(), budget.limit()),
                Err(TokenError::Unavailable)
            ));
            drop(build_hook);
            drop(hook);
            assert_eq!(builds.get(), 0);
            assert_eq!(budget.used(), 0);
        }
    }
    #[test]
    fn guarded_namespace_history_remove_order_and_stop_semantics_match_legacy() {
        for position in 0..5 {
            for name_position in 0..5 {
                let mut item = serde_json::Map::new();
                for index in 0..5 {
                    if index == position {
                        item.insert("namespace".into(), json!(" ns "));
                    }
                    if index == name_position {
                        item.insert("name".into(), json!(" f "));
                    }
                    item.insert(format!("k{index}"), json!(-0.0));
                }
                item.insert("type".into(), json!("function_call"));
                item.insert(
                    "nested".into(),
                    json!({"type":"function_call","namespace":"ns","name":"f"}),
                );
                let body = json!({"tools":[{"type":"namespace","name":"ns","tools":[{"type":"function","name":"f"}]}],"input":[{"nested":[item.clone()]}],"tool_choice":item});
                let mut legacy = body.clone();
                crate::proxy::providers::transform_codex_responses_namespace::flatten_request_namespaces(&mut legacy).expect("legacy");
                let budget = PayloadBudget::with_limit(1024 * 1024);
                let output = PayloadJson::try_namespace_body(&body, budget.clone(), budget.limit())
                    .expect("candidate")
                    .expect("namespace");
                assert_eq!(
                    serde_json::to_vec(output.as_value()).expect("output"),
                    serde_json::to_vec(&legacy).expect("legacy")
                );
                drop(output);
                assert_eq!(budget.used(), 0);
            }
        }
    }
}
