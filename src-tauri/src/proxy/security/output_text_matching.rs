//! Exact placeholder recognition; syntax alone never grants authority.
//! Nonmatches remain text; a match never grants restoration authority.
use std::ops::Range;

use super::{
    payload_budget::PayloadBudget,
    payload_metadata::PayloadVec,
    payload_text::PayloadText,
    response_restoration::RestoreError,
    restore_diagnostics::RestoreReason,
    tokenization::{DlpRequestLease, TokenError},
};

use super::token_format;

pub(super) fn visit_complete_references(
    text: &str,
    mut visitor: impl FnMut(Range<usize>) -> Result<(), TokenError>,
) -> Result<(), TokenError> {
    let mut offset = 0;
    let mut count = 0usize;
    while let Some((start, prefix_len)) = token_format::next_candidate(text, offset) {
        let bytes = &text.as_bytes()[start..];
        // Advance only the ASCII prefix on a nonmatch, never over nested starts.
        offset = start + prefix_len;
        let Some(layout) = token_format::header(bytes) else {
            continue;
        };
        if !layout.complete(bytes) {
            continue;
        }
        if count >= 4096 {
            return Err(TokenError::CapacityExceeded);
        }
        count += 1;
        offset = start + layout.len;
        visitor(start..offset)?;
    }
    Ok(())
}

/// Retain only a suffix that could still become a complete reference.
/// An invalid earlier candidate never suppresses a later candidate start.
pub(super) fn split_prefix_and_tail(text: &str) -> (&str, &str) {
    for start in text
        .len()
        .saturating_sub(token_format::MAX_REFERENCE_BYTES - 1)..text.len()
    {
        if text.is_char_boundary(start) && token_format::is_prefix(&text.as_bytes()[start..]) {
            return (&text[..start], &text[start..]);
        }
    }
    (text, "")
}

