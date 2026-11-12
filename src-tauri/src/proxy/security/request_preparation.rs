//! Private request preparation; not an HTTP entry point or public action surface.
//! Candidate JSON and serialized bytes own shared reservations through final use.
use std::io::{self, Write};

use serde_json::Value;

use super::{
    history_provenance::{visit_request_references, DlpHistoryProjection, HistoryLeafPolicy},
    payload_budget::PayloadBudget,
    payload_buffer::PayloadBuffer,
    payload_json::{JsonStringReplacement, PayloadJson},
    payload_metadata::PayloadVec,
    payload_text::PayloadText,
    request_carriers::CodexRequestCarriers,
    request_detection::detect_spans_with_budget,
    request_plan::{EffectiveAction, PlanError, PlanField, SpanMatch},
    restoration_contract::{
        ContractError, EditConstraint, HistoryClaim, RequestObservation, TextPatch,
    },
    restoration_core::prepare_fields_with_history,
    runtime::SecurityRuntimeConfig,
    tokenization::{DlpRequestLease, TokenError},
};

const MAX_ITEMS: usize = 4096;
const MAX_ENCODED_BYTES: usize = 64 * 1024 * 1024;

/// Inspection provenance for immutable client input, never restoration or execution authority.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum RequestHistoryClassification {
    FreshReadable,
    ExplicitDependency,
    UnknownPassthrough,
}

/// Preserve the boolean classifier for callers that only need fresh-input provenance.
pub(super) fn is_fresh_readable_history(
    body: &Value,
    budget: &PayloadBudget,
) -> Result<bool, TokenError> {
    Ok(matches!(
        classify_request_history(body, budget, None)?,
        RequestHistoryClassification::FreshReadable
    ))
}

/// Check readable input for per-call selection; the cursor is resumed separately.
pub(super) fn is_fresh_readable_history_for_call_selection(
    body: &Value,
    budget: &PayloadBudget,
) -> Result<bool, TokenError> {
    Ok(matches!(
        classify_request_history_impl(body, budget, None, false)?,
        RequestHistoryClassification::FreshReadable
    ))
}

#[cfg(test)]
pub(super) fn is_fresh_readable_history_with_history(
    body: &Value,
    budget: &PayloadBudget,
    projection: Option<&DlpHistoryProjection<'_>>,
) -> Result<bool, TokenError> {
    Ok(matches!(
        classify_request_history(body, budget, projection)?,
        RequestHistoryClassification::FreshReadable
    ))
}

/// Unknown syntax has no recovery obligations; explicit dependencies remain strict.
pub(super) fn classify_request_history(
    body: &Value,
    budget: &PayloadBudget,
    projection: Option<&DlpHistoryProjection<'_>>,
) -> Result<RequestHistoryClassification, TokenError> {
    classify_request_history_impl(body, budget, projection, true)
}

