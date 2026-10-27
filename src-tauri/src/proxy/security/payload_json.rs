//! Private immutable JSON copies with conservative, pre-build payload accounting.
//!
//! Rebuild by logical length: `Value::clone` can inherit spare IndexMap capacity.
//! Node/entry charges cover the pinned serde_json/indexmap representation, not
//! allocator bookkeeping, process RSS, physical OOM recovery or zeroization.
use super::{
    payload_budget::{PayloadBudget, Reservation},
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
