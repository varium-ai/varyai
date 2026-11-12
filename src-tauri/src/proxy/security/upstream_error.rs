//! Client-only upstream errors. Diagnostics never expose response payloads.
use super::{
    atomic_update::try_update_usize,
    payload_budget::PayloadBudget,
    payload_buffer::PayloadBuffer,
    payload_json::PayloadJson,
    payload_metadata::PayloadVec,
    payload_text::PayloadText,
    request_detection::detect_spans_with_budget,
    runtime::SecurityRuntimeConfig,
    tokenization::{visit_token_references, TokenError},
};
use crate::proxy::hyper_client::ProxyResponse;
use axum::{body::Body, response::Response};
use base64::{engine::general_purpose, Engine};
use bytes::Bytes;
use http::{header, StatusCode};
use serde::{
    ser::{SerializeMap, SerializeSeq},
    Serialize, Serializer,
};
use serde_json::Value;
use std::{
    fmt,
    ops::Range,
    sync::atomic::{AtomicUsize, Ordering},
    time::Duration,
};

const LIMIT: usize = 1024 * 1024;
const MAX_PATTERNS: usize = 4096;
const MAX_NODES: usize = 4096;
const WORK_LIMIT: usize = 32 * 1024 * 1024;
const REDACTED: &str = "[REDACTED]";

pub(crate) struct SafeUpstreamError {
    status: StatusCode,
    body: Option<Bytes>,
    reason: &'static str,
}
impl fmt::Debug for SafeUpstreamError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SafeUpstreamError")
            .field("status", &self.status.as_u16())
            .field("category", &self.reason)
            .finish()
    }
}
impl fmt::Display for SafeUpstreamError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "upstream_error HTTP {} {}",
            self.status.as_u16(),
            self.reason
        )
    }
}
impl SafeUpstreamError {
    pub(super) fn unavailable(status: StatusCode) -> Self {
        Self {
            status,
            body: None,
            reason: "body_safety_context_failed",
        }
    }
    pub(crate) fn status(&self) -> StatusCode {
        self.status
    }
    pub(crate) fn to_response(&self) -> Response {
        let body = self.body.clone().unwrap_or_else(|| {
            // Only fixed metadata is allocated outside the payload budget.
            let value = serde_json::json!({"error":{
                "message":format!("Upstream returned HTTP {}; its error body could not be safely returned ({}).", self.status.as_u16(), self.reason),
                "type":"upstream_error", "code":"upstream_error_body_unavailable", "param":null
            }});
            Bytes::from(value.to_string())
        });
        let mut response = Response::new(Body::from(body));
        *response.status_mut() = self.status;
        response.headers_mut().insert(
            header::CONTENT_TYPE,
            http::HeaderValue::from_static("application/json"),
        );
        response
    }
}

// Independent of log redaction; this owner is never formatted or persisted.
pub(crate) struct ErrorCredentials {
    values: PayloadVec<PayloadText>,
}
impl ErrorCredentials {
    pub(super) fn capture(
        headers: &http::HeaderMap,
        budget: PayloadBudget,
    ) -> Result<Self, TokenError> {
        let mut values = PayloadVec::new(budget.clone())?;
        for name in [
            "authorization",
            "x-api-key",
            "x-goog-api-key",
            "cookie",
            "proxy-authorization",
        ] {
            for value in headers.get_all(name) {
                let raw = value.to_str().map_err(|_| TokenError::RedactionFailed)?;
                if raw.is_empty() {
                    continue;
                }
                if values.len() >= 32 {
                    return Err(TokenError::CapacityExceeded);
                }
                values.push(PayloadText::try_from_str(raw, budget.clone(), 64 * 1024)?)?;
            }
        }
        Ok(Self { values })
    }
}

pub(super) struct ErrorSafetyContext<'a> {
    pub(super) runtime: &'a SecurityRuntimeConfig,
    pub(super) request: &'a Value,
    pub(super) provider: &'a Value,
    pub(super) credentials: &'a [String],
    pub(super) sent_credentials: Option<&'a ErrorCredentials>,
    pub(super) budget: PayloadBudget,
}