fn classify_request_history_impl(
    body: &Value,
    budget: &PayloadBudget,
    projection: Option<&DlpHistoryProjection<'_>>,
    previous_response_id_is_dependency: bool,
) -> Result<RequestHistoryClassification, TokenError> {
    use super::completed_context::valid_external_id;
    fn encrypted_dependency(value: &Value, depth: usize) -> Result<bool, TokenError> {
        if depth > 128 {
            return Err(TokenError::CapacityExceeded);
        }
        match value {
            Value::Array(items) => {
                for item in items {
                    if encrypted_dependency(item, depth + 1)? {
                        return Ok(true);
                    }
                }
                Ok(false)
            }
            Value::Object(fields) => {
                for (key, value) in fields {
                    if matches!(key.as_str(), "encrypted_content" | "encrypted_reasoning")
                        && !value.is_null()
                    {
                        return Ok(true);
                    }
                    if encrypted_dependency(value, depth + 1)? {
                        return Ok(true);
                    }
                }
                Ok(false)
            }
            _ => Ok(false),
        }
    }
    fn prior_dependency(
        value: &Value,
        projection: Option<&DlpHistoryProjection<'_>>,
    ) -> Result<bool, TokenError> {
        // Syntax alone is not a dependency or execution proof.
        visit_request_references(value, projection, |_| Ok(()))?;
        encrypted_dependency(value, 0)
    }
    fn opaque_input_image(part: &Value) -> bool {
        fn valid_detail(value: Option<&Value>) -> bool {
            value.is_none()
                || matches!(value.and_then(Value::as_str), Some("auto" | "low" | "high"))
        }
        let Some(fields) = part.as_object() else {
            return false;
        };
        if fields
            .keys()
            .any(|key| !matches!(key.as_str(), "type" | "image_url" | "detail"))
            || !valid_detail(part.get("detail"))
        {
            return false;
        }
        match part.get("image_url") {
            Some(Value::String(url)) => !url.is_empty(),
            Some(Value::Object(image)) => {
                image
                    .keys()
                    .all(|key| matches!(key.as_str(), "url" | "detail"))
                    && image
                        .get("url")
                        .and_then(Value::as_str)
                        .is_some_and(|url| !url.is_empty())
                    && valid_detail(image.get("detail"))
            }
            _ => false,
        }
    }
    fn readable_parts(
        value: &Value,
        kinds: &[&str],
        allow_refusal: bool,
        allow_image: bool,
    ) -> Result<(), TokenError> {
        let parts = value.as_array().ok_or(TokenError::ContextUnavailable)?;
        if parts.len() > MAX_ITEMS {
            return Err(TokenError::CapacityExceeded);
        }
        for part in parts {
            let kind = part
                .get("type")
                .and_then(Value::as_str)
                .ok_or(TokenError::ContextUnavailable)?;
            // Only user-message image envelopes are opaque; dependencies were checked above.
            if allow_image && kind == "input_image" && opaque_input_image(part) {
                continue;
            }
            let field = if kinds.contains(&kind) {
                "text"
            } else if allow_refusal && kind == "refusal" {
                "refusal"
            } else {
                return Err(TokenError::ContextUnavailable);
            };
            if !part.get(field).is_some_and(Value::is_string) {
                return Err(TokenError::ContextUnavailable);
            }
        }
        Ok(())
    }
    fn readable_text(value: &Value, allow_image: bool) -> Result<(), TokenError> {
        if value.is_string() {
            return Ok(());
        }
        readable_parts(
            value,
            &["text", "input_text", "output_text"],
            true,
            allow_image,
        )
    }
    if (previous_response_id_is_dependency
        && body
            .get("previous_response_id")
            .is_some_and(|v| !v.is_null()))
        || prior_dependency(body, projection)?
    {
        return Ok(RequestHistoryClassification::ExplicitDependency);
    }
    let Some(items) = body.get("input").and_then(Value::as_array) else {
        return Ok(RequestHistoryClassification::UnknownPassthrough);
    };
    if items.len() > MAX_ITEMS {
        return Err(TokenError::CapacityExceeded);
    }
    let mut calls = PayloadVec::new(budget.clone())?;
    let mut outputs = PayloadVec::new(budget.clone())?;
    let has_calls = items.iter().any(|item| {
        matches!(
            item.get("type").and_then(Value::as_str),
            Some("function_call" | "custom_tool_call")
        )
    });
    if !has_calls {
        return Ok(RequestHistoryClassification::UnknownPassthrough);
    }
    // Scan all items before discarding unknown syntax so a live recorded call or
    // plaintext call reasoning cannot lose its completion obligation.
    let has_plaintext_call_reasoning = items.iter().any(|item| {
        matches!(
            item.get("type").and_then(Value::as_str),
            Some("function_call" | "custom_tool_call")
        ) && item.get("reasoning_content").is_some_and(Value::is_string)
    });
    let has_recorded_call = items
        .iter()
        .any(|item| projection.is_some_and(|p| p.has_recorded_call_identity(item)));
    let readable = (|| -> Result<(), TokenError> {
        for item in items {
            let kind = match item.get("type") {
                None => None,
                Some(Value::String(kind)) => Some(kind.as_str()),
                Some(_) => return Err(TokenError::ContextUnavailable),
            };
            match kind {
                Some("function_call" | "custom_tool_call") => {
                    let id = item
                        .get("call_id")
                        .and_then(Value::as_str)
                        .filter(|s| valid_external_id(s))
                        .ok_or(TokenError::ContextUnavailable)?;
                    if calls.as_slice().iter().any(|(known, _)| *known == id) {
                        return Err(TokenError::ContextUnavailable);
                    }
                    if !item
                        .get("name")
                        .and_then(Value::as_str)
                        .is_some_and(valid_external_id)
                    {
                        return Err(TokenError::ContextUnavailable);
                    }
                    let payload = item
                        .get(if kind == Some("function_call") {
                            "arguments"
                        } else {
                            "input"
                        })
                        .and_then(Value::as_str)
                        .ok_or(TokenError::ContextUnavailable)?;
                    if kind == Some("function_call") {
                        let parsed =
                            PayloadJson::try_from_str(payload, budget.clone(), budget.limit())?;
                        // Decoded arguments are tool data, not protocol dependency envelopes.
                        visit_request_references(parsed.as_value(), None, |_| Ok(()))?;
                    }
                    if item
                        .get("reasoning_content")
                        .is_some_and(|v| !v.is_null() && !v.is_string())
                    {
                        return Err(TokenError::ContextUnavailable);
                    }
                    calls.push((id, kind == Some("custom_tool_call")))?;
                }
                Some("function_call_output" | "custom_tool_call_output") => {
                    if crate::proxy::providers::codex_chat_common::is_unsolicited_page_context(item)
                    {
                        readable_text(
                            item.get("output").ok_or(TokenError::ContextUnavailable)?,
                            false,
                        )?;
                        continue;
                    }
                    let id = item
                        .get("call_id")
                        .and_then(Value::as_str)
                        .ok_or(TokenError::ContextUnavailable)?;
                    let custom = kind == Some("custom_tool_call_output");
                    if !calls.as_slice().contains(&(id, custom)) || outputs.as_slice().contains(&id)
                    {
                        return Err(TokenError::ContextUnavailable);
                    }
                    readable_text(
                        item.get("output").ok_or(TokenError::ContextUnavailable)?,
                        false,
                    )?;
                    outputs.push(id)?;
                }
                Some("reasoning") => {
                    let summary = item.get("summary").filter(|v| !v.is_null());
                    let content = item.get("content").filter(|v| !v.is_null());
                    if summary.is_none() && content.is_none() {
                        return Err(TokenError::ContextUnavailable);
                    }
                    if let Some(summary) = summary {
                        readable_parts(summary, &["summary_text", "reasoning_text"], false, false)?;
                    }
                    if let Some(content) = content {
                        readable_parts(content, &["reasoning_text"], false, false)?;
                    }
                }
                Some("message") | None
                    if matches!(
                        item.get("role").and_then(Value::as_str),
                        Some("user" | "assistant" | "system" | "developer")
                    ) =>
                {
                    readable_text(
                        item.get("content").ok_or(TokenError::ContextUnavailable)?,
                        item.get("role").and_then(Value::as_str) == Some("user"),
                    )?;
                }
                _ => return Err(TokenError::ContextUnavailable),
            }
        }
        if calls.as_slice().len() != outputs.as_slice().len() {
            return Err(TokenError::ContextUnavailable);
        }
        Ok(())
    })();
    match readable {
        Ok(()) if has_recorded_call => Ok(RequestHistoryClassification::ExplicitDependency),
        Ok(()) => Ok(RequestHistoryClassification::FreshReadable),
        Err(TokenError::ContextUnavailable)
            if has_recorded_call || has_plaintext_call_reasoning =>
        {
            Ok(RequestHistoryClassification::ExplicitDependency)
        }
        Err(TokenError::ContextUnavailable) => Ok(RequestHistoryClassification::UnknownPassthrough),
        Err(error) => Err(error),
    }
}

