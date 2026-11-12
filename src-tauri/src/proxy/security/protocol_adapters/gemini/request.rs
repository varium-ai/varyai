//! GE1 borrowed request inspection only; native admission remains disabled.

use http::{Method, Uri};
use serde_json::Value;

use crate::proxy::{
    protocol_bridge::Protocol,
    security::{
        payload_budget::PayloadBudget,
        request_plan::PlanError,
        restoration_contract::{
            ClientProtocol, ContractError, EditConstraint, Framing, HistoryClaim, Operation,
            RequestObservation, SourceClass,
        },
        tokenization::TokenError,
    },
};

// Match the shared native membership search's root-zero depth and work ceilings.
const MAX_NATIVE_DEPTH: usize = 64;
const MAX_NATIVE_WORK: usize = 4 * 1024 * 1024;

/// Descriptive classification only; never grants admission or restoration authority.
pub(super) fn classify_generation(
    method: &Method,
    uri: &Uri,
) -> Result<(Operation, Framing), PlanError> {
    if method != Method::POST {
        return Err(PlanError::Unsupported);
    }
    let model_and_operation = [
        "/v1beta/models/",
        "/gemini/v1beta/models/",
        "/gemini/v1/models/",
    ]
    .into_iter()
    .find_map(|prefix| uri.path().strip_prefix(prefix));
    let Some((model, operation)) = model_and_operation.and_then(|path| path.split_once(':')) else {
        return Err(PlanError::Unsupported);
    };
    if model.is_empty() || model.contains('/') {
        return Err(PlanError::Unsupported);
    }
    // Bare alt keys still count, so duplicate or incomplete framing cannot be admitted.
    let mut alt = uri
        .query()
        .into_iter()
        .flat_map(|query| query.split('&'))
        .map(|pair| match pair.split_once('=') {
            Some(pair) => pair,
            None => (pair, ""),
        })
        .filter(|(key, _)| *key == "alt")
        .map(|(_, value)| value);
    let alt_value = alt.next();
    if alt.next().is_some() {
        return Err(PlanError::Unsupported);
    }
    let framing = match (operation, alt_value) {
        ("generateContent", None | Some("json")) => Framing::Json,
        ("streamGenerateContent", Some("sse")) => Framing::Sse,
        _ => return Err(PlanError::Unsupported),
    };
    Ok((Operation::GenerateContent, framing))
}

/// Borrow the exact native owner and charge metadata to the caller's existing budget.
/// Unsupported framing is rejected before allocating any observation metadata.
pub(super) fn observe_request<'native>(
    method: &Method,
    uri: &Uri,
    native: &'native Value,
    budget: PayloadBudget,
) -> Result<RequestObservation<'native>, PlanError> {
    classify_generation(method, uri)?;
    observe_admitted_request(native, budget)
}

/// Observe only after handler-owned admission already fixed a supported generation
/// route and framing; this never re-derives route authority from the body.
pub(in crate::proxy::security) fn observe_admitted_request<'native>(
    native: &'native Value,
    budget: PayloadBudget,
) -> Result<RequestObservation<'native>, PlanError> {
    if !native.is_object() {
        return Err(PlanError::Unsupported);
    }
    let mut walker = RequestWalker {
        observation: RequestObservation::new(native, ClientProtocol::new(Protocol::Gemini), budget)
            .map_err(contract_error)?,
        remaining_work: MAX_NATIVE_WORK,
    };
    walker.visit(0)?;
    // Preserve both evidenced ProtoJSON spellings without renaming or copying either field.
    for field in ["systemInstruction", "system_instruction"] {
        if let Some(instruction) = native.get(field) {
            walker.content(instruction, 1, SourceClass::CurrentInput, false)?;
        }
    }
    if let Some(contents) = native.get("contents") {
        walker.visit(1)?;
        if let Some(contents) = contents.as_array() {
            for content in contents {
                let source = if content.get("role").and_then(Value::as_str) == Some("model") {
                    SourceClass::HistoricalText
                } else {
                    SourceClass::CurrentInput
                };
                walker.content(content, 2, source, true)?;
            }
        }
    }
    Ok(walker.observation)
}

fn contract_error(error: ContractError) -> PlanError {
    match error {
        ContractError::Resource(error) => PlanError::Token(error),
        ContractError::ImmutableSite => PlanError::ImmutableContent,
        _ => PlanError::Unsupported,
    }
}

struct RequestWalker<'native> {
    observation: RequestObservation<'native>,
    remaining_work: usize,
}

impl<'native> RequestWalker<'native> {
    fn visit(&mut self, depth: usize) -> Result<(), PlanError> {
        if depth > MAX_NATIVE_DEPTH {
            return Err(TokenError::CapacityExceeded.into());
        }
        self.remaining_work = self
            .remaining_work
            .checked_sub(1)
            .ok_or(TokenError::CapacityExceeded)?;
        Ok(())
    }

    fn content(
        &mut self,
        content: &'native Value,
        depth: usize,
        source: SourceClass,
        include_functions: bool,
    ) -> Result<(), PlanError> {
        self.visit(depth)?;
        if source == SourceClass::HistoricalText {
            // A model Content is only a replay claim, never proof of delivery or execution.
            self.observation
                .claim_history(HistoryClaim::AssistantItem { native: content })
                .map_err(contract_error)?;
        }
        if let Some(parts) = content.get("parts") {
            self.visit(depth + 1)?;
            if let Some(parts) = parts.as_array() {
                for part in parts {
                    self.part(part, depth + 2, source, include_functions)?;
                }
            }
        }
        Ok(())
    }

    fn part(
        &mut self,
        part: &'native Value,
        depth: usize,
        source: SourceClass,
        include_functions: bool,
    ) -> Result<(), PlanError> {
        self.visit(depth)?;
        // Presence fixes the whole Part's editability before any readable leaf is observed.
        let constraint =
            if part.get("thoughtSignature").is_some() || part.get("thought_signature").is_some() {
                EditConstraint::Immutable
            } else {
                EditConstraint::Editable
            };
        if let Some(text) = part.get("text") {
            self.visit(depth + 1)?;
            if text.is_string() {
                self.observation
                    .observe_text(text, source, constraint)
                    .map_err(contract_error)?;
            }
        }
        if include_functions {
            for (carrier, field, source) in [
                ("functionCall", "args", SourceClass::ToolArguments),
                ("functionResponse", "response", SourceClass::ToolResult),
            ] {
                if let Some(function) = part.get(carrier) {
                    self.visit(depth + 1)?;
                    // Native args/results are Structs; never serialize a whole argument object.
                    if let Some(value) = function.get(field).filter(|value| value.is_object()) {
                        self.string_values(value, depth + 2, source, constraint)?;
                    }
                }
            }
        }
        // Signatures, media, call names/IDs and unknown members remain opaque.
        Ok(())
    }

    fn string_values(
        &mut self,
        value: &'native Value,
        depth: usize,
        source: SourceClass,
        constraint: EditConstraint,
    ) -> Result<(), PlanError> {
        self.visit(depth)?;
        match value {
            Value::String(_) => self
                .observation
                .observe_text(value, source, constraint)
                .map_err(contract_error)?,
            Value::Array(values) => {
                for value in values {
                    self.string_values(value, depth + 1, source, constraint)?;
                }
            }
            Value::Object(fields) => {
                for value in fields.values() {
                    self.string_values(value, depth + 1, source, constraint)?;
                }
            }
            _ => {}
        }
        Ok(())
    }
}