/// Prepared, budget-owned secrets for a later native error; never completion authority.
/// Only policy metadata is cloned; all private patterns retain their original charges.
pub(super) struct NativeErrorSafety {
    runtime: SecurityRuntimeConfig,
    patterns: PayloadVec<PayloadText>,
    pattern_bytes: usize,
    nodes: usize,
    work: Work,
    budget: PayloadBudget,
}
impl NativeErrorSafety {
    pub(super) fn prepare(context: ErrorSafetyContext<'_>) -> Result<Self, TokenError> {
        let safety = Safety::build(&context)?;
        Ok(Self {
            runtime: context.runtime.clone(),
            patterns: safety.patterns,
            pattern_bytes: safety.pattern_bytes,
            nodes: safety.nodes,
            work: safety.work,
            budget: context.budget.clone(),
        })
    }
    pub(super) fn sanitize(self, value: &Value) -> Result<PayloadBuffer, TokenError> {
        let context = ErrorSafetyContext {
            runtime: &self.runtime,
            request: &Value::Null,
            provider: &Value::Null,
            credentials: &[],
            sent_credentials: None,
            budget: self.budget.clone(),
        };
        let safety = Safety {
            context: &context,
            patterns: self.patterns,
            pattern_bytes: self.pattern_bytes,
            nodes: self.nodes,
            work: self.work,
        };
        encode_checked(
            &CheckedValue {
                value,
                safety: &safety,
                depth: 0,
            },
            &self.budget,
        )
    }
}

