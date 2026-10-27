//! Private logical-field transaction; callers supply detector spans and final validation.
//! No HTTP authorization or public action is enabled by this module.
use super::{
    payload_budget::PayloadBudget,
    payload_metadata::PayloadVec,
    payload_text::PayloadText,
    request_plan::{
        plan_request_with_budget, EffectiveAction, PlanError, PlanField, PlannedField, PlannedSpan,
    },
    tokenization::{visit_token_references, DlpRequestLease, TokenError, TokenFragment},
};

const MAX_ITEMS: usize = 4096;
const MAX_FIELD_BYTES: usize = 1024 * 1024;
const MAX_TEXT_BYTES: usize = 4 * 1024 * 1024;

/// Candidate text and proof strings are memory-only and must not be diagnosed.
pub(super) struct PreparedFields<T> {
    pub fields: PayloadVec<PayloadText>,
    pub validated: T,
    pub reversible_occurrences: usize,
    pub irreversible_occurrences: usize,
}

pub(super) fn prepare_fields<T>(
    lease: &mut DlpRequestLease,
    fields: &[PlanField<'_>],
    historical_tokens: &[&str],
    validate: impl FnMut(&[PayloadText], &[&str]) -> Result<T, TokenError>,
) -> Result<PreparedFields<T>, PlanError> {
    prepare_fields_with_budget(
        lease,
        fields,
        historical_tokens,
        PayloadBudget::default(),
        validate,
    )
}

/// Final rebuilding, serialization and residual inspection belong in `validate`.
/// It may run repeatedly and must not perform externally visible side effects.
pub(super) fn prepare_fields_with_budget<T>(
    lease: &mut DlpRequestLease,
    fields: &[PlanField<'_>],
    historical_tokens: &[&str],
    budget: PayloadBudget,
    mut validate: impl FnMut(&[PayloadText], &[&str]) -> Result<T, TokenError>,
) -> Result<PreparedFields<T>, PlanError> {
    if fields.len() > MAX_ITEMS || historical_tokens.len() > MAX_ITEMS {
        return Err(TokenError::CapacityExceeded.into());
    }
    let mut input_bytes = 0usize;
    let mut match_count = 0usize;
    for field in fields {
        input_bytes = bounded_sum(input_bytes, field.text.len(), MAX_TEXT_BYTES)?;
        match_count = bounded_sum(match_count, field.matches.len(), MAX_ITEMS)?;
        if field.text.len() > MAX_FIELD_BYTES {
            return Err(TokenError::CapacityExceeded.into());
        }
    }
    let mut proven = PayloadVec::new(budget.clone())?;
    for &token in historical_tokens {
        // Namespace membership alone is insufficient: resolve checks lease authority.
        lease.resolve(token)?;
        push_bounded(&mut proven, token)?;
    }
    let mut literals = PayloadVec::new(budget.clone())?;
    let mut reference_count = 0usize;
    for field in fields {
        visit_token_references(field.text, |range| {
            reference_count = bounded_sum(reference_count, 1, MAX_ITEMS)?;
            let token = &field.text[range];
            if lease.resolve(token).is_ok() {
                push_bounded(&mut proven, token)?;
            } else {
                push_bounded(&mut literals, token)?;
            }
            Ok(())
        })?;
    }
    sort_unique(&mut proven);
    sort_unique(&mut literals);
    let plan = {
        let mut historical_plaintexts = PayloadVec::new(budget.clone())?;
        for token in proven.as_slice() {
            historical_plaintexts.push(lease.resolve(token)?)?;
        }
        // Borrowed plaintext cannot outlive this block or the lease's authority.
        plan_request_with_budget(fields, historical_plaintexts.as_slice(), budget.clone())?
    };
    let mut fragments = PayloadVec::new(budget.clone())?;
    let mut irreversible_occurrences = 0usize;
    for (field, planned) in fields.iter().zip(plan.as_slice()) {
        for span in planned.spans.as_slice() {
            if span.action == EffectiveAction::Restore {
                fragments.push(TokenFragment {
                    raw: &field.text[span.range.clone()],
                    whole_span_categories: std::slice::from_ref(&span.category),
                })?;
            } else {
                irreversible_occurrences += 1;
            }
        }
    }
    let reversible_occurrences = fragments.len();
    lease
        .prepare_with_budget(
            fragments.as_slice(),
            literals.as_slice(),
            budget.clone(),
            |tokens| {
                // Collision detection runs first, but unknown references never escape
                // merely because a replacement would remove their original carrier.
                if !literals.as_slice().is_empty() {
                    return Err(TokenError::ContextUnavailable);
                }
                let candidate = build_candidate(fields, plan.as_slice(), tokens, &budget)?;
                let mut proof = PayloadVec::new(budget.clone())?;
                for token in proven
                    .as_slice()
                    .iter()
                    .copied()
                    .chain(tokens.iter().map(PayloadText::as_str))
                {
                    proof.push(token)?;
                }
                sort_unique(&mut proof);
                if proof.len() > MAX_ITEMS {
                    return Err(TokenError::CapacityExceeded);
                }
                for text in candidate.as_slice() {
                    visit_token_references(text.as_str(), |range| {
                        if proof
                            .as_slice()
                            .binary_search(&&text.as_str()[range])
                            .is_err()
                        {
                            return Err(TokenError::ContextUnavailable);
                        }
                        Ok(())
                    })?;
                }
                let validated = validate(candidate.as_slice(), proof.as_slice())?;
                Ok(PreparedFields {
                    fields: candidate,
                    validated,
                    reversible_occurrences,
                    irreversible_occurrences,
                })
            },
        )
        .map_err(PlanError::from)
}

fn bounded_sum(total: usize, added: usize, maximum: usize) -> Result<usize, TokenError> {
    total
        .checked_add(added)
        .filter(|sum| *sum <= maximum)
        .ok_or(TokenError::CapacityExceeded)
}

fn push_bounded<'a>(items: &mut PayloadVec<&'a str>, value: &'a str) -> Result<(), TokenError> {
    if items.len() >= MAX_ITEMS {
        return Err(TokenError::CapacityExceeded);
    }
    items.push(value)
}

fn sort_unique(items: &mut PayloadVec<&str>) {
    items.sort_unstable_by(|left, right| left.cmp(right));
    items.dedup_by(|left, right| left == right);
}

fn replacement<'a>(
    span: &PlannedSpan,
    tokens: &'a [PayloadText],
    index: &mut usize,
) -> Result<&'a str, TokenError> {
    if span.action == EffectiveAction::Restore {
        let token = tokens.get(*index).ok_or(TokenError::RedactionFailed)?;
        *index += 1;
        Ok(token.as_str())
    } else if span.private_key_placeholder {
        Ok("[REDACTED_PRIVATE_KEY]")
    } else {
        Ok("[REDACTED]")
    }
}

