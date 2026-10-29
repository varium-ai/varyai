//! Private, bounded detector spans for reversible request planning.
//! This module does not enable a public action or replace legacy inspection.
use std::ops::{Deref, Range};

use super::{
    builtin_rules::{BuiltinRule, BUILTIN_RULES, GLOBAL_AHO_CORASICK},
    payload_budget::PayloadBudget,
    payload_metadata::PayloadVec,
    payload_text::PayloadText,
    pii::{visit_id_card_ranges, visit_phone_ranges},
    request_plan::EffectiveAction,
    runtime::SecurityRuntimeConfig,
    tokenization::{TokenCategory, TokenError},
    types::{AuditAction, RuleCategory},
};

const MAX_TEXT_BYTES: usize = 1024 * 1024;
const MAX_MATCHES: usize = 4096;

// No Debug/Serialize: custom rule identities are private configuration.
pub(super) struct DetectedSpan<'a> {
    pub rule_id: &'a str,
    pub range: Range<usize>,
    pub action: EffectiveAction,
    pub category: TokenCategory,
}

pub(super) fn detect_spans<'a>(
    text: &str,
    is_password_field: bool,
    runtime: &'a SecurityRuntimeConfig,
) -> Result<PayloadVec<DetectedSpan<'a>>, TokenError> {
    detect_spans_with_budget(text, is_password_field, runtime, PayloadBudget::default())
}

pub(super) fn detect_spans_with_budget<'a>(
    text: &str,
    is_password_field: bool,
    runtime: &'a SecurityRuntimeConfig,
    budget: PayloadBudget,
) -> Result<PayloadVec<DetectedSpan<'a>>, TokenError> {
    if text.len() > MAX_TEXT_BYTES {
        return Err(TokenError::CapacityExceeded);
    }
    if !runtime.enabled || text.is_empty() {
        return PayloadVec::new(budget);
    }
    // Bound configured request work before scanning with captured matchers.
    if runtime.custom_rules.len() > MAX_MATCHES {
        return Err(TokenError::CapacityExceeded);
    }
    let mut pattern_bytes = 0usize;
    for rule in runtime.custom_rules.iter().filter(|rule| rule.enabled) {
        pattern_bytes = pattern_bytes
            .checked_add(rule.pattern.len())
            .filter(|bytes| *bytes <= 4 * MAX_TEXT_BYTES)
            .ok_or(TokenError::CapacityExceeded)?;
        if rule.pattern.len() > MAX_TEXT_BYTES {
            return Err(TokenError::CapacityExceeded);
        }
    }
    let (normalized, offsets) = normalize_with_budget(text, budget.clone())?;
    let mut found = PayloadVec::new(budget.clone())?;
    let mut push = |rule_id: &'a str, range: Range<usize>, action: EffectiveAction, category| {
        if found.len() >= MAX_MATCHES {
            return Err(TokenError::CapacityExceeded);
        }
        let original = if let Some(offsets) = &offsets {
            *offsets
                .as_slice()
                .get(range.start)
                .ok_or(TokenError::RedactionFailed)?
                ..*offsets
                    .as_slice()
                    .get(range.end)
                    .ok_or(TokenError::RedactionFailed)?
        } else {
            range
        };
        if text.get(original.clone()).is_none()
            || (original.is_empty()
                && matches!(action, EffectiveAction::Redact | EffectiveAction::Restore))
        {
            return Err(TokenError::RedactionFailed);
        }
        #[cfg(test)]
        test_support::emit(budget.used())?;
        found.push(DetectedSpan {
            rule_id,
            range: original,
            action,
            category,
        })?;
        Ok(())
    };
    let matcher = GLOBAL_AHO_CORASICK
        .as_ref()
        .ok_or(TokenError::RedactionFailed)?;
    if matcher.is_match(normalized.as_ref()) {
        for rule in BUILTIN_RULES
            .iter()
            .filter(|rule| rule.id != "catalog:password-field")
        {
            if !rule
                .trigger_keywords
                .iter()
                .any(|keyword| contains_ascii_case_insensitive(&normalized, keyword))
            {
                continue;
            }
            let regex = rule.regex.as_ref().ok_or(TokenError::RedactionFailed)?;
            for captures in regex.captures_iter(&normalized) {
                let Some(matched) = captures.get(rule.capture_group.unwrap_or(0)) else {
                    continue;
                };
                if placeholder(matched.as_str())
                    || rule.allowlisted_values.contains(&matched.as_str())
                {
                    continue;
                }
                let mut range = matched.range();
                if rule.category == RuleCategory::PrivateKey {
                    range.end = private_key_end(&normalized, range.end, matched.as_str());
                }
                push(
                    rule.id,
                    range,
                    builtin_action(rule, runtime),
                    category(&rule.category),
                )?;
            }
        }
    }
    if is_password_field && !placeholder(&normalized) {
        if let Some(rule) = BUILTIN_RULES
            .iter()
            .find(|rule| rule.id == "catalog:password-field")
        {
            push(
                rule.id,
                0..normalized.len(),
                builtin_action(rule, runtime),
                TokenCategory::Password,
            )?;
        }
    }
    visit_id_card_ranges(&normalized, |range| {
        push(
            "chinese_id_card",
            range,
            action(
                runtime
                    .builtin_action_overrides
                    .get("chinese_id_card")
                    .copied()
                    .unwrap_or(AuditAction::Redact),
            ),
            TokenCategory::IdCard,
        )
    })?;
    visit_phone_ranges(&normalized, |range| {
        push(
            "chinese_phone_number",
            range,
            action(
                runtime
                    .builtin_action_overrides
                    .get("chinese_phone_number")
                    .copied()
                    .unwrap_or(AuditAction::Redact),
            ),
            TokenCategory::Phone,
        )
    })?;
    for rule in runtime.custom_rules.iter().filter(|rule| rule.enabled) {
        if rule.rule_type == "regex" {
            // Preserve legacy disabling of invalid custom expressions.
            if let Some(regex) = &rule.regex {
                for matched in regex.find_iter(&normalized) {
                    push(
                        &rule.id,
                        matched.range(),
                        action(rule.action),
                        TokenCategory::Custom,
                    )?;
                }
            }
        } else if rule.rule_type == "keyword" && !rule.pattern.is_empty() {
            let regex = rule.regex.as_ref().ok_or(TokenError::RedactionFailed)?;
            for matched in regex.find_iter(&normalized) {
                push(
                    &rule.id,
                    matched.range(),
                    action(rule.action),
                    TokenCategory::Custom,
                )?;
            }
        }
    }
    Ok(found)
}