#[cfg(test)]
std::thread_local! {
    // Observe copy attempts without recording text or changing allocation behavior.
    pub(super) static COPY_ATTEMPT_MAX_BYTES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

// No Debug/Serialize: candidates and encoded bodies can contain private text.
pub(super) struct DlpPreparedRequest<T> {
    pub body: PayloadJson,
    pub encoded: PayloadBuffer,
    pub validated: T,
    pub reversible_occurrences: usize,
    pub irreversible_occurrences: usize,
}

/// Prepare exact native sites without granting route or history authority.
/// Rebuilding, residual inspection, encoding and validation all precede token commit.
pub(super) fn prepare_observed_request<T>(
    lease: &mut DlpRequestLease,
    observation: &RequestObservation<'_>,
    runtime: &SecurityRuntimeConfig,
    restore_rule_ids: &[&str],
    historical_tokens: &[&str],
    mut validate: impl FnMut(&Value, &[u8], &[&str]) -> Result<T, TokenError>,
) -> Result<DlpPreparedRequest<T>, PlanError> {
    lease.require_native_request_scope(observation.client())?;
    if restore_rule_ids.len() > MAX_ITEMS || historical_tokens.len() > MAX_ITEMS {
        return Err(TokenError::CapacityExceeded.into());
    }
    let budget = observation.budget();
    let native = observation.native();
    let sites = observation.sites();
    let mut remaining_work = MAX_NATIVE_SITE_WORK;
    let mut require_member = |node: &Value| -> Result<(), TokenError> {
        native_site_candidate(native, node, native, 0, &mut remaining_work)?
            .map(|_| ())
            .ok_or(TokenError::RedactionFailed)
    };
    for index in 0..sites.len() {
        require_member(
            sites
                .site(index)
                .ok_or(TokenError::RedactionFailed)?
                .native(),
        )?;
    }
    // Claims validate ownership only; pairing and source classes confer no proof.
    for claim in observation.history() {
        match claim {
            HistoryClaim::CallResultPair { call, result } => {
                require_member(call)?;
                require_member(result)?;
            }
            HistoryClaim::AssistantItem { native }
            | HistoryClaim::OpaqueItem { native, .. }
            | HistoryClaim::ResponsesCursor { native } => require_member(native)?,
        }
    }
    let mut logical = PayloadVec::new(budget.clone())?;
    let mut match_count = 0usize;
    let mut protective_block = false;
    let mut immutable_reference = false;
    for index in 0..sites.len() {
        let site = sites.site(index).ok_or(TokenError::RedactionFailed)?;
        let text = site.text();
        let writable = site.constraint() == EditConstraint::Editable;
        let mut references = PayloadVec::new(budget.clone())?;
        HistoryLeafPolicy::UnprovenText.visit_references(text, |range| {
            if lease.resolve_if_authorized(&text[range.clone()])?.is_some() {
                immutable_reference |= !writable;
                references.push(range)?;
            }
            Ok(())
        })?;
        let mut matches = PayloadVec::new(budget.clone())?;
        for found in detect_spans_with_budget(text, false, runtime, budget.clone())?.as_slice() {
            if !found.range.is_empty()
                && references
                    .as_slice()
                    .iter()
                    .any(|range| range.start <= found.range.start && found.range.end <= range.end)
            {
                continue;
            }
            match_count = match_count
                .checked_add(1)
                .filter(|count| *count <= MAX_ITEMS)
                .ok_or(TokenError::CapacityExceeded)?;
            protective_block |= found.action == EffectiveAction::Block;
            if !found.range.is_empty() {
                matches.push(SpanMatch {
                    range: found.range.clone(),
                    action: selected_action(found.rule_id, found.action, restore_rule_ids),
                    category: found.category,
                })?;
            }
        }
        logical.push(LogicalField {
            text,
            writable,
            policy: HistoryLeafPolicy::UnprovenText,
            matches,
        })?;
    }
    if protective_block {
        return Err(PlanError::Blocked);
    }
    if immutable_reference {
        // Restoring here would have to edit immutable signed content.
        return Err(PlanError::ImmutableContent);
    }
    // Retain already pinned dependencies, but never authorize caller-supplied tokens.
    let mut dependencies = PayloadVec::new(budget.clone())?;
    let mut snapshot = |reference: &str| -> Result<(), TokenError> {
        lease.resolve(reference)?;
        if dependencies
            .as_slice()
            .iter()
            .any(|known: &PayloadText| known.as_str() == reference)
        {
            return Ok(());
        }
        if dependencies.len() >= MAX_ITEMS {
            return Err(TokenError::CapacityExceeded);
        }
        dependencies.push(copy_text(reference, budget.clone())?)
    };
    lease.visit_authorized_tokens(&mut snapshot)?;
    for &reference in historical_tokens {
        snapshot(reference)?;
    }
    let mut history = PayloadVec::new(budget.clone())?;
    for token in dependencies.as_slice() {
        history.push(token.as_str())?;
    }
    let mut fields = PayloadVec::new(budget.clone())?;
    for field in logical.as_slice() {
        fields.push(PlanField {
            text: field.text,
            writable: field.writable,
            matches: field.matches.as_slice(),
        })?;
    }
    // The validator callback can only report TokenError, so an immutable-site conflict
    // is recorded out of band and re-raised with its exact cause afterwards.
    let immutable_conflict = std::cell::Cell::new(false);
    let prepared = prepare_fields_with_history(
        lease,
        fields.as_slice(),
        history.as_slice(),
        budget.clone(),
        &[],
        |texts, proof| {
            if texts.len() != sites.len() {
                return Err(TokenError::RedactionFailed);
            }
            let mut patches = PayloadVec::new(budget.clone())?;
            for (index, text) in texts.iter().enumerate() {
                let site = sites.site(index).ok_or(TokenError::RedactionFailed)?;
                if text.as_str() != site.text() {
                    patches.push(TextPatch::new(site, 0..site.text().len(), text.as_str()))?;
                }
            }
            let plan = match sites.plan(patches.as_slice()) {
                Ok(plan) => plan,
                Err(ContractError::ImmutableSite) => {
                    immutable_conflict.set(true);
                    return Err(TokenError::RedactionFailed);
                }
                Err(ContractError::Resource(error)) => return Err(error),
                Err(_) => return Err(TokenError::RedactionFailed),
            };
            let mut replacements = PayloadVec::new(budget.clone())?;
            for edit in plan.edits() {
                replacements.push(JsonStringReplacement {
                    target: edit.native(),
                    text: edit.text(),
                })?;
            }
            let candidate = PayloadJson::try_with_replacements(
                native,
                replacements.as_slice(),
                budget.clone(),
                MAX_ENCODED_BYTES,
            )?;
            let mut remaining_work = MAX_NATIVE_SITE_WORK;
            for index in 0..sites.len() {
                let site = sites.site(index).ok_or(TokenError::RedactionFailed)?;
                let text = native_site_candidate(
                    native,
                    site.native(),
                    candidate.as_value(),
                    0,
                    &mut remaining_work,
                )?
                .and_then(Value::as_str)
                .ok_or(TokenError::RedactionFailed)?;
                let mut references = PayloadVec::new(budget.clone())?;
                HistoryLeafPolicy::UnprovenText.visit_references(text, |range| {
                    if proof.contains(&&text[range.clone()]) {
                        references.push(range)?;
                    }
                    Ok(())
                })?;
                for found in
                    detect_spans_with_budget(text, false, runtime, budget.clone())?.as_slice()
                {
                    if matches!(
                        found.action,
                        EffectiveAction::Redact | EffectiveAction::Restore | EffectiveAction::Block
                    ) && (found.range.is_empty()
                        || !references.as_slice().iter().any(|range| {
                            range.start <= found.range.start && found.range.end <= range.end
                        }))
                    {
                        return Err(TokenError::RedactionFailed);
                    }
                }
            }
            let encoded = encode_candidate(candidate.as_value(), budget.clone())?;
            let validated = validate(candidate.as_value(), encoded.as_slice(), proof)?;
            Ok((candidate, encoded, validated))
        },
    );
    let prepared = match prepared {
        Ok(prepared) => prepared,
        // Preserve the exact cause when the conflict was an immutable signed site.
        Err(_) if immutable_conflict.get() => return Err(PlanError::ImmutableContent),
        Err(error) => return Err(error),
    };
    let (body, encoded, validated) = prepared.validated;
    Ok(DlpPreparedRequest {
        body,
        encoded,
        validated,
        reversible_occurrences: prepared.reversible_occurrences,
        irreversible_occurrences: prepared.irreversible_occurrences,
    })
}

// Bound identity searches without allocating a diagnostic path or native-value index.
const MAX_NATIVE_SITE_WORK: usize = 4 * 1024 * 1024;
fn native_site_candidate<'candidate>(
    source: &Value,
    target: &Value,
    candidate: &'candidate Value,
    depth: usize,
    remaining_work: &mut usize,
) -> Result<Option<&'candidate Value>, TokenError> {
    if depth > 64 {
        return Err(TokenError::CapacityExceeded);
    }
    *remaining_work = remaining_work
        .checked_sub(1)
        .ok_or(TokenError::CapacityExceeded)?;
    if std::ptr::eq(source, target) {
        return Ok(Some(candidate));
    }
    match source {
        Value::Array(items) => {
            let projected = candidate.as_array().ok_or(TokenError::RedactionFailed)?;
            if projected.len() != items.len() {
                return Err(TokenError::RedactionFailed);
            }
            for (item, projected) in items.iter().zip(projected) {
                if let Some(found) =
                    native_site_candidate(item, target, projected, depth + 1, remaining_work)?
                {
                    return Ok(Some(found));
                }
            }
        }
        Value::Object(fields) => {
            let projected = candidate.as_object().ok_or(TokenError::RedactionFailed)?;
            if projected.len() != fields.len() {
                return Err(TokenError::RedactionFailed);
            }
            for (key, value) in fields {
                let projected = projected.get(key).ok_or(TokenError::RedactionFailed)?;
                if let Some(found) =
                    native_site_candidate(value, target, projected, depth + 1, remaining_work)?
                {
                    return Ok(Some(found));
                }
            }
        }
        _ => {}
    }
    Ok(None)
}

