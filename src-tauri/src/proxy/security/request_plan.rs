//! Private action arbitration on decoded logical text, before issuing references.
use std::ops::Range;

use super::{
    payload_budget::PayloadBudget,
    payload_metadata::PayloadVec,
    tokenization::{TokenCategory, TokenError},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum EffectiveAction {
    Allow,
    Audit,
    Redact,
    Restore,
    Block,
}

pub(super) struct SpanMatch {
    pub range: Range<usize>,
    pub action: EffectiveAction,
    pub category: TokenCategory,
}

pub(super) struct PlanField<'a> {
    pub text: &'a str,
    pub writable: bool,
    pub matches: &'a [SpanMatch],
}

pub(super) struct PlannedSpan {
    pub range: Range<usize>,
    pub action: EffectiveAction,
    pub category: TokenCategory,
    pub private_key_placeholder: bool,
}

pub(super) struct PlannedField {
    pub spans: PayloadVec<PlannedSpan>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum PlanError {
    Token(TokenError),
    Blocked,
    PolicyConflict,
    Unsupported,
}

impl From<TokenError> for PlanError {
    fn from(value: TokenError) -> Self {
        Self::Token(value)
    }
}

pub(super) fn plan_request(
    fields: &[PlanField<'_>],
    historical_plaintexts: &[&str],
) -> Result<PayloadVec<PlannedField>, PlanError> {
    plan_request_with_budget(fields, historical_plaintexts, PayloadBudget::default())
}

pub(super) fn plan_request_with_budget(
    fields: &[PlanField<'_>],
    historical_plaintexts: &[&str],
    budget: PayloadBudget,
) -> Result<PayloadVec<PlannedField>, PlanError> {
    const MAX_ITEMS: usize = 4096;
    const MAX_FRAGMENT_BYTES: usize = 1024 * 1024;
    const MAX_COMPARISON_BYTES: usize = 64 * 1024 * 1024;
    let match_count = fields.iter().try_fold(0usize, |sum, field| {
        sum.checked_add(field.matches.len())
            .ok_or(TokenError::CapacityExceeded)
    })?;
    if fields.len() > MAX_ITEMS
        || match_count > MAX_ITEMS
        || historical_plaintexts.len() > MAX_ITEMS
    {
        return Err(TokenError::CapacityExceeded.into());
    }
    for field in fields {
        if field.text.len() > MAX_FRAGMENT_BYTES {
            return Err(TokenError::CapacityExceeded.into());
        }
        for found in field.matches {
            if found.range.start >= found.range.end || field.text.get(found.range.clone()).is_none()
            {
                return Err(TokenError::RedactionFailed.into());
            }
        }
    }
    // Decide an explicit protective block before rejecting any writable carrier.
    if fields
        .iter()
        .flat_map(|field| field.matches)
        .any(|found| found.action == EffectiveAction::Block)
    {
        return Err(PlanError::Blocked);
    }
    let mut plan = PayloadVec::new(budget.clone())?;
    for field in fields {
        let mut ordered = PayloadVec::new(budget.clone())?;
        for found in field.matches {
            if matches!(
                found.action,
                EffectiveAction::Redact | EffectiveAction::Restore
            ) {
                ordered.push(found)?;
            }
        }
        if !field.writable && !ordered.as_slice().is_empty() {
            return Err(PlanError::Unsupported);
        }
        ordered.sort_unstable_by(|left, right| {
            (left.range.start, left.range.end).cmp(&(right.range.start, right.range.end))
        });
        let mut spans = PayloadVec::new(budget.clone())?;
        let mut start = 0;
        while start < ordered.len() {
            let mut end = start + 1;
            let mut merged = ordered.as_slice()[start].range.clone();
            while end < ordered.len() && ordered.as_slice()[end].range.start < merged.end {
                merged.end = merged.end.max(ordered.as_slice()[end].range.end);
                end += 1;
            }
            let component = &ordered.as_slice()[start..end];
            let action = if component
                .iter()
                .any(|found| found.action == EffectiveAction::Redact)
            {
                EffectiveAction::Redact
            } else {
                EffectiveAction::Restore
            };
            // Fold the same precedence without allocating a category container.
            let mut category = TokenCategory::Mixed;
            for found in component.iter().filter(|found| found.range == merged) {
                category = super::tokenization::select_category(&[category, found.category]);
            }
            spans.push(PlannedSpan {
                range: merged,
                action,
                category,
                private_key_placeholder: component
                    .iter()
                    .any(|found| found.category == TokenCategory::PrivateKey),
            })?;
            start = end;
        }
        drop(ordered);
        plan.push(PlannedField { spans })?;
    }
    // Keep original irreversible ranges as well as whole merged components:
    // overlap merging must not erase a smaller plaintext's no-restoration rule.
    // Indices and ranges borrow sensitive text without making plaintext copies.
    let mut positions = PayloadVec::new(budget.clone())?;
    let mut queue = PayloadVec::new(budget)?;
    for (field_index, field) in fields.iter().enumerate() {
        for found in field.matches {
            if found.action == EffectiveAction::Redact {
                queue.push((field_index, found.range.clone()))?;
            }
        }
    }
    for (field_index, field) in plan.as_slice().iter().enumerate() {
        for (span_index, span) in field.spans.as_slice().iter().enumerate() {
            positions.push((field_index, span_index))?;
            if span.action == EffectiveAction::Redact {
                queue.push((field_index, span.range.clone()))?;
            }
        }
    }
    queue.sort_unstable_by(|(lf, lr), (rf, rr)| {
        (*lf, lr.start, lr.end).cmp(&(*rf, rr.start, rr.end))
    });
    queue.dedup_by(|left, right| left == right);
    let mut remaining_work = MAX_COMPARISON_BYTES;
    let mut cursor = 0;
    while cursor < queue.len() {
        let (field_index, range) = &queue.as_slice()[cursor];
        let irreversible = &fields[*field_index].text[range.clone()];
        for historical in historical_plaintexts {
            if historical.is_empty() || historical.len() > MAX_FRAGMENT_BYTES {
                return Err(TokenError::ContextUnavailable.into());
            }
            if related(irreversible, historical, &mut remaining_work)? {
                return Err(PlanError::PolicyConflict);
            }
        }
        for &(candidate_field, candidate_span) in positions.as_slice() {
            let candidate = &plan.as_slice()[candidate_field].spans.as_slice()[candidate_span];
            if candidate.action != EffectiveAction::Restore {
                continue;
            }
            let raw = &fields[candidate_field].text[candidate.range.clone()];
            if related(irreversible, raw, &mut remaining_work)? {
                let range = candidate.range.clone();
                plan.get_mut(candidate_field)
                    .ok_or(TokenError::RedactionFailed)?
                    .spans
                    .get_mut(candidate_span)
                    .ok_or(TokenError::RedactionFailed)?
                    .action = EffectiveAction::Redact;
                queue.push((candidate_field, range))?;
            }
        }
        cursor += 1;
    }
    Ok(plan)
}

fn related(left: &str, right: &str, remaining_work: &mut usize) -> Result<bool, TokenError> {
    let charge = left
        .len()
        .checked_add(right.len())
        .and_then(|bytes| bytes.checked_mul(2))
        .ok_or(TokenError::CapacityExceeded)?;
    *remaining_work = remaining_work
        .checked_sub(charge)
        .ok_or(TokenError::CapacityExceeded)?;
    Ok(left.contains(right) || right.contains(left))
}