// Shared across fields and both serialization passes, not reset per string.
struct Work(AtomicUsize);
impl Work {
    fn charge(&self, bytes: usize) -> Result<(), TokenError> {
        try_update_usize(&self.0, Ordering::Relaxed, Ordering::Relaxed, |used| {
            used.checked_add(bytes).filter(|next| *next <= WORK_LIMIT)
        })
        .map(|_| ())
        .map_err(|_| TokenError::CapacityExceeded)
    }
}
#[derive(Clone, Copy)]
enum SourcePosition {
    RequestRoot,
    ProviderRoot,
    Items,
    Item,
    Parts,
    Part,
    Private,
}
struct Safety<'a> {
    context: &'a ErrorSafetyContext<'a>,
    patterns: PayloadVec<PayloadText>,
    pattern_bytes: usize,
    nodes: usize,
    work: Work,
}
impl<'a> Safety<'a> {
    fn build(context: &'a ErrorSafetyContext<'a>) -> Result<Self, TokenError> {
        let mut safety = Self {
            context,
            patterns: PayloadVec::new(context.budget.clone())?,
            pattern_bytes: 0,
            nodes: 0,
            work: Work(AtomicUsize::new(0)),
        };
        safety.source(context.request, false, 0, SourcePosition::RequestRoot)?;
        safety.source(context.provider, true, 0, SourcePosition::ProviderRoot)?;
        for raw in context.credentials {
            safety.secret(raw)?;
        }
        if let Some(credentials) = context.sent_credentials {
            for raw in credentials.values.as_slice() {
                safety.secret(raw.as_str())?;
            }
        }
        Ok(safety)
    }
    fn pattern(&mut self, raw: &str) -> Result<(), TokenError> {
        if raw.is_empty() {
            return Ok(());
        }
        self.work.charge(raw.len())?;
        self.pattern_bytes = self
            .pattern_bytes
            .checked_add(raw.len())
            .filter(|size| *size <= LIMIT)
            .ok_or(TokenError::CapacityExceeded)?;
        if self.patterns.len() >= MAX_PATTERNS {
            return Err(TokenError::CapacityExceeded);
        }
        self.patterns.push(PayloadText::try_from_str(
            raw,
            self.context.budget.clone(),
            LIMIT,
        )?)
    }
    fn variants(&mut self, raw: &str) -> Result<(), TokenError> {
        if raw.is_empty() {
            return Ok(());
        }
        self.pattern(raw)?;
        // JSON escaping is budgeted before serialization, including nested JSON strings.
        let escaped = encode_checked(&raw, &self.context.budget)?;
        let escaped =
            std::str::from_utf8(escaped.as_slice()).map_err(|_| TokenError::RedactionFailed)?;
        if let Some(inner) = escaped.get(1..escaped.len().saturating_sub(1)) {
            if inner != raw {
                self.pattern(inner)?;
            }
        }
        let capacity = raw
            .len()
            .checked_add(2)
            .and_then(|size| size.checked_div(3))
            .and_then(|size| size.checked_mul(4))
            .ok_or(TokenError::CapacityExceeded)?;
        // Reserve scratch before encoding; encode_slice cannot grow it.
        for engine in [
            &general_purpose::STANDARD,
            &general_purpose::STANDARD_NO_PAD,
            &general_purpose::URL_SAFE,
            &general_purpose::URL_SAFE_NO_PAD,
        ] {
            let mut scratch =
                PayloadBuffer::with_capacity(capacity, self.context.budget.clone(), LIMIT)?;
            let mut input = [0u8; 3];
            for chunk in raw.as_bytes().chunks(3) {
                input[..chunk.len()].copy_from_slice(chunk);
                let mut output = [0u8; 4];
                let count = engine
                    .encode_slice(&input[..chunk.len()], &mut output)
                    .map_err(|_| TokenError::CapacityExceeded)?;
                scratch.append(&output[..count])?;
            }
            let encoded =
                std::str::from_utf8(scratch.as_slice()).map_err(|_| TokenError::RedactionFailed)?;
            self.pattern(encoded)?;
        }
        for hex in [b"0123456789ABCDEF", b"0123456789abcdef"] {
            let mut escaped = PayloadText::with_capacity(
                raw.len()
                    .checked_mul(3)
                    .ok_or(TokenError::CapacityExceeded)?,
                self.context.budget.clone(),
                LIMIT,
            )?;
            for byte in raw.bytes() {
                if byte.is_ascii_alphanumeric() || b"-._~".contains(&byte) {
                    let one = [byte];
                    escaped.append(
                        std::str::from_utf8(&one).map_err(|_| TokenError::RedactionFailed)?,
                    )?;
                } else {
                    let part = [
                        b'%',
                        hex[usize::from(byte >> 4)],
                        hex[usize::from(byte & 15)],
                    ];
                    escaped.append(
                        std::str::from_utf8(&part).map_err(|_| TokenError::RedactionFailed)?,
                    )?;
                }
            }
            if escaped.as_str() != raw {
                self.pattern(escaped.as_str())?;
            }
        }
        Ok(())
    }
    fn secret(&mut self, raw: &str) -> Result<(), TokenError> {
        self.variants(raw)?;
        // Authentication components remain private even when the wrapper is omitted.
        for part in
            raw.split(|c: char| c.is_ascii_whitespace() || matches!(c, ';' | ',' | '=' | '"' | ':'))
        {
            if !part.is_empty() {
                self.variants(part)?;
            }
        }
        let mut components = raw.split_ascii_whitespace();
        if let (Some(scheme), Some(encoded)) = (components.next(), components.next()) {
            if scheme.eq_ignore_ascii_case("basic") {
                if components.next().is_some() {
                    return Err(TokenError::RedactionFailed);
                }
                self.work.charge(encoded.len())?;
                let mut decoded = PayloadBuffer::with_capacity(
                    encoded.len(),
                    self.context.budget.clone(),
                    LIMIT,
                )?;
                for chunk in encoded.as_bytes().chunks(4) {
                    let mut output = [0u8; 3];
                    let count = general_purpose::STANDARD
                        .decode_slice(chunk, &mut output)
                        .map_err(|_| TokenError::RedactionFailed)?;
                    decoded.append(&output[..count])?;
                }
                let text = std::str::from_utf8(decoded.as_slice())
                    .map_err(|_| TokenError::RedactionFailed)?;
                self.variants(text)?;
                for part in text.split(':') {
                    self.variants(part)?;
                }
            }
        }
        Ok(())
    }
    fn url_component(&mut self, raw: &str) -> Result<(), TokenError> {
        self.secret(raw)?;
        let mut decoded =
            PayloadBuffer::with_capacity(raw.len(), self.context.budget.clone(), LIMIT)?;
        let mut bytes = raw.bytes();
        while let Some(byte) = bytes.next() {
            let byte = if byte == b'%' {
                let high = bytes
                    .next()
                    .and_then(|b| char::from(b).to_digit(16))
                    .ok_or(TokenError::RedactionFailed)?;
                let low = bytes
                    .next()
                    .and_then(|b| char::from(b).to_digit(16))
                    .ok_or(TokenError::RedactionFailed)?;
                ((high << 4) | low) as u8
            } else {
                byte
            };
            decoded.append(&[byte])?;
        }
        let decoded =
            std::str::from_utf8(decoded.as_slice()).map_err(|_| TokenError::RedactionFailed)?;
        self.secret(decoded)
    }
    fn private_url(&mut self, raw: &str) -> Result<(), TokenError> {
        self.variants(raw)?;
        // Borrowed decomposition avoids uncharged URL normalization allocations.
        let (_, rest) = raw.split_once("://").ok_or(TokenError::RedactionFailed)?;
        let authority_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
        let authority = &rest[..authority_end];
        let host = if let Some((user, host)) = authority.rsplit_once('@') {
            for component in user.split(':') {
                self.url_component(component)?;
            }
            host
        } else {
            authority
        };
        self.url_component(host)?;
        if host.starts_with('[') {
            if let Some((host, _)) = host.split_once(']') {
                self.url_component(host.trim_start_matches('['))?;
            }
        } else if let Some((host, _)) = host.split_once(':') {
            self.url_component(host)?;
        }
        for component in rest[authority_end..].split(['/', '?', '#', '&', '=']) {
            self.url_component(component)?;
        }
        Ok(())
    }
    fn private_text(&mut self, raw: &str, depth: usize) -> Result<(), TokenError> {
        self.variants(raw)?;
        if raw.trim_start().starts_with(['{', '[']) {
            let parsed = PayloadJson::try_from_str(raw, self.context.budget.clone(), LIMIT)?;
            self.source(parsed.as_value(), true, depth + 1, SourcePosition::Private)?;
        }
        for part in raw.split(|c: char| !c.is_alphanumeric() && c != '_' && c != '-') {
            if !part.is_empty() && part != raw {
                self.variants(part)?;
            }
        }
        Ok(())
    }
    fn source(
        &mut self,
        value: &Value,
        private: bool,
        depth: usize,
        position: SourcePosition,
    ) -> Result<(), TokenError> {
        self.nodes += 1;
        if depth > 64 || self.nodes > MAX_NODES {
            return Err(TokenError::CapacityExceeded);
        }
        self.work.charge(1)?;
        match value {
            Value::String(raw) if private => self.private_text(raw, depth)?,
            Value::Number(_) | Value::Bool(_) if private => {
                let encoded = encode_checked(value, &self.context.budget)?;
                self.variants(
                    std::str::from_utf8(encoded.as_slice())
                        .map_err(|_| TokenError::RedactionFailed)?,
                )?;
            }
            Value::Array(values) => {
                for value in values {
                    let child = match position {
                        SourcePosition::Items => SourcePosition::Item,
                        SourcePosition::Parts => SourcePosition::Part,
                        _ => SourcePosition::Private,
                    };
                    self.source(value, private, depth + 1, child)?;
                }
            }
            Value::Object(values) => {
                for (key, value) in values {
                    self.work.charge(key.len())?;
                    let protocol_field = matches!(
                        (position, key.as_str(), value.as_str()),
                        (
                            SourcePosition::Item,
                            "type",
                            Some(
                                "message" | "function_call" | "function_call_output" | "reasoning"
                            )
                        ) | (
                            SourcePosition::Item,
                            "role",
                            Some("user" | "assistant" | "system" | "developer" | "tool")
                        ) | (
                            SourcePosition::Item,
                            "status",
                            Some("completed" | "in_progress" | "incomplete")
                        ) | (
                            SourcePosition::Part,
                            "type",
                            Some("input_text" | "output_text" | "refusal")
                        )
                    );
                    if protocol_field {
                        continue;
                    }
                    let root = matches!(
                        position,
                        SourcePosition::RequestRoot | SourcePosition::ProviderRoot
                    );
                    // Only root protocol/config identifiers are diagnostic, never nested names.
                    if root
                        && matches!(key.as_str(), "model" | "api_format" | "apiFormat")
                        && value.is_string()
                    {
                        continue;
                    }
                    if root && matches!(key.as_str(), "base_url" | "baseURL") {
                        if let Some(raw) = value.as_str() {
                            self.private_url(raw)?;
                        } else {
                            return Err(TokenError::RedactionFailed);
                        }
                        continue;
                    }
                    let protocol_key = matches!(
                        (position, key.as_str()),
                        (
                            SourcePosition::Item,
                            "type"
                                | "role"
                                | "status"
                                | "content"
                                | "text"
                                | "arguments"
                                | "name"
                                | "call_id"
                                | "id"
                                | "output"
                                | "summary"
                        ) | (SourcePosition::Part, "type" | "text" | "refusal")
                    );
                    if private && !protocol_key {
                        self.variants(key)?;
                    }
                    let protected = private
                        || matches!(
                            key.as_str(),
                            "input"
                                | "messages"
                                | "content"
                                | "text"
                                | "arguments"
                                | "instructions"
                                | "config"
                                | "env"
                                | "api_key"
                                | "apiKey"
                                | "authorization"
                                | "password"
                                | "token"
                                | "tools"
                                | "metadata"
                                | "system"
                                | "prompt"
                        );
                    let child = match (position, key.as_str()) {
                        (SourcePosition::RequestRoot, "input" | "messages") => {
                            SourcePosition::Items
                        }
                        (SourcePosition::Item, "content") => SourcePosition::Parts,
                        _ => SourcePosition::Private,
                    };
                    self.source(value, protected, depth + 1, child)?;
                }
            }
            _ => {}
        }
        Ok(())
    }
    fn sanitized(&self, text: &str) -> Result<PayloadText, TokenError> {
        self.sanitized_at(text, 0)
    }
    fn unicode_ranges(
        &self,
        text: &str,
        depth: usize,
        add: &mut impl FnMut(Range<usize>) -> Result<(), TokenError>,
    ) -> Result<(), TokenError> {
        if !text.as_bytes().windows(2).any(|pair| {
            pair[0] == b'\\'
                && matches!(
                    pair[1],
                    b'"' | b'\\' | b'/' | b'b' | b'f' | b'n' | b'r' | b't' | b'u'
                )
        }) {
            return Ok(());
        }
        self.work.charge(text.len())?;
        let mut decoded =
            PayloadText::with_capacity(text.len(), self.context.budget.clone(), LIMIT)?;
        let mut cursor = 0;
        let mut copied = 0;
        while cursor < text.len() {
            let bytes = text.as_bytes();
            if bytes[cursor] != b'\\' {
                cursor += 1;
                continue;
            }
            let Some(next) = bytes.get(cursor + 1) else {
                break;
            };
            let (character, consumed) = match next {
                b'\\' => ('\\', 2),
                b'/' => ('/', 2),
                b'"' => ('"', 2),
                b'b' => ('\u{0008}', 2),
                b'f' => ('\u{000c}', 2),
                b'n' => ('\n', 2),
                b'r' => ('\r', 2),
                b't' => ('\t', 2),
                b'u' => {
                    let unit = unicode_unit(
                        bytes
                            .get(cursor + 2..cursor + 6)
                            .ok_or(TokenError::RedactionFailed)?,
                    )?;
                    if (0xD800..=0xDBFF).contains(&unit) {
                        if bytes.get(cursor + 6..cursor + 8) != Some(b"\\u") {
                            return Err(TokenError::RedactionFailed);
                        }
                        let low = unicode_unit(
                            bytes
                                .get(cursor + 8..cursor + 12)
                                .ok_or(TokenError::RedactionFailed)?,
                        )?;
                        if !(0xDC00..=0xDFFF).contains(&low) {
                            return Err(TokenError::RedactionFailed);
                        }
                        (
                            char::from_u32(
                                0x10000
                                    + ((u32::from(unit) - 0xD800) << 10)
                                    + (u32::from(low) - 0xDC00),
                            )
                            .ok_or(TokenError::RedactionFailed)?,
                            12,
                        )
                    } else {
                        (
                            char::from_u32(u32::from(unit)).ok_or(TokenError::RedactionFailed)?,
                            6,
                        )
                    }
                }
                _ => {
                    cursor += 1;
                    continue;
                }
            };
            decoded.append(
                text.get(copied..cursor)
                    .ok_or(TokenError::RedactionFailed)?,
            )?;
            let mut utf8 = [0u8; 4];
            decoded.append(character.encode_utf8(&mut utf8))?;
            cursor += consumed;
            copied = cursor;
        }
        decoded.append(text.get(copied..).ok_or(TokenError::RedactionFailed)?)?;
        if decoded.as_str() != text {
            if depth >= 3 {
                return Err(TokenError::CapacityExceeded);
            }
            if self.sanitized_at(decoded.as_str(), depth + 1)?.as_str() != decoded.as_str() {
                add(0..text.len())?;
            }
        }
        Ok(())
    }
    fn encoded_ranges(
        &self,
        text: &str,
        depth: usize,
        add: &mut impl FnMut(Range<usize>) -> Result<(), TokenError>,
    ) -> Result<(), TokenError> {
        // Decode candidate runs only for inspection; decoded content is never emitted.
        for percent in [false, true] {
            let candidate = |byte: u8| {
                byte.is_ascii_alphanumeric()
                    || if percent {
                        b"%-._~+".contains(&byte)
                    } else {
                        b"+/=_-".contains(&byte)
                    }
            };
            let mut start = 0;
            while start < text.len() {
                if !candidate(text.as_bytes()[start]) {
                    start += 1;
                    continue;
                }
                let mut end = start + 1;
                while end < text.len() && candidate(text.as_bytes()[end]) {
                    end += 1;
                }
                let raw = &text[start..end];
                self.work.charge(raw.len())?;
                if (percent && raw.contains('%')) || (!percent && raw.len() >= 8) {
                    let mut decoded = PayloadBuffer::with_capacity(
                        raw.len(),
                        self.context.budget.clone(),
                        LIMIT,
                    )?;
                    let valid = if percent {
                        let mut bytes = raw.bytes();
                        while let Some(byte) = bytes.next() {
                            let byte = if byte == b'%' {
                                let high = bytes
                                    .next()
                                    .and_then(|b| char::from(b).to_digit(16))
                                    .ok_or(TokenError::RedactionFailed)?;
                                let low = bytes
                                    .next()
                                    .and_then(|b| char::from(b).to_digit(16))
                                    .ok_or(TokenError::RedactionFailed)?;
                                ((high << 4) | low) as u8
                            } else {
                                byte
                            };
                            decoded.append(&[byte])?;
                        }
                        true
                    } else {
                        let engine = if raw.contains(['-', '_']) {
                            &general_purpose::URL_SAFE_NO_PAD
                        } else {
                            &general_purpose::STANDARD_NO_PAD
                        };
                        let mut valid = true;
                        for chunk in raw.trim_end_matches('=').as_bytes().chunks(4) {
                            let mut output = [0u8; 3];
                            match engine.decode_slice(chunk, &mut output) {
                                Ok(count) => decoded.append(&output[..count])?,
                                Err(_) => {
                                    valid = false;
                                    break;
                                }
                            }
                        }
                        valid
                    };
                    if valid {
                        if let Ok(decoded) = std::str::from_utf8(decoded.as_slice()) {
                            if depth >= 3 {
                                return Err(TokenError::CapacityExceeded);
                            }
                            if self.sanitized_at(decoded, depth + 1)?.as_str() != decoded {
                                add(start..end)?;
                            }
                        } else if percent {
                            return Err(TokenError::RedactionFailed);
                        }
                    }
                }
                start = end;
            }
        }
        Ok(())
    }
    fn sanitized_at(&self, text: &str, depth: usize) -> Result<PayloadText, TokenError> {
        self.work.charge(text.len().max(1))?;
        let mut ranges = PayloadVec::new(self.context.budget.clone())?;
        let mut add = |range: Range<usize>| {
            if ranges.len() >= 4096 {
                return Err(TokenError::CapacityExceeded);
            }
            if !range.is_empty() {
                ranges.push(range)?;
            }
            Ok(())
        };
        visit_token_references(text, &mut add)?;
        for span in detect_spans_with_budget(
            text,
            false,
            self.context.runtime,
            self.context.budget.clone(),
        )?
        .as_slice()
        {
            add(span.range.clone())?;
        }
        for pattern in self.patterns.as_slice() {
            self.work
                .charge(text.len().saturating_add(pattern.as_str().len()))?;
            for (start, _) in text.match_indices(pattern.as_str()) {
                add(start..start + pattern.as_str().len())?;
            }
        }
        self.encoded_ranges(text, depth, &mut add)?;
        let direct = self.apply_ranges(text, &mut ranges)?;
        let mut encoded = PayloadVec::new(self.context.budget.clone())?;
        let mut add_encoded = |range: Range<usize>| {
            if encoded.len() >= 4096 {
                return Err(TokenError::CapacityExceeded);
            }
            if !range.is_empty() {
                encoded.push(range)?;
            }
            Ok(())
        };
        self.unicode_ranges(direct.as_str(), depth, &mut add_encoded)?;
        self.encoded_ranges(direct.as_str(), depth, &mut add_encoded)?;
        if encoded.as_slice().is_empty() {
            return Ok(direct);
        }
        self.apply_ranges(direct.as_str(), &mut encoded)
    }
    fn apply_ranges(
        &self,
        text: &str,
        ranges: &mut PayloadVec<Range<usize>>,
    ) -> Result<PayloadText, TokenError> {
        self.work.charge(text.len().saturating_add(ranges.len()))?;
        ranges.sort_unstable_by(|a, b| a.start.cmp(&b.start).then(a.end.cmp(&b.end)));
        let capacity = text
            .len()
            .checked_add(
                ranges
                    .len()
                    .checked_mul(REDACTED.len())
                    .ok_or(TokenError::CapacityExceeded)?,
            )
            .ok_or(TokenError::CapacityExceeded)?;
        let mut result = PayloadText::with_capacity(capacity, self.context.budget.clone(), LIMIT)?;
        let mut cursor = 0;
        for range in ranges.as_slice() {
            if range.end <= cursor {
                continue;
            }
            if range.start >= cursor {
                result.append(
                    text.get(cursor..range.start)
                        .ok_or(TokenError::RedactionFailed)?,
                )?;
                result.append(REDACTED)?;
            }
            cursor = range.end;
        }
        result.append(text.get(cursor..).ok_or(TokenError::RedactionFailed)?)?;
        Ok(result)
    }
}
fn unicode_unit(bytes: &[u8]) -> Result<u16, TokenError> {
    bytes.iter().try_fold(0u16, |value, byte| {
        let digit = char::from(*byte)
            .to_digit(16)
            .ok_or(TokenError::RedactionFailed)?;
        Ok((value << 4) | (digit as u16))
    })
}
fn private_field(key: &str) -> bool {
    [
        "authorization",
        "api_key",
        "apikey",
        "password",
        "secret",
        "token",
        "access_token",
        "refresh_token",
        "arguments",
        "tool_input",
        "config",
        "env",
        "request",
        "request_body",
        "headers",
    ]
    .iter()
    .any(|field| key.eq_ignore_ascii_case(field))
}
struct CheckedValue<'a, 'b> {
    value: &'a Value,
    safety: &'b Safety<'b>,
    depth: usize,
}
impl Serialize for CheckedValue<'_, '_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        if self.depth > 64 {
            return Err(serde::ser::Error::custom("error_body_depth"));
        }
        self.safety
            .work
            .charge(1)
            .map_err(|_| serde::ser::Error::custom("error_body_work"))?;
        match self.value {
            Value::String(text) => {
                let safe = self
                    .safety
                    .sanitized(text)
                    .map_err(|_| serde::ser::Error::custom("error_body_safety"))?;
                serializer.serialize_str(safe.as_str())
            }
            Value::Array(values) => {
                let mut seq = serializer.serialize_seq(Some(values.len()))?;
                for value in values {
                    seq.serialize_element(&CheckedValue {
                        value,
                        safety: self.safety,
                        depth: self.depth + 1,
                    })?;
                }
                seq.end()
            }
            Value::Object(values) => {
                let mut map = serializer.serialize_map(None)?;
                for (key, value) in values {
                    let safe = self
                        .safety
                        .sanitized(key)
                        .map_err(|_| serde::ser::Error::custom("error_body_safety"))?;
                    // Dropping unsafe keys cannot introduce renaming collisions.
                    if safe.as_str() != key {
                        continue;
                    }
                    if private_field(key) {
                        map.serialize_entry(key, REDACTED)?;
                    } else {
                        map.serialize_entry(
                            key,
                            &CheckedValue {
                                value,
                                safety: self.safety,
                                depth: self.depth + 1,
                            },
                        )?;
                    }
                }
                map.end()
            }
            Value::Number(_) | Value::Bool(_) => {
                let encoded = encode_checked(self.value, &self.safety.context.budget)
                    .map_err(|_| serde::ser::Error::custom("error_body_capacity"))?;
                let raw = std::str::from_utf8(encoded.as_slice())
                    .map_err(|_| serde::ser::Error::custom("error_body_encoding"))?;
                let safe = self
                    .safety
                    .sanitized(raw)
                    .map_err(|_| serde::ser::Error::custom("error_body_safety"))?;
                if safe.as_str() == raw {
                    self.value.serialize(serializer)
                } else {
                    serializer.serialize_str(REDACTED)
                }
            }
            other => other.serialize(serializer),
        }
    }
}
struct BufferWriter<'a>(&'a mut PayloadBuffer);
struct SizeWriter(usize);
impl std::io::Write for SizeWriter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0 = self
            .0
            .checked_add(bytes.len())
            .filter(|size| *size <= LIMIT)
            .ok_or_else(|| std::io::Error::other("error_body_capacity"))?;
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
fn encode_checked(
    value: &impl Serialize,
    budget: &PayloadBudget,
) -> Result<PayloadBuffer, TokenError> {
    let mut count = SizeWriter(0);
    serde_json::to_writer(&mut count, value).map_err(|_| TokenError::RedactionFailed)?;
    let mut wire = PayloadBuffer::with_capacity(count.0, budget.clone(), LIMIT)?;
    serde_json::to_writer(BufferWriter(&mut wire), value)
        .map_err(|_| TokenError::RedactionFailed)?;
    if wire.len() != count.0 {
        return Err(TokenError::RedactionFailed);
    }
    Ok(wire)
}
#[derive(Serialize)]
struct TextEnvelope<'a> {
    error: TextError<'a>,
}
#[derive(Serialize)]
struct TextError<'a> {
    message: &'a str,
    r#type: &'static str,
    code: &'static str,
    param: Option<()>,
}
impl std::io::Write for BufferWriter<'_> {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0
            .append(bytes)
            .map_err(|_| std::io::Error::other("error_body_capacity"))?;
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
struct OwnedWire(PayloadBuffer);
impl AsRef<[u8]> for OwnedWire {
    fn as_ref(&self) -> &[u8] {
        self.0.as_slice()
    }
}

