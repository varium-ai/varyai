//! Private logical carriers for Codex `instructions`, `input` and `prompt` only.
//! This does not cover other protocol roots inspected by the legacy extractor.
//! Original/decoded JSON, paths and leaf/document metadata own shared reservations.
//! Rewrites return immutable guarded candidates and borrow frozen replacement text.
use super::{
    payload_budget::PayloadBudget,
    payload_buffer::PayloadBuffer,
    payload_json::{JsonStringReplacement, PayloadJson},
    payload_metadata::{PayloadKey, PayloadVec},
    tokenization::TokenError,
};
use serde_json::Value;
use std::io::{self, Write};
use std::ops::Range;

const MAX_DEPTH: usize = 64;
const MAX_NODES: usize = 65_536;
const MAX_LEAVES: usize = 4096;
const MAX_STRING_BYTES: usize = 1024 * 1024;
const MAX_TEXT_BYTES: usize = 4 * 1024 * 1024;
const MAX_ARGUMENT_BYTES: usize = 4 * 1024 * 1024;
const MAX_COPY_BYTES: usize = 8 * 1024 * 1024;
const NODE_CHARGE: usize = 64;

/// An ordinal is stable only within this immutable plan. No keys/paths are exposed.
/// Deliberately no Debug/Serialize: this view contains request plaintext.
pub(super) struct CarrierLeaf<'a> {
    pub id: usize,
    pub text: &'a str,
    pub writable: bool,
    pub is_password_field: bool,
}

enum Step {
    Field(PayloadKey),
    Index(usize),
}

type Path = PayloadVec<Step>;

struct Leaf {
    path: Path,
    document: Option<usize>,
    encoded_range: Option<Range<usize>>,
    writable: bool,
    password: bool,
}

struct Document {
    outer_path: Path,
    value: PayloadJson,
}

/// Deliberately no Debug/Serialize: a plan owns private request and decoded text.
/// Only the three Codex roots above are inspected. Unrecognized strings in those
/// roots remain visible but read-only; opaque/media protocol carriers are excluded.
pub(super) struct CodexRequestCarriers {
    original: PayloadJson,
    leaves: PayloadVec<Leaf>,
    documents: PayloadVec<Document>,
    shared_budget: PayloadBudget,
    text_bytes: usize,
    argument_bytes: usize,
    budget: Budget,
    purpose: CollectionPurpose,
}

#[derive(Default, Clone, Copy)]
struct Budget {
    nodes: usize,
    bytes: usize,
}

fn add_limited(total: &mut usize, amount: usize, limit: usize) -> Result<(), TokenError> {
    let next = total
        .checked_add(amount)
        .ok_or(TokenError::CapacityExceeded)?;
    if next > limit {
        return Err(TokenError::CapacityExceeded);
    }
    *total = next;
    Ok(())
}

fn replace_charge(
    total: &mut usize,
    old: usize,
    new: usize,
    limit: usize,
) -> Result<(), TokenError> {
    *total = total.checked_sub(old).ok_or(TokenError::RedactionFailed)?;
    add_limited(total, new, limit)
}

impl Budget {
    fn node(&mut self) -> Result<(), TokenError> {
        add_limited(&mut self.nodes, 1, MAX_NODES)?;
        self.bytes(NODE_CHARGE)
    }

    fn bytes(&mut self, bytes: usize) -> Result<(), TokenError> {
        add_limited(&mut self.bytes, bytes, MAX_COPY_BYTES)
    }