struct LogicalField<'a> {
    text: &'a str,
    writable: bool,
    policy: HistoryLeafPolicy,
    matches: PayloadVec<SpanMatch>,
}

/// The private rule selection can promote only effective irreversible redaction.
/// Final validation is repeatable and must have no externally visible side effects.
pub(super) fn prepare_codex_request<T>(
    lease: &mut DlpRequestLease,
    body: &Value,
    runtime: &SecurityRuntimeConfig,
    restore_rule_ids: &[&str],
    historical_tokens: &[&str],
    validate: impl FnMut(&Value, &[u8], &[&str]) -> Result<T, TokenError>,
) -> Result<DlpPreparedRequest<T>, PlanError> {
    prepare_codex_request_with_budget(
        lease,
        body,
        runtime,
        restore_rule_ids,
        historical_tokens,
        PayloadBudget::default(),
        validate,
    )
}

#[allow(clippy::too_many_arguments)]
pub(super) fn prepare_codex_request_with_budget<T>(
    lease: &mut DlpRequestLease,
    body: &Value,
    runtime: &SecurityRuntimeConfig,
    restore_rule_ids: &[&str],
    historical_tokens: &[&str],
    budget: PayloadBudget,
    validate: impl FnMut(&Value, &[u8], &[&str]) -> Result<T, TokenError>,
) -> Result<DlpPreparedRequest<T>, PlanError> {
    prepare_codex_request_with_history(
        lease,
        body,
        runtime,
        restore_rule_ids,
        historical_tokens,
        budget,
        None,
        validate,
    )
}