pub(super) fn restore_text(
    text: &str,
    lease: &DlpRequestLease,
    budget: &PayloadBudget,
    output_tokens: &mut PayloadVec<PayloadText>,
    restored_occurrences: &mut usize,
) -> Result<PayloadText, RestoreError> {
    let mut ranges = PayloadVec::new(budget.clone()).map_err(RestoreError::capacity)?;
    visit_complete_references(text, |range| {
        if lease.resolve_if_authorized(&text[range.clone()])?.is_some() {
            ranges.push(range)?;
        }
        Ok(())
    })
    .map_err(RestoreError::scan_error)?;
    let next_occurrences = restored_occurrences
        .checked_add(ranges.len())
        .ok_or(RestoreError::FailedBecause(RestoreReason::RestoreCapacity))?;
    let mut additions = PayloadVec::new(budget.clone()).map_err(RestoreError::capacity)?;
    let mut out_len = text.len();
    for range in ranges.as_slice() {
        let reference = &text[range.clone()];
        let plain = lease
            .resolve(reference)
            .map_err(|_| RestoreError::FailedBecause(RestoreReason::TokenContext))?;
        if !output_tokens
            .as_slice()
            .iter()
            .any(|item| item.as_str() == reference)
            && !additions.as_slice().contains(&reference)
        {
            if output_tokens
                .len()
                .checked_add(additions.len())
                .is_none_or(|len| len >= 4096)
            {
                return Err(RestoreError::FailedBecause(RestoreReason::RestoreCapacity));
            }
            additions.push(reference).map_err(RestoreError::capacity)?;
        }
        out_len = out_len
            .checked_sub(range.len())
            .and_then(|n| n.checked_add(plain.len()))
            .ok_or(RestoreError::FailedBecause(RestoreReason::RestoreCapacity))?;
    }
    let mut out = PayloadText::with_capacity(out_len, budget.clone(), budget.limit())
        .map_err(RestoreError::capacity)?;
    let mut cursor = 0usize;
    for range in ranges.as_slice() {
        out.append(&text[cursor..range.start])
            .map_err(RestoreError::capacity)?;
        let plain = lease
            .resolve(&text[range.clone()])
            .map_err(|_| RestoreError::FailedBecause(RestoreReason::TokenContext))?;
        out.append(plain).map_err(RestoreError::capacity)?;
        cursor = range.end;
    }
    out.append(&text[cursor..])
        .map_err(RestoreError::capacity)?;
    if !additions.as_slice().is_empty() {
        // Stage the entire replacement ledger while its old backing remains charged.
        // Moving it into the caller is the sole commit and cannot allocate or fail.
        let mut staged = PayloadVec::new(budget.clone()).map_err(RestoreError::capacity)?;
        for reference in output_tokens
            .as_slice()
            .iter()
            .map(PayloadText::as_str)
            .chain(additions.as_slice().iter().copied())
        {
            let owned = PayloadText::try_from_str(
                reference,
                budget.clone(),
                token_format::MAX_REFERENCE_BYTES,
            )
            .map_err(RestoreError::capacity)?;
            staged.push(owned).map_err(RestoreError::capacity)?;
        }
        *output_tokens = staged;
    }
    *restored_occurrences = next_occurrences;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reference_format_migration_exact_scanning_and_all_tails() -> Result<(), TokenError> {
        for name in [
            "cn_phone",
            "cn_id_card",
            "private_key",
            "api_token",
            "db_conn",
            "password",
            "infra",
            "personal",
            "custom",
            "mixed",
        ] {
            let reference = format!("rredact-{name}-{}-ai", "0".repeat(20));
            let text = format!("前🔒rredact-custom-bad{reference}尾");
            let mut count = 0;
            visit_complete_references(&text, |range| {
                assert!(
                    &text[range] == reference,
                    "only complete references are visited"
                );
                count += 1;
                Ok(())
            })?;
            assert!(count == 1, "nonmatch must not hide a later reference");
            for split in 1..reference.len() {
                let text = format!("前rredact-custom-bad{}", &reference[..split]);
                let (prefix, tail) = split_prefix_and_tail(&text);
                assert!(
                    tail == &reference[..split],
                    "every viable new suffix must be retained"
                );
                assert!(
                    prefix.len() + tail.len() == text.len(),
                    "splitting preserves all bytes"
                );
            }
            assert!(
                split_prefix_and_tail(&reference).1.is_empty(),
                "complete references are not pending"
            );
        }
        Ok(())
    }

    #[test]
    fn output_text_exact_scanner_reference_limit_and_callback_failures() {
        let reference = format!("rredact-custom-{}-ai", "0".repeat(20));
        let mut count = 0;
        assert!(visit_complete_references(&reference.repeat(4096), |_| {
            count += 1;
            Ok(())
        })
        .is_ok());
        assert_eq!(count, 4096);
        count = 0;
        assert!(matches!(
            visit_complete_references(&reference.repeat(4097), |_| {
                count += 1;
                Ok(())
            }),
            Err(TokenError::CapacityExceeded)
        ));
        assert_eq!(count, 4096);
        assert!(matches!(
            visit_complete_references(&reference, |_| Err(TokenError::CapacityExceeded)),
            Err(TokenError::CapacityExceeded)
        ));
    }

    #[test]
    fn output_text_exact_scanner_nested_utf8_candidates_and_every_prefix() {
        let reference = format!("rredact-custom-{}-ai", "0".repeat(20));
        let header = ["rredact", "-custom-"].concat();
        let text = format!("前🔒{header}{reference}尾{header}**{reference}");
        let mut count = 0;
        visit_complete_references(&text, |range| {
            assert!(
                &text[range] == reference,
                "only exact complete references are visited"
            );
            count += 1;
            Ok(())
        })
        .expect("synthetic scan");
        assert_eq!(count, 2);
        for len in 1..reference.len() {
            let text = format!("前🔒{header}**{}", &reference[..len]);
            let (prefix, tail) = split_prefix_and_tail(&text);
            assert!(
                tail == &reference[..len],
                "a later viable suffix survives an earlier nonmatch"
            );
            assert!(
                prefix.len() + tail.len() == text.len(),
                "splitting never loses bytes"
            );
        }
        assert!(
            split_prefix_and_tail(&reference).1.is_empty(),
            "complete references never remain pending"
        );
    }
}