    // Count keys for allocation accounting only, never as inspectable strings.
    fn value(&mut self, value: &Value, depth: usize) -> Result<(), TokenError> {
        if depth > MAX_DEPTH {
            return Err(TokenError::CapacityExceeded);
        }
        self.node()?;
        match value {
            Value::String(text) => self.bytes(text.len())?,
            Value::Array(values) => {
                for value in values {
                    self.value(value, depth + 1)?;
                }
            }
            Value::Object(fields) => {
                for (key, value) in fields {
                    self.bytes(NODE_CHARGE)?;
                    self.bytes(key.len())?;
                    self.value(value, depth + 1)?;
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn path(&mut self, path: &Path, shared: &PayloadBudget) -> Result<Path, TokenError> {
        for step in path.as_slice() {
            self.bytes(NODE_CHARGE)?;
            if let Step::Field(key) = step {
                self.bytes(key.as_str().len())?;
            }
        }
        let mut copy = Path::new(shared.clone())?;
        for step in path.as_slice() {
            copy.push(match step {
                Step::Field(key) => Step::Field(PayloadKey::try_from_str(key.as_str(), shared)?),
                Step::Index(index) => Step::Index(*index),
            })?;
        }
        Ok(copy)
    }
}

#[derive(Clone, Copy)]
enum Mode {
    InputRoot,
    TextRoot,
    ScalarText,
    Item,
    ReadOnly,
    Readable,
    TextBlock,
    SummaryList,
    Summary,
    ReasoningList,
    Reasoning,
    Arguments,
}

#[derive(Clone, Copy)]
enum CollectionPurpose {
    Forwarding,
    RefusalAudit,
}

impl CodexRequestCarriers {
    pub(super) fn from_body(body: &Value) -> Result<Self, TokenError> {
        Self::from_body_with_budget(body, PayloadBudget::default())
    }

    pub(super) fn from_body_with_budget(
        body: &Value,
        shared_budget: PayloadBudget,
    ) -> Result<Self, TokenError> {
        Self::collect_body(body, shared_budget, CollectionPurpose::Forwarding)
    }

    /// Best-effort observations for an already refused request, never authorization.
    /// Malformed encoded arguments are omitted; other carrier semantics and limits
    /// remain unchanged. Audit-only plans cannot produce forwarding candidates.
    pub(super) fn from_body_for_refusal_audit(
        body: &Value,
        shared_budget: PayloadBudget,
    ) -> Result<Self, TokenError> {
        Self::collect_body(body, shared_budget, CollectionPurpose::RefusalAudit)
    }

    fn collect_body(
        body: &Value,
        shared_budget: PayloadBudget,
        purpose: CollectionPurpose,
    ) -> Result<Self, TokenError> {
        if !body.is_object() {
            return Err(TokenError::RedactionFailed);
        }
        let mut budget = Budget::default();
        budget.value(body, 0)?;
        let original = PayloadJson::try_from_value(body, shared_budget.clone(), MAX_COPY_BYTES)?;
        let mut plan = Self {
            original,
            leaves: PayloadVec::new(shared_budget.clone())?,
            documents: PayloadVec::new(shared_budget.clone())?,
            shared_budget,
            text_bytes: 0,
            argument_bytes: 0,
            budget,
            purpose,
        };
        for root in ["instructions", "input", "prompt"] {
            if let Some(value) = body.get(root) {
                let mut path = Path::new(plan.shared_budget.clone())?;
                path.push(Step::Field(PayloadKey::try_from_str(
                    root,
                    &plan.shared_budget,
                )?))?;
                let mode = if root == "input" {
                    Mode::InputRoot
                } else {
                    Mode::TextRoot
                };
                plan.collect(value, &mut path, None, mode, false)?;
            }
        }
        Ok(plan)
    }

    pub(super) fn visit<'a>(
        &'a self,
        mut visitor: impl FnMut(CarrierLeaf<'a>) -> Result<(), TokenError>,
    ) -> Result<(), TokenError> {
        for (id, leaf) in self.leaves.as_slice().iter().enumerate() {
            visitor(self.view(id, leaf)?)?;
        }
        Ok(())
    }

    /// Freeze each borrowed callback result once, then build one guarded candidate.
    /// A stale source or any failure leaves both the source and plan unchanged.
    pub(super) fn rewrite_candidate<'a>(
        &self,
        body: &Value,
        mut rewriter: impl FnMut(CarrierLeaf<'_>) -> Result<Option<&'a str>, TokenError>,
    ) -> Result<PayloadJson, TokenError> {
        if matches!(self.purpose, CollectionPurpose::RefusalAudit) {
            return Err(TokenError::RedactionFailed);
        }
        Budget::default().value(body, 0)?;
        if !same_snapshot(body, self.original.as_value()) {
            return Err(TokenError::RedactionFailed);
        }
        let mut edits = PayloadVec::new(self.shared_budget.clone())?;
        let mut text_bytes = self.text_bytes;
        let mut argument_bytes = self.argument_bytes;
        let mut copy_bytes = self.budget.bytes;
        for (id, leaf) in self.leaves.as_slice().iter().enumerate() {
            let view = self.view(id, leaf)?;
            let Some(replacement) = rewriter(view)? else {
                continue;
            };
            if !leaf.writable {
                return Err(TokenError::RedactionFailed);
            }
            if replacement.len() > MAX_STRING_BYTES {
                return Err(TokenError::CapacityExceeded);
            }
            let original = self.view(id, leaf)?.text;
            replace_charge(
                &mut text_bytes,
                original.len(),
                replacement.len(),
                MAX_TEXT_BYTES,
            )?;
            replace_charge(
                &mut copy_bytes,
                original.len(),
                replacement.len(),
                MAX_COPY_BYTES,
            )?;
            if replacement != original {
                edits.push((id, replacement))?;
            }
        }
        // Every edited argument document has one complete preallocated buffer.
        // Buffers and their vector remain charged until the final tree is rebuilt.
        let mut documents = PayloadVec::new(self.shared_budget.clone())?;
        for (index, document) in self.documents.as_slice().iter().enumerate() {
            let original = at(self.original.as_value(), &document.outer_path)?
                .as_str()
                .ok_or(TokenError::RedactionFailed)?;
            let mut size = original.len();
            let mut changed = false;
            for &(id, text) in edits.as_slice() {
                let leaf = &self.leaves.as_slice()[id];
                if leaf.document != Some(index) {
                    continue;
                }
                let range = leaf
                    .encoded_range
                    .as_ref()
                    .ok_or(TokenError::RedactionFailed)?;
                let encoded = encoded_size(text)?;
                size = size
                    .checked_sub(range.len())
                    .and_then(|n| n.checked_add(encoded))
                    .filter(|n| *n <= MAX_STRING_BYTES)
                    .ok_or(TokenError::CapacityExceeded)?;
                replace_charge(
                    &mut argument_bytes,
                    range.len(),
                    encoded,
                    MAX_ARGUMENT_BYTES,
                )?;
                replace_charge(&mut copy_bytes, range.len(), encoded, MAX_COPY_BYTES)?;
                changed = true;
            }
            if !changed {
                continue;
            }
            let mut writer = ArgumentWriter {
                buffer: PayloadBuffer::with_capacity(
                    size,
                    self.shared_budget.clone(),
                    MAX_STRING_BYTES,
                )?,
                error: None,
            };
            let mut cursor = 0;
            // Collection visits each decoded document in lexical source order.
            // Ranges must still prove monotonically nonoverlapping at this boundary.
            for &(id, text) in edits.as_slice() {
                let leaf = &self.leaves.as_slice()[id];
                if leaf.document != Some(index) {
                    continue;
                }
                let range = leaf
                    .encoded_range
                    .as_ref()
                    .ok_or(TokenError::RedactionFailed)?;
                if range.start < cursor {
                    return Err(TokenError::RedactionFailed);
                }
                writer.append(
                    original
                        .get(cursor..range.start)
                        .ok_or(TokenError::RedactionFailed)?
                        .as_bytes(),
                )?;
                if serde_json::to_writer(&mut writer, text).is_err() {
                    return Err(writer.error.unwrap_or(TokenError::RedactionFailed));
                }
                cursor = range.end;
            }
            writer.append(
                original
                    .get(cursor..)
                    .ok_or(TokenError::RedactionFailed)?
                    .as_bytes(),
            )?;
            if writer.buffer.len() != size {
                return Err(TokenError::RedactionFailed);
            }
            documents.push((index, writer.buffer))?;
        }
        let mut replacements = PayloadVec::new(self.shared_budget.clone())?;
        for &(id, text) in edits.as_slice() {
            let leaf = &self.leaves.as_slice()[id];
            if leaf.document.is_none() {
                replacements.push(JsonStringReplacement {
                    target: at(self.original.as_value(), &leaf.path)?,
                    text,
                })?;
            }
        }
        for (index, buffer) in documents.as_slice() {
            replacements.push(JsonStringReplacement {
                target: at(
                    self.original.as_value(),
                    &self.documents.as_slice()[*index].outer_path,
                )?,
                text: std::str::from_utf8(buffer.as_slice())
                    .map_err(|_| TokenError::RedactionFailed)?,
            })?;
        }
        let candidate = PayloadJson::try_with_replacements(
            self.original.as_value(),
            replacements.as_slice(),
            self.shared_budget.clone(),
            self.shared_budget.limit(),
        )?;
        Budget::default().value(candidate.as_value(), 0)?;
        Ok(candidate)
    }

    fn view<'a>(&'a self, id: usize, leaf: &Leaf) -> Result<CarrierLeaf<'a>, TokenError> {
        let root = match leaf.document {
            Some(index) => self.documents.as_slice()[index].value.as_value(),
            None => self.original.as_value(),
        };
        Ok(CarrierLeaf {
            id,
            text: at(root, &leaf.path)?
                .as_str()
                .ok_or(TokenError::RedactionFailed)?,
            writable: leaf.writable,
            is_password_field: leaf.password,
        })
    }

    fn collect(
        &mut self,
        value: &Value,
        path: &mut Path,
        document: Option<usize>,
        mode: Mode,
        password: bool,
    ) -> Result<(), TokenError> {
        match value {
            Value::String(text) => {
                if text.len() > MAX_STRING_BYTES || self.leaves.len() >= MAX_LEAVES {
                    return Err(TokenError::CapacityExceeded);
                }
                add_limited(&mut self.text_bytes, text.len(), MAX_TEXT_BYTES)?;
                self.leaves.push(Leaf {
                    path: self.budget.path(path, &self.shared_budget)?,
                    document,
                    encoded_range: None,
                    writable: matches!(
                        mode,
                        Mode::InputRoot
                            | Mode::TextRoot
                            | Mode::ScalarText
                            | Mode::Readable
                            | Mode::Arguments
                    ),
                    password,
                })?;
            }
            Value::Array(values) => {
                let child_mode = match mode {
                    Mode::InputRoot => Mode::Item,
                    Mode::TextRoot => Mode::ScalarText,
                    Mode::SummaryList => Mode::Summary,
                    Mode::ReasoningList => Mode::Reasoning,
                    Mode::Readable => Mode::TextBlock,
                    Mode::Arguments => Mode::Arguments,
                    _ => Mode::ReadOnly,
                };
                for (index, child) in values.iter().enumerate() {
                    path.push(Step::Index(index))?;
                    self.collect(child, path, document, child_mode, password)?;
                    path.pop();
                }
            }
            Value::Object(fields) => {
                let mode = if matches!(
                    mode,
                    Mode::InputRoot | Mode::TextRoot | Mode::ScalarText | Mode::Readable
                ) {
                    Mode::ReadOnly
                } else {
                    mode
                };
                let kind = value.get("type").and_then(Value::as_str);
                if !matches!(mode, Mode::Arguments) && media_type(kind) {
                    return Ok(());
                }
                let message = matches!(mode, Mode::Item)
                    && (value.get("type").is_none() || kind == Some("message"))
                    && matches!(
                        value.get("role").and_then(Value::as_str),
                        Some("user" | "assistant" | "system" | "developer")
                    );
                for (key, child) in fields {
                    if !matches!(mode, Mode::Arguments) && excluded_field(key, kind) {
                        continue;
                    }
                    path.push(Step::Field(PayloadKey::try_from_str(
                        key,
                        &self.shared_budget,
                    )?))?;
                    if matches!(mode, Mode::Item)
                        && kind == Some("function_call")
                        && key == "arguments"
                    {
                        self.arguments(child, path, password)?;
                    } else {
                        let child_mode = match mode {
                            Mode::Arguments => Mode::Arguments,
                            Mode::Item if message && key == "content" => Mode::Readable,
                            Mode::Item
                                if matches!(
                                    kind,
                                    Some("function_call_output" | "custom_tool_call_output")
                                ) && key == "output" =>
                            {
                                Mode::Readable
                            }
                            Mode::Item
                                if kind == Some("custom_tool_call")
                                    && key == "input"
                                    && child.is_string() =>
                            {
                                Mode::Readable
                            }
                            Mode::Item
                                if kind == Some("reasoning")
                                    && key == "summary"
                                    && child.is_array() =>
                            {
                                Mode::SummaryList
                            }
                            Mode::Item
                                if kind == Some("reasoning")
                                    && key == "content"
                                    && child.is_array() =>
                            {
                                Mode::ReasoningList
                            }
                            Mode::TextBlock | Mode::Readable | Mode::Item
                                if key == "text"
                                    && matches!(
                                        kind,
                                        Some("text" | "input_text" | "output_text")
                                    )
                                    && child.is_string() =>
                            {
                                Mode::Readable
                            }
                            Mode::TextBlock | Mode::Readable
                                if key == "refusal"
                                    && kind == Some("refusal")
                                    && child.is_string() =>
                            {
                                Mode::Readable
                            }
                            Mode::Summary
                                if key == "text"
                                    && matches!(kind, Some("summary_text" | "reasoning_text"))
                                    && child.is_string() =>
                            {
                                Mode::Readable
                            }
                            Mode::Reasoning
                                if key == "text"
                                    && kind == Some("reasoning_text")
                                    && child.is_string() =>
                            {
                                Mode::Readable
                            }
                            _ => Mode::ReadOnly,
                        };
                        self.collect(
                            child,
                            path,
                            document,
                            child_mode,
                            password || (child.is_string() && password_key(key)),
                        )?;
                    }
                    path.pop();
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn arguments(
        &mut self,
        value: &Value,
        path: &mut Path,
        password: bool,
    ) -> Result<(), TokenError> {
        let Some(encoded) = value.as_str() else {
            return self.collect(value, path, None, Mode::Arguments, password);
        };
        if encoded.len() > MAX_STRING_BYTES {
            return Err(TokenError::CapacityExceeded);
        }
        add_limited(&mut self.argument_bytes, encoded.len(), MAX_ARGUMENT_BYTES)?;
        // Bound bytes, nesting and allocations before invoking the JSON parser.
        preflight_json(encoded, &mut self.budget)?;
        let decoded = match PayloadJson::try_from_str(
            encoded,
            self.shared_budget.clone(),
            self.shared_budget.limit(),
        ) {
            Ok(decoded) => decoded,
            Err(TokenError::RedactionFailed)
                if matches!(self.purpose, CollectionPurpose::RefusalAudit) =>
            {
                // No document or leaf has been added yet. Keep the original and
                // preflight charges, and let the caller pop the unchanged path.
                return Ok(());
            }
            Err(error) => return Err(error),
        };
        let mut validation = Budget::default();
        validation.value(decoded.as_value(), 0)?;
        let index = self.documents.len();
        let outer_path = self.budget.path(path, &self.shared_budget)?;
        let mut cursor = 0;
        self.argument_ranges(
            encoded,
            decoded.as_value(),
            &mut cursor,
            &mut Path::new(self.shared_budget.clone())?,
            index,
            password,
        )?;
        skip_space(encoded, &mut cursor);
        if cursor != encoded.len() {
            return Err(TokenError::RedactionFailed);
        }
        self.documents.push(Document {
            outer_path,
            value: decoded,
        })?;
        Ok(())
    }

    // Syntax and duplicate keys were already checked. Walk original lexical order
    // solely to bind decoded string values (never keys) to exact encoded ranges.
    fn argument_ranges(
        &mut self,
        encoded: &str,
        value: &Value,
        cursor: &mut usize,
        path: &mut Path,
        document: usize,
        password: bool,
    ) -> Result<(), TokenError> {
        skip_space(encoded, cursor);
        match value {
            Value::String(_) => {
                let range = string_range(encoded, cursor)?;
                self.collect(value, path, Some(document), Mode::Arguments, password)?;
                self.leaves
                    .last_mut()
                    .ok_or(TokenError::RedactionFailed)?
                    .encoded_range = Some(range);
            }
            Value::Array(values) => {
                consume(encoded, cursor, b'[')?;
                for (index, child) in values.iter().enumerate() {
                    if index != 0 {
                        consume(encoded, cursor, b',')?;
                    }
                    path.push(Step::Index(index))?;
                    self.argument_ranges(encoded, child, cursor, path, document, password)?;
                    path.pop();
                }
                consume(encoded, cursor, b']')?;
            }
            Value::Object(fields) => {
                consume(encoded, cursor, b'{')?;
                for (index, (key, child)) in fields.iter().enumerate() {
                    if index != 0 {
                        consume(encoded, cursor, b',')?;
                    }
                    // PlainJson preserves source order after uniqueness validation.
                    // Skip the validated key lexeme; never decode/copy it a second time.
                    string_range(encoded, cursor)?;
                    consume(encoded, cursor, b':')?;
                    let password = password || (child.is_string() && password_key(key));
                    path.push(Step::Field(PayloadKey::try_from_str(
                        key,
                        &self.shared_budget,
                    )?))?;
                    self.argument_ranges(encoded, child, cursor, path, document, password)?;
                    path.pop();
                }
                consume(encoded, cursor, b'}')?;
            }
            _ => {
                let start = *cursor;
                while encoded.as_bytes().get(*cursor).is_some_and(|byte| {
                    !matches!(byte, b',' | b']' | b'}' | b' ' | b'\t' | b'\r' | b'\n')
                }) {
                    *cursor += 1;
                }
                if *cursor == start {
                    return Err(TokenError::RedactionFailed);
                }
            }
        }
        Ok(())
    }
}

fn skip_space(encoded: &str, cursor: &mut usize) {
    while encoded
        .as_bytes()
        .get(*cursor)
        .is_some_and(u8::is_ascii_whitespace)
    {
        *cursor += 1;
    }
}

fn consume(encoded: &str, cursor: &mut usize, expected: u8) -> Result<(), TokenError> {
    skip_space(encoded, cursor);
    if encoded.as_bytes().get(*cursor) != Some(&expected) {
        return Err(TokenError::RedactionFailed);
    }
    *cursor += 1;
    Ok(())
}

fn string_range(encoded: &str, cursor: &mut usize) -> Result<Range<usize>, TokenError> {
    skip_space(encoded, cursor);
    let start = *cursor;
    consume(encoded, cursor, b'"')?;
    let mut escaped = false;
    while let Some(&byte) = encoded.as_bytes().get(*cursor) {
        *cursor += 1;
        if escaped {
            escaped = false;
        } else if byte == b'\\' {
            escaped = true;
        } else if byte == b'"' {
            return Ok(start..*cursor);
        }
    }
    Err(TokenError::RedactionFailed)
}

fn at<'a>(mut value: &'a Value, path: &Path) -> Result<&'a Value, TokenError> {
    for step in path.as_slice() {
        value = match step {
            Step::Field(key) => value.get(key.as_str()),
            Step::Index(index) => value.get(*index),
        }
        .ok_or(TokenError::RedactionFailed)?;
    }
    Ok(value)
}

fn password_key(key: &str) -> bool {
    ["password", "passwd", "pwd", "passphrase", "passcode"]
        .iter()
        .any(|known| key.eq_ignore_ascii_case(known))
}

fn media_type(kind: Option<&str>) -> bool {
    matches!(
        kind,
        Some(
            "input_image"
                | "image"
                | "image_url"
                | "input_audio"
                | "output_audio"
                | "audio"
                | "input_file"
                | "file"
                | "input_video"
                | "video"
        )
    )
}

fn excluded_field(key: &str, kind: Option<&str>) -> bool {
    matches!(
        key,
        "encrypted_content"
            | "signature"
            | "compact_blob"
            | "compaction_blob"
            | "image_url"
            | "file_data"
            | "file_id"
            | "input_audio"
            | "audio_url"
            | "video_url"
    ) || (matches!(kind, Some("compaction" | "compact"))
        && matches!(key, "content" | "data" | "blob"))
}

// This is allocation/depth preflight, not a second JSON parser. The serde parser
// subsequently validates syntax; none of its diagnostic details escape.
fn preflight_json(encoded: &str, budget: &mut Budget) -> Result<(), TokenError> {
    budget.bytes(encoded.len())?;
    let mut quoted = false;
    let mut escaped = false;
    let mut primitive = false;
    let mut depth = 0usize;
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
                budget.node()?;
                quoted = true;
                primitive = false;
            }
            b'[' | b'{' => {
                budget.node()?;
                add_limited(&mut depth, 1, MAX_DEPTH)?;
                primitive = false;
            }
            b']' | b'}' => {
                depth = depth.saturating_sub(1);
                primitive = false;
            }
            b',' | b':' | b' ' | b'\t' | b'\r' | b'\n' => primitive = false,
            _ if !primitive => {
                budget.node()?;
                primitive = true;
            }
            _ => {}
        }
    }
    Ok(())
}

struct SizeWriter(usize);

impl Write for SizeWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        add_limited(&mut self.0, bytes.len(), MAX_STRING_BYTES)
            .map_err(|_| io::Error::other("carrier capacity exceeded"))?;
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

fn encoded_size(value: &str) -> Result<usize, TokenError> {
    let mut writer = SizeWriter(0);
    serde_json::to_writer(&mut writer, value).map_err(|_| TokenError::CapacityExceeded)?;
    Ok(writer.0)
}

#[cfg(test)]
pub(super) fn metadata_sizes() -> (usize, usize, usize) {
    (
        std::mem::size_of::<Step>(),
        std::mem::size_of::<Leaf>(),
        std::mem::size_of::<Document>(),
    )
}

// A serializer can only write inside the exact precharged argument capacity.
struct ArgumentWriter {
    buffer: PayloadBuffer,
    error: Option<TokenError>,
}

impl ArgumentWriter {
    fn append(&mut self, bytes: &[u8]) -> Result<(), TokenError> {
        if let Some(error) = self.error {
            return Err(error);
        }
        let result = self
            .buffer
            .len()
            .checked_add(bytes.len())
            .filter(|required| *required <= self.buffer.capacity())
            .ok_or(TokenError::CapacityExceeded)
            .and_then(|_| self.buffer.append(bytes));
        if let Err(error) = result {
            self.error = Some(error);
        }
        result
    }
}

impl Write for ArgumentWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.append(bytes)
            .map_err(|error| io::Error::other(error.code()))?;
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

// Both trees passed depth/node preflight. Match ordered structure and float bits,
// not Value equality, which ignores map order and the sign of floating zero.
pub(super) fn same_snapshot(left: &Value, right: &Value) -> bool {
    match (left, right) {
        (Value::Array(left), Value::Array(right)) => {
            left.len() == right.len()
                && left
                    .iter()
                    .zip(right)
                    .all(|(left, right)| same_snapshot(left, right))
        }
        (Value::Object(left), Value::Object(right)) => {
            left.len() == right.len()
                && left
                    .iter()
                    .zip(right)
                    .all(|((lk, lv), (rk, rv))| lk == rk && same_snapshot(lv, rv))
        }
        (Value::Number(left), Value::Number(right)) if left.is_f64() && right.is_f64() => {
            match (left.as_f64(), right.as_f64()) {
                (Some(left), Some(right)) => left.to_bits() == right.to_bits(),
                _ => false,
            }
        }
        _ => left == right,
    }
}

#[cfg(test)]
mod refusal_audit_tests {
    use super::*;
    use serde_json::json;