fn action(action: AuditAction) -> EffectiveAction {
    match action {
        AuditAction::Allow => EffectiveAction::Allow,
        AuditAction::Audit => EffectiveAction::Audit,
        AuditAction::Redact => EffectiveAction::Redact,
        AuditAction::RedactRestore => EffectiveAction::Restore,
        AuditAction::Block => EffectiveAction::Block,
    }
}

fn builtin_action(rule: &BuiltinRule, runtime: &SecurityRuntimeConfig) -> EffectiveAction {
    let chosen = runtime
        .builtin_action_overrides
        .get(rule.id)
        .copied()
        .unwrap_or(rule.default_action);
    if rule.id == "catalog:generic-sk-key-notice"
        && matches!(
            chosen,
            AuditAction::Block | AuditAction::Redact | AuditAction::RedactRestore
        )
    {
        return action(rule.default_action);
    }
    action(chosen)
}

fn category(category: &RuleCategory) -> TokenCategory {
    match category {
        RuleCategory::PrivateKey => TokenCategory::PrivateKey,
        RuleCategory::ApiToken => TokenCategory::ApiKey,
        RuleCategory::DbConnection => TokenCategory::Database,
        RuleCategory::Password => TokenCategory::Password,
        RuleCategory::Infrastructure => TokenCategory::Infrastructure,
        RuleCategory::Pii => TokenCategory::Personal,
        RuleCategory::Custom => TokenCategory::Custom,
    }
}

fn placeholder(text: &str) -> bool {
    matches!(
        text.trim(),
        "[REDACTED]" | "[REDACTED_PASSWORD]" | "[REDACTED_PRIVATE_KEY]" | "[REDACTED_DB_CONN]"
    )
}