pub(super) async fn read_error(
    response: ProxyResponse,
    context: ErrorSafetyContext<'_>,
    timeout: Duration,
) -> SafeUpstreamError {
    let status = response.status();
    let operation = async {
        let guarded =
            super::upstream_guard::guard_upstream_error_response(response, &context.budget)
                .await
                .map_err(|_| "body_read_or_decode_failed")?;
        let raw = guarded
            .response
            .bytes_with_limit(LIMIT)
            .await
            .map_err(|_| "body_limit_or_read_failed")?;
        let text = std::str::from_utf8(&raw).map_err(|_| "body_encoding_failed")?;
        let safety = Safety::build(&context).map_err(|_| "body_safety_failed")?;
        let wire = if text.trim_start().starts_with(['{', '[']) {
            let parsed = PayloadJson::try_from_str(text, context.budget.clone(), LIMIT)
                .map_err(|_| "body_json_failed")?;
            encode_checked(
                &CheckedValue {
                    value: parsed.as_value(),
                    safety: &safety,
                    depth: 0,
                },
                &context.budget,
            )
            .map_err(|_| "body_safety_failed")?
        } else {
            let safe = safety.sanitized(text).map_err(|_| "body_safety_failed")?;
            encode_checked(
                &TextEnvelope {
                    error: TextError {
                        message: safe.as_str(),
                        r#type: "upstream_error",
                        code: "upstream_error",
                        param: None,
                    },
                },
                &context.budget,
            )
            .map_err(|_| "body_capacity_failed")?
        };
        drop(guarded.owner);
        Ok::<_, &'static str>(Bytes::from_owner(OwnedWire(wire)))
    };
    match tokio::time::timeout(timeout, operation).await {
        Ok(Ok(body)) => SafeUpstreamError {
            status,
            body: Some(body),
            reason: "upstream_error",
        },
        Ok(Err(reason)) => SafeUpstreamError {
            status,
            body: None,
            reason,
        },
        Err(_) => SafeUpstreamError {
            status,
            body: None,
            reason: "body_read_timeout",
        },
    }
}