#[allow(clippy::too_many_arguments)]
pub(super) fn prepare_codex_request_with_history<T>(
    lease: &mut DlpRequestLease,
    body: &Value,
    runtime: &SecurityRuntimeConfig,
    restore_rule_ids: &[&str],
    historical_tokens: &[&str],
    budget: PayloadBudget,
    projection: Option<&DlpHistoryProjection<'_>>,
    mut validate: impl FnMut(&Value, &[u8], &[&str]) -> Result<T, TokenError>,
) -> Result<DlpPreparedRequest<T>, PlanError> {
    lease.require_responses_request_scope()?;
    if restore_rule_ids.len() > MAX_ITEMS || historical_tokens.len() > MAX_ITEMS {
        return Err(TokenError::CapacityExceeded.into());
    }
    let carriers = CodexRequestCarriers::from_body_with_budget(body, budget.clone())?;
    carriers.visit_key_references(|reference| {
        if lease.resolve_if_authorized(reference)?.is_some() {
            return Err(TokenError::ContextUnavailable);
        }
        Ok(())
    })?;
    let mut logical = PayloadVec::new(budget.clone())?;
    let mut match_count = 0usize;
    let mut protective_block = false;
    carriers.visit_with_history(projection, |leaf| {
        let mut matches = PayloadVec::new(budget.clone())?;
        if leaf.policy == HistoryLeafPolicy::DeliveredReasoning {
            return logical.push(LogicalField {
                text: leaf.text,
                writable: leaf.writable,
                policy: leaf.policy,
                matches,
            });
        }
        let mut references = PayloadVec::new(budget.clone())?;
        leaf.policy
            .visit_references(leaf.text, |range| references.push(range))?;
        for found in
            detect_spans_with_budget(leaf.text, leaf.is_password_field, runtime, budget.clone())?
                .as_slice()
        {
            // Empty matches carry no token authority, including at token boundaries.
            if !found.range.is_empty()
                && references.as_slice().iter().any(|range| {
                    range.start <= found.range.start
                        && found.range.end <= range.end
                        && lease.resolve(&leaf.text[range.clone()]).is_ok()
                })
            {
                continue;
            }
            match_count = match_count
                .checked_add(1)
                .filter(|count| *count <= MAX_ITEMS)
                .ok_or(TokenError::CapacityExceeded)?;
            if found.range.is_empty() {
                if found.action == EffectiveAction::Block {
                    protective_block = true;
                }
                continue;
            }
            let action = selected_action(found.rule_id, found.action, restore_rule_ids);
            matches.push(SpanMatch {
                range: found.range.clone(),
                action,
                category: found.category,
            })?;
        }
        logical.push(LogicalField {
            text: leaf.text,
            writable: leaf.writable,
            policy: leaf.policy,
            matches,
        })?;
        Ok(())
    })?;
    if protective_block {
        return Err(PlanError::Blocked);
    }
    // A repeated preparation must not silently omit previously pinned dependencies.
    let mut dependencies = PayloadVec::new(budget.clone())?;
    let mut snapshot = |reference: &str| -> Result<(), TokenError> {
        // Reject invalid lengths and missing request authority before any owned copy.
        lease.resolve(reference)?;
        if dependencies
            .as_slice()
            .iter()
            .any(|known: &PayloadText| known.as_str() == reference)
        {
            return Ok(());
        }
        if dependencies.len() >= MAX_ITEMS {
            return Err(TokenError::CapacityExceeded);
        }
        dependencies.push(copy_text(reference, budget.clone())?)
    };
    lease.visit_authorized_tokens(&mut snapshot)?;
    for &reference in historical_tokens {
        snapshot(reference)?;
    }
    let mut history = PayloadVec::new(budget.clone())?;
    for token in dependencies.as_slice() {
        history.push(token.as_str())?;
    }
    let mut fields = PayloadVec::new(budget.clone())?;
    let mut policies = PayloadVec::new(budget.clone())?;
    for field in logical.as_slice() {
        if projection.is_some() {
            policies.push(field.policy)?;
        }
        fields.push(PlanField {
            text: field.text,
            writable: field.writable,
            matches: field.matches.as_slice(),
        })?;
    }
    let prepared = prepare_fields_with_history(
        lease,
        fields.as_slice(),
        history.as_slice(),
        budget.clone(),
        policies.as_slice(),
        |texts, proof| {
            let candidate = carriers.rewrite_candidate(body, |leaf| {
                let text = texts.get(leaf.id).ok_or(TokenError::RedactionFailed)?;
                Ok((text.as_str() != leaf.text).then_some(text.as_str()))
            })?;
            let final_carriers =
                CodexRequestCarriers::from_body_with_budget(candidate.as_value(), budget.clone())?;
            final_carriers.visit_key_references(|reference| {
                if proof.contains(&reference) {
                    return Err(TokenError::ContextUnavailable);
                }
                Ok(())
            })?;
            final_carriers.visit_with_history(projection, |leaf| {
                if leaf.policy == HistoryLeafPolicy::DeliveredReasoning {
                    return Ok(());
                }
                let mut references = PayloadVec::new(budget.clone())?;
                leaf.policy.visit_references(leaf.text, |range| {
                    if !proof.contains(&&leaf.text[range.clone()]) {
                        return Ok(());
                    }
                    references.push(range)
                })?;
                for found in detect_spans_with_budget(
                    leaf.text,
                    leaf.is_password_field,
                    runtime,
                    budget.clone(),
                )?
                .as_slice()
                {
                    if !matches!(
                        found.action,
                        EffectiveAction::Redact | EffectiveAction::Restore | EffectiveAction::Block
                    ) {
                        continue;
                    }
                    // Never exempt zero-width matches, surrounding text, or crossing spans.
                    if found.range.is_empty()
                        || !references.as_slice().iter().any(|range| {
                            range.start <= found.range.start && found.range.end <= range.end
                        })
                    {
                        return Err(TokenError::RedactionFailed);
                    }
                }
                Ok(())
            })?;
            let encoded = encode_candidate(candidate.as_value(), budget.clone())?;
            let validated = validate(candidate.as_value(), encoded.as_slice(), proof)?;
            Ok((candidate, encoded, validated))
        },
    )?;
    let (body, encoded, validated) = prepared.validated;
    Ok(DlpPreparedRequest {
        body,
        encoded,
        validated,
        reversible_occurrences: prepared.reversible_occurrences,
        irreversible_occurrences: prepared.irreversible_occurrences,
    })
}