    fn leaf_values(
        carriers: &CodexRequestCarriers,
    ) -> Result<Vec<(usize, String, bool, bool)>, TokenError> {
        let mut values = Vec::new();
        carriers.visit(|leaf| {
            values.push((
                leaf.id,
                leaf.text.to_owned(),
                leaf.writable,
                leaf.is_password_field,
            ));
            Ok(())
        })?;
        Ok(values)
    }

    #[test]
    fn refusal_audit_malformed_arguments_preserve_all_valid_sibling_semantics(
    ) -> Result<(), TokenError> {
        for malformed in [
            r#"{"partial":"discarded","password":"unclosed"#,
            r#"{"duplicate":"left","duplicate":"right"}"#,
            r#"["complete",]"#,
            "not-json",
        ] {
            for position in [0, 3, 6] {
                let budget = PayloadBudget::with_limit(1024 * 1024);
                let mut body = json!({
                    "instructions": "synthetic-instruction",
                    "input": [
                        {"role":"user", "content":[
                            {"type":"input_text", "text":"before-message"},
                            {"type":"input_image", "image_url":"excluded-media", "caption":"excluded-caption"}
                        ]},
                        {"type":"function_call", "arguments":{
                            "password":"structured-password",
                            "nested":{"password":"nested-password"},
                            "file_data":"ordinary-argument"
                        }},
                        {"type":"function_call", "arguments":r#"{"password":"encoded-password","signature":"ordinary-signature","values":["encoded-value"],"password_context":{"nested":"not-direct-password"}}"#},
                        {"type":"function_call_output", "output":"function-output", "signature":"excluded-opaque"},
                        {"type":"reasoning", "summary":[{"type":"summary_text", "text":"visible-summary"}], "encrypted_content":"excluded-ciphertext"},
                        {"future":"read-only-sibling"}
                    ],
                    "prompt": ["synthetic-prompt"]
                });
                body["input"]
                    .as_array_mut()
                    .ok_or(TokenError::Unavailable)?
                    .insert(position, json!({"type":"function_call", "arguments":null}));
                let expected = CodexRequestCarriers::from_body_with_budget(&body, budget.clone())?;
                let expected_values = leaf_values(&expected)?;
                let expected_documents = expected.documents.len();
                drop(expected);
                assert_eq!(budget.used(), 0);

                body["input"][position]["arguments"] = Value::String(malformed.to_owned());
                assert_eq!(
                    CodexRequestCarriers::from_body_with_budget(&body, budget.clone()).err(),
                    Some(TokenError::RedactionFailed)
                );
                assert_eq!(budget.used(), 0);
                let audit =
                    CodexRequestCarriers::from_body_for_refusal_audit(&body, budget.clone())?;
                let actual = leaf_values(&audit)?;
                assert_eq!(actual, expected_values);
                assert_eq!(audit.documents.len(), expected_documents);
                assert!(same_snapshot(audit.original.as_value(), &body));
                for password in ["structured-password", "nested-password", "encoded-password"] {
                    assert!(actual.iter().any(|(_, text, writable, is_password)| {
                        text == password && *writable && *is_password
                    }));
                }
                assert!(actual.iter().any(|(_, text, writable, password)| {
                    text == "not-direct-password" && *writable && !password
                }));
                assert!(actual
                    .iter()
                    .any(|(_, text, writable, _)| { text == "read-only-sibling" && !writable }));
                assert!(!actual
                    .iter()
                    .any(|(_, text, _, _)| text.starts_with("excluded-")));
                assert!(budget.used() > 0);
                drop(audit);
                assert_eq!(budget.used(), 0);
            }
        }
        Ok(())
    }

    #[test]
    fn refusal_audit_skipped_arguments_keep_original_and_preflight_charges(
    ) -> Result<(), TokenError> {
        let malformed = r#"{"partial":"discarded""#;
        let body = json!({"input":[
            {"type":"function_call", "arguments":malformed},
            {"role":"user", "content":"synthetic-sibling"}
        ]});
        let budget = PayloadBudget::with_limit(1024 * 1024);
        let mut original = Budget::default();
        original.value(&body, 0)?;
        let audit = CodexRequestCarriers::from_body_for_refusal_audit(&body, budget.clone())?;
        assert_eq!(audit.argument_bytes, malformed.len());
        assert!(audit.budget.bytes >= original.bytes + malformed.len());
        assert!(same_snapshot(audit.original.as_value(), &body));
        assert!(leaf_values(&audit)?
            .iter()
            .any(|(_, text, _, _)| text == "synthetic-sibling"));
        drop(audit);
        assert_eq!(budget.used(), 0);
        Ok(())
    }

    #[test]
    fn refusal_audit_never_skips_capacity_errors() {
        for encoded in ["x".repeat(MAX_STRING_BYTES + 1), "[".repeat(MAX_DEPTH + 1)] {
            let budget = PayloadBudget::with_limit(16 * 1024 * 1024);
            let body = json!({"input":[
                {"type":"function_call", "arguments":encoded},
                {"role":"user", "content":"synthetic-sibling"}
            ]});
            assert_eq!(
                CodexRequestCarriers::from_body_for_refusal_audit(&body, budget.clone()).err(),
                Some(TokenError::CapacityExceeded)
            );
            assert_eq!(budget.used(), 0);
        }
        let budget = PayloadBudget::with_limit(0);
        assert_eq!(
            CodexRequestCarriers::from_body_for_refusal_audit(
                &json!({"input":"synthetic"}),
                budget.clone(),
            )
            .err(),
            Some(TokenError::CapacityExceeded)
        );
        assert_eq!(budget.used(), 0);
    }

    #[test]
    fn refusal_audit_never_skips_parser_unavailable() {
        use super::super::payload_json::test_support::{self, Action, Phase};
        for (phase, fail_at) in [
            (Phase::BeforeParse, 1),
            (Phase::BeforeParse, 2),
            (Phase::AfterParse, 1),
        ] {
            let budget = PayloadBudget::with_limit(1024 * 1024);
            let hits = std::rc::Rc::new(std::cell::Cell::new(0));
            let observed = std::rc::Rc::clone(&hits);
            let _hook = test_support::install(move |event| {
                if event.phase == phase {
                    observed.set(observed.get() + 1);
                    if observed.get() == fail_at {
                        return Action::Fail;
                    }
                }
                Action::Continue
            });
            let body = json!({"instructions":"synthetic-before", "input":[
                {"type":"function_call", "arguments":r#"{"partial":"discarded""#},
                {"type":"function_call", "arguments":r#"{"valid":"synthetic-sibling"}"#}
            ]});
            assert_eq!(
                CodexRequestCarriers::from_body_for_refusal_audit(&body, budget.clone()).err(),
                Some(TokenError::Unavailable)
            );
            assert_eq!(hits.get(), fail_at);
            assert_eq!(budget.used(), 0);
        }
    }

    #[test]
    fn refusal_audit_carriers_cannot_create_forwarding_candidates() -> Result<(), TokenError> {
        for body in [
            json!({"input":"synthetic-valid"}),
            json!({"input":[
                {"type":"function_call", "arguments":"not-json"},
                {"role":"user", "content":"synthetic-sibling"}
            ]}),
        ] {
            let budget = PayloadBudget::with_limit(1024 * 1024);
            let audit = CodexRequestCarriers::from_body_for_refusal_audit(&body, budget.clone())?;
            let mut calls = 0;
            assert_eq!(
                audit
                    .rewrite_candidate(&body, |_| {
                        calls += 1;
                        Ok(None)
                    })
                    .err(),
                Some(TokenError::RedactionFailed)
            );
            assert_eq!(calls, 0);
            assert_eq!(
                audit.visit(|_| Err(TokenError::TokenCollision)),
                Err(TokenError::TokenCollision)
            );
            drop(audit);
            assert_eq!(budget.used(), 0);
        }
        Ok(())
    }
}