// The borrowed variant owns no allocation; the owned variant retains its charge.
pub(super) enum NormalizedText<'a> {
    Borrowed(&'a str),
    Owned(PayloadText),
}
impl AsRef<str> for NormalizedText<'_> {
    fn as_ref(&self) -> &str {
        match self {
            Self::Borrowed(text) => text,
            Self::Owned(text) => text.as_str(),
        }
    }
}
impl Deref for NormalizedText<'_> {
    type Target = str;
    fn deref(&self) -> &str {
        self.as_ref()
    }
}

// Preserve exact original UTF-8 boundaries while matching legacy escaped spelling.
pub(super) fn normalize_with_budget(
    text: &str,
    budget: PayloadBudget,
) -> Result<(NormalizedText<'_>, Option<PayloadVec<usize>>), TokenError> {
    if text.len() > MAX_TEXT_BYTES {
        return Err(TokenError::CapacityExceeded);
    }
    let escapes = text.match_indices("\\_").count();
    if escapes == 0 {
        return Ok((NormalizedText::Borrowed(text), None));
    }
    let len = text
        .len()
        .checked_sub(escapes)
        .ok_or(TokenError::CapacityExceeded)?;
    let mut normalized = PayloadText::with_capacity(len, budget.clone(), MAX_TEXT_BYTES)?;
    let mut offsets = PayloadVec::new(budget)?;
    offsets.push(0)?;
    let mut chars = text.char_indices().peekable();
    while let Some((start, ch)) = chars.next() {
        if ch == '\\' && matches!(chars.peek(), Some((_, '_'))) {
            chars.next();
            normalized.append("_")?;
            offsets.push(start + 2)?;
        } else {
            normalized.append(&text[start..start + ch.len_utf8()])?;
            for byte in 1..=ch.len_utf8() {
                offsets.push(start + byte)?;
            }
        }
    }
    Ok((NormalizedText::Owned(normalized), Some(offsets)))
}

// Equivalent to comparing ASCII-lowercased copies; non-ASCII bytes remain exact.
pub(super) fn contains_ascii_case_insensitive(text: &str, keyword: &str) -> bool {
    keyword.is_empty()
        || text
            .as_bytes()
            .windows(keyword.len())
            .any(|candidate| candidate.eq_ignore_ascii_case(keyword.as_bytes()))
}

// Compare the two borrowed parts of the legacy replacement without allocating it.
pub(super) fn private_key_end(text: &str, start: usize, header: &str) -> usize {
    let Some(suffix) = header.strip_prefix("-----BEGIN ") else {
        return text.len();
    };
    text[start..]
        .match_indices("-----END ")
        .find_map(|(offset, _)| {
            text[start + offset + 9..]
                .starts_with(suffix)
                .then_some(start + offset + 9 + suffix.len())
        })
        .unwrap_or(text.len())
}

#[cfg(test)]
pub(super) mod test_support {
    use super::TokenError;
    use std::{cell::RefCell, marker::PhantomData, rc::Rc};
    type Hook = Box<dyn FnMut(usize) -> Result<(), TokenError>>;
    thread_local! { static HOOK: RefCell<Option<Hook>> = const { RefCell::new(None) }; }
    pub(in super::super) struct Guard {
        previous: Option<Hook>,
        _thread_bound: PhantomData<Rc<()>>,
    }
    impl Drop for Guard {
        fn drop(&mut self) {
            HOOK.with(|slot| slot.replace(self.previous.take()));
        }
    }
    pub(in super::super) fn install(
        hook: impl FnMut(usize) -> Result<(), TokenError> + 'static,
    ) -> Guard {
        Guard {
            previous: HOOK.with(|slot| slot.replace(Some(Box::new(hook)))),
            _thread_bound: PhantomData,
        }
    }
    pub(super) fn emit(used: usize) -> Result<(), TokenError> {
        HOOK.with(|slot| match slot.borrow_mut().as_mut() {
            Some(hook) => hook(used),
            None => Ok(()),
        })
    }
}