fn selected_action(id: &str, action: EffectiveAction, selected: &[&str]) -> EffectiveAction {
    if action == EffectiveAction::Redact && selected.contains(&id) {
        EffectiveAction::Restore
    } else {
        action
    }
}

fn copy_text(text: &str, budget: PayloadBudget) -> Result<PayloadText, TokenError> {
    #[cfg(test)]
    COPY_ATTEMPT_MAX_BYTES.with(|largest| largest.set(largest.get().max(text.len())));
    PayloadText::try_from_str(text, budget, super::token_format::MAX_REFERENCE_BYTES)
}

struct SizeWriter(usize);
impl Write for SizeWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0 = self
            .0
            .checked_add(bytes.len())
            .filter(|size| *size <= MAX_ENCODED_BYTES)
            .ok_or_else(|| io::Error::other("request capacity exceeded"))?;
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

struct PayloadWriter {
    encoded: PayloadBuffer,
    error: Option<TokenError>,
}

impl Write for PayloadWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if let Some(error) = self.error {
            return Err(io::Error::other(error.code()));
        }
        // The first pass fixes the capacity. Never grow during serialization.
        let result = self
            .encoded
            .len()
            .checked_add(bytes.len())
            .filter(|required| *required <= self.encoded.capacity())
            .ok_or(TokenError::CapacityExceeded)
            .and_then(|_| self.encoded.append(bytes));
        if let Err(error) = result {
            self.error = Some(error);
            return Err(io::Error::other(error.code()));
        }
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

pub(super) fn encode_candidate(
    body: &Value,
    budget: PayloadBudget,
) -> Result<PayloadBuffer, TokenError> {
    let mut size = SizeWriter(0);
    serde_json::to_writer(&mut size, body).map_err(|_| TokenError::CapacityExceeded)?;
    let mut writer = PayloadWriter {
        encoded: PayloadBuffer::with_capacity(size.0, budget, MAX_ENCODED_BYTES)?,
        error: None,
    };
    if serde_json::to_writer(&mut writer, body).is_err() {
        return Err(writer.error.unwrap_or(TokenError::RedactionFailed));
    }
    if writer.encoded.len() != size.0 {
        return Err(TokenError::RedactionFailed);
    }
    Ok(writer.encoded)
}

#[cfg(test)]
pub(super) fn logical_field_size() -> usize {
    std::mem::size_of::<LogicalField<'_>>()
}