fn build_candidate(
    fields: &[PlanField<'_>],
    plan: &[PlannedField],
    tokens: &[PayloadText],
    budget: &PayloadBudget,
) -> Result<PayloadVec<PayloadText>, TokenError> {
    let mut result = PayloadVec::new(budget.clone())?;
    let mut token_index = 0usize;
    let mut total_bytes = 0usize;
    for (field, planned) in fields.iter().zip(plan) {
        let mut length = field.text.len();
        let first_token = token_index;
        for span in planned.spans.as_slice() {
            let replacement = replacement(span, tokens, &mut token_index)?;
            length = length
                .checked_sub(span.range.len())
                .and_then(|bytes| bytes.checked_add(replacement.len()))
                .ok_or(TokenError::CapacityExceeded)?;
        }
        if length > MAX_FIELD_BYTES {
            return Err(TokenError::CapacityExceeded);
        }
        total_bytes = bounded_sum(total_bytes, length, MAX_TEXT_BYTES)?;
        let mut output = PayloadText::with_capacity(length, budget.clone(), MAX_FIELD_BYTES)?;
        let mut build_index = first_token;
        let mut cursor = 0;
        for span in planned.spans.as_slice() {
            output.append(&field.text[cursor..span.range.start])?;
            output.append(replacement(span, tokens, &mut build_index)?)?;
            cursor = span.range.end;
        }
        output.append(&field.text[cursor..])?;
        result.push(output)?;
    }
    if token_index != tokens.len() {
        return Err(TokenError::RedactionFailed);
    }
    Ok(result)
}
