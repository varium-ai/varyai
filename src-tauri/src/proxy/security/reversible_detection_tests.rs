//! Synthetic parity tests; the private span adapter does not publish an action.
use super::{
    engine::SecurityAuditEngine,
    payload_budget::PayloadBudget,
    request_detection::{detect_spans_with_budget, DetectedSpan},
    request_plan::EffectiveAction,
    runtime::{CompiledCustomRule, SecurityRuntimeConfig},
    tokenization::{TokenCategory, TokenError},
    types::{AuditAction, RiskLevel},
};

// Isolated budgets keep ordinary detector fixtures off the process-global probe.
fn detect_spans<'a>(
    text: &str,
    password: bool,
    runtime: &'a SecurityRuntimeConfig,
) -> Result<Vec<DetectedSpan<'a>>, TokenError> {
    let found = detect_spans_with_budget(
        text,
        password,
        runtime,
        PayloadBudget::with_limit(64 * 1024 * 1024),
    )?;
    Ok(found
        .as_slice()
        .iter()
        .map(|span| DetectedSpan {
            rule_id: span.rule_id,
            range: span.range.clone(),
            action: span.action,
            category: span.category,
        })
        .collect())
}

fn custom(pattern: &str, rule_type: &str, action: AuditAction) -> CompiledCustomRule {
    CompiledCustomRule {
        id: "synthetic-custom".into(),
        name: "synthetic-name".into(),
        rule_type: rule_type.into(),
        pattern: pattern.into(),
        regex: if rule_type == "regex" {
            regex::Regex::new(pattern).ok()
        } else if rule_type == "keyword" && !pattern.is_empty() {
            regex::RegexBuilder::new(&regex::escape(pattern))
                .case_insensitive(true)
                .build()
                .ok()
        } else {
            None
        },
        risk_level: RiskLevel::Medium,
        action,
        enabled: true,
    }
}

#[test]
fn detector_matches_legacy_rule_ids_on_synthetic_fixture_matrix() -> Result<(), TokenError> {
    for text in [
        "13800138000",
        "11010519491231002X",
        "192.168.1.20",
        "mysql://synthetic:synthetic@localhost/test",
        "sk-proj-abcdefghijklmnopqrstuvwxyz1234567890",
        "sk-ant-abcdefghijklmnopqrstuvwxyz1234567890",
        "ghp_abcdefghijklmnopqrstuvwxyz123456789012",
        "AWS AKIAIOSFODNN7EXAMPLE",
        "-----BEGIN PRIVATE KEY-----\nSYNTHETIC\n-----END PRIVATE KEY-----",
        "sk-shortsynthetic",
        "nothing sensitive",
        "[REDACTED]",
        "sk\\_test\\_abcdefghijklmnopqrstuvwxyz",
    ] {
        let runtime = SecurityRuntimeConfig::default();
        let mut expected: Vec<_> =
            SecurityAuditEngine::inspect_text_with_runtime(text, "", &runtime)
                .into_iter()
                .map(|f| f.rule_id)
                .collect();
        let mut actual: Vec<_> = detect_spans(text, false, &runtime)?
            .into_iter()
            .map(|f| f.rule_id.to_owned())
            .collect();
        expected.sort();
        actual.sort();
        assert!(expected == actual, "detector rule identities differ");
    }
    Ok(())
}

#[test]
fn escaped_underscore_spans_refer_to_exact_original_utf8_bytes() -> Result<(), TokenError> {
    let text = "中文 sk\\_test\\_abcdefghijklmnopqrstuvwxyz end";
    let runtime = SecurityRuntimeConfig::default();
    let found = detect_spans(text, false, &runtime)?;
    let found = found
        .iter()
        .find(|f| f.rule_id == "catalog:stripe-key")
        .ok_or(TokenError::RedactionFailed)?;
    assert!(text.get(found.range.clone()) == Some("sk\\_test\\_abcdefghijklmnopqrstuvwxyz"));
    assert_eq!(found.category, TokenCategory::ApiKey);
    Ok(())
}

#[test]
fn private_key_span_includes_complete_key_or_unclosed_tail() -> Result<(), TokenError> {
    for (text, expected) in [
        (
            "pre -----BEGIN PRIVATE KEY-----\nSYNTHETIC\n-----END PRIVATE KEY----- post",
            "-----BEGIN PRIVATE KEY-----\nSYNTHETIC\n-----END PRIVATE KEY-----",
        ),
        (
            "pre -----BEGIN PRIVATE KEY-----\nSYNTHETIC tail",
            "-----BEGIN PRIVATE KEY-----\nSYNTHETIC tail",
        ),
    ] {
        let runtime = SecurityRuntimeConfig::default();
        let found = detect_spans(text, false, &runtime)?;
        let key = found
            .iter()
            .find(|f| f.rule_id == "rsa_ssh_private_key")
            .ok_or(TokenError::RedactionFailed)?;
        assert!(text.get(key.range.clone()) == Some(expected));
        assert_eq!(key.category, TokenCategory::PrivateKey);
    }
    Ok(())
}

#[test]
fn password_field_capture_groups_allowlists_and_pii_keep_legacy_semantics() -> Result<(), TokenError>
{
    let runtime = SecurityRuntimeConfig::default();
    let text = "synthetic\\_password";
    let found = detect_spans(text, true, &runtime)?;
    assert!(found.iter().any(|f| f.rule_id == "catalog:password-field"
        && f.range == (0..text.len())
        && f.category == TokenCategory::Password));
    let text = "password=\"synthetic\"";
    let found = detect_spans(text, false, &runtime)?;
    assert!(found
        .iter()
        .any(|f| f.rule_id == "catalog:password-assignment"));
    assert!(found
        .iter()
        .filter(|f| f.category == TokenCategory::Password)
        .all(|f| text.get(f.range.clone()) == Some("synthetic")));
    assert!(
        detect_spans("AIzaSyabcdefghijklmnopqrstuvwxyz1234567", false, &runtime)?
            .iter()
            .all(|f| f.rule_id != "catalog:google-api-key")
    );
    let found = detect_spans(
        "13800138000 11010519491231002X 110105194912310021",
        false,
        &runtime,
    )?;
    assert_eq!(
        found
            .iter()
            .filter(|f| f.category == TokenCategory::Phone)
            .count(),
        1
    );
    assert_eq!(
        found
            .iter()
            .filter(|f| f.category == TokenCategory::IdCard)
            .count(),
        1
    );
    Ok(())
}

#[test]
fn custom_actions_keyword_casefold_and_disabled_runtime_are_respected() -> Result<(), TokenError> {
    for (action, expected) in [
        (AuditAction::Block, EffectiveAction::Block),
        (AuditAction::Redact, EffectiveAction::Redact),
        (AuditAction::Audit, EffectiveAction::Audit),
        (AuditAction::Allow, EffectiveAction::Allow),
    ] {
        let runtime = SecurityRuntimeConfig {
            custom_rules: vec![custom("中文a", "keyword", action)],
            ..Default::default()
        };
        let found = detect_spans("prefix 中文A suffix", false, &runtime)?;
        let found = found
            .iter()
            .find(|f| f.rule_id == "synthetic-custom")
            .ok_or(TokenError::RedactionFailed)?;
        assert_eq!(found.action, expected);
        assert_eq!(found.range, 7..14);
    }
    let runtime = SecurityRuntimeConfig {
        enabled: false,
        ..Default::default()
    };
    assert!(detect_spans("13800138000", true, &runtime)?.is_empty());
    Ok(())
}

#[test]
fn generic_sk_actions_remain_restricted_and_saved_defaults_are_unchanged() -> Result<(), TokenError>
{
    for action in [
        AuditAction::Block,
        AuditAction::Redact,
        AuditAction::Audit,
        AuditAction::Allow,
    ] {
        let mut runtime = SecurityRuntimeConfig::default();
        runtime
            .builtin_action_overrides
            .insert("catalog:generic-sk-key-notice".into(), action);
        let found = detect_spans("sk-synthetic123", false, &runtime)?;
        let generic = found
            .iter()
            .find(|f| f.rule_id == "catalog:generic-sk-key-notice")
            .ok_or(TokenError::RedactionFailed)?;
        assert_eq!(
            generic.action,
            if action == AuditAction::Audit {
                EffectiveAction::Audit
            } else {
                EffectiveAction::Allow
            }
        );
        let phone = detect_spans("13800138000", false, &runtime)?;
        assert_eq!(phone[0].action, EffectiveAction::Redact);
    }
    Ok(())
}

#[test]
fn detection_is_locally_bounded_and_zero_width_custom_matches_fail_closed() {
    let runtime = SecurityRuntimeConfig::default();
    assert!(matches!(
        detect_spans(&"x".repeat(1024 * 1024 + 1), false, &runtime),
        Err(TokenError::CapacityExceeded)
    ));
    let text = vec!["13800138000"; 4097].join(" ");
    assert!(matches!(
        detect_spans(&text, false, &runtime),
        Err(TokenError::CapacityExceeded)
    ));
    let runtime = SecurityRuntimeConfig {
        custom_rules: vec![custom("^", "regex", AuditAction::Redact)],
        ..Default::default()
    };
    assert!(matches!(
        detect_spans("synthetic", false, &runtime),
        Err(TokenError::RedactionFailed)
    ));
}

#[test]
fn exact_detector_limits_accept_one_mib_and_4096_occurrences() -> Result<(), TokenError> {
    let runtime = SecurityRuntimeConfig::default();
    assert!(detect_spans(&"x".repeat(1024 * 1024), false, &runtime)?.is_empty());
    let text = vec!["13800138000"; 4096].join(" ");
    assert_eq!(detect_spans(&text, false, &runtime)?.len(), 4096);
    Ok(())
}

#[test]
fn disabled_invalid_and_unicode_custom_rules_preserve_detection_boundaries(
) -> Result<(), TokenError> {
    let mut disabled = custom("synthetic", "keyword", AuditAction::Block);
    disabled.enabled = false;
    let invalid = CompiledCustomRule {
        regex: None,
        ..custom("synthetic", "regex", AuditAction::Redact)
    };
    let runtime = SecurityRuntimeConfig {
        custom_rules: vec![
            disabled,
            invalid,
            custom("秘密", "regex", AuditAction::Audit),
        ],
        ..Default::default()
    };
    let text = "synthetic 中文秘密";
    let found = detect_spans(text, false, &runtime)?;
    assert_eq!(found.len(), 1);
    assert!(text.get(found[0].range.clone()) == Some("秘密"));
    assert_eq!(found[0].action, EffectiveAction::Audit);
    Ok(())
}

#[test]
fn review_zero_width_allow_audit_and_block_preserve_effective_actions() -> Result<(), TokenError> {
    for (chosen, expected) in [
        (AuditAction::Allow, EffectiveAction::Allow),
        (AuditAction::Audit, EffectiveAction::Audit),
        (AuditAction::Block, EffectiveAction::Block),
    ] {
        let runtime = SecurityRuntimeConfig {
            custom_rules: vec![custom("^", "regex", chosen)],
            ..SecurityRuntimeConfig::default()
        };
        let spans = detect_spans("synthetic", false, &runtime)?;
        assert!(spans.iter().any(|span| span.rule_id == "synthetic-custom"
            && span.range == (0..0)
            && span.action == expected));
    }
    Ok(())
}

#[test]
fn review_private_detector_bounds_configured_request_work_before_scanning() {
    let oversized = "(".repeat(1024 * 1024 + 1);
    let runtime = SecurityRuntimeConfig {
        custom_rules: vec![custom(&oversized, "keyword", AuditAction::Audit)],
        ..SecurityRuntimeConfig::default()
    };
    assert_eq!(
        detect_spans("synthetic", false, &runtime).err(),
        Some(TokenError::CapacityExceeded)
    );
    let runtime = SecurityRuntimeConfig {
        custom_rules: (0..4097)
            .map(|_| custom("synthetic", "keyword", AuditAction::Audit))
            .collect(),
        ..SecurityRuntimeConfig::default()
    };
    assert_eq!(
        detect_spans("no match", false, &runtime).err(),
        Some(TokenError::CapacityExceeded)
    );
}

#[test]
fn detector_owner_rejects_zero_before_retaining_results() {
    let runtime = SecurityRuntimeConfig::default();
    let budget = PayloadBudget::with_limit(0);
    let result = detect_spans_with_budget("13800138000", false, &runtime, budget.clone());
    assert_eq!(result.err(), Some(TokenError::CapacityExceeded));
    assert_eq!(budget.used(), 0);
}

#[test]
fn detector_owner_retains_actual_result_capacity_until_drop() -> Result<(), TokenError> {
    let runtime = SecurityRuntimeConfig::default();
    let bytes = 4 * std::mem::size_of::<DetectedSpan<'_>>();
    let budget = PayloadBudget::with_limit(bytes);
    let found = detect_spans_with_budget("13800138000", false, &runtime, budget.clone())?;
    assert_eq!(found.len(), 1);
    assert_eq!(budget.used(), bytes);
    drop(found);
    assert_eq!(budget.used(), 0);
    Ok(())
}

#[test]
fn detector_owner_rejects_one_byte_below_first_capacity() {
    let runtime = SecurityRuntimeConfig::default();
    let bytes = 4 * std::mem::size_of::<DetectedSpan<'_>>();
    let budget = PayloadBudget::with_limit(bytes - 1);
    assert_eq!(
        detect_spans_with_budget("13800138000", false, &runtime, budget.clone()).err(),
        Some(TokenError::CapacityExceeded)
    );
    assert_eq!(budget.used(), 0);
}

#[test]
fn normalization_owner_rejects_zero_even_without_matches() {
    let runtime = SecurityRuntimeConfig::default();
    let budget = PayloadBudget::with_limit(0);
    assert_eq!(
        detect_spans_with_budget("中文\\_é", false, &runtime, budget.clone()).err(),
        Some(TokenError::CapacityExceeded)
    );
    assert_eq!(budget.used(), 0);
}

#[test]
fn normalization_owner_retains_exact_text_and_utf8_offsets() -> Result<(), TokenError> {
    let budget = PayloadBudget::with_limit(4096);
    let (text, offsets) =
        super::request_detection::normalize_with_budget("中文\\_é\\_尾", budget.clone())?;
    assert_eq!(text.as_ref(), "中文_é_尾");
    assert_eq!(
        offsets.as_ref().map(|offsets| offsets.as_slice()),
        Some([0usize, 1, 2, 3, 4, 5, 6, 8, 9, 10, 12, 13, 14, 15].as_slice())
    );
    assert_eq!(budget.used(), 13 + 16 * std::mem::size_of::<usize>());
    drop((text, offsets));
    assert_eq!(budget.used(), 0);
    Ok(())
}

#[test]
fn normalization_owner_exact_growth_peak_and_one_byte_below() -> Result<(), TokenError> {
    // 13 normalized bytes: 16 offsets retained, all 8 old slots live during final growth.
    let peak = 13 + (8 + 16) * std::mem::size_of::<usize>();
    let budget = PayloadBudget::with_limit(peak);
    let owner = super::request_detection::normalize_with_budget("中文\\_é\\_尾", budget.clone())?;
    assert_eq!(budget.used(), 13 + 16 * std::mem::size_of::<usize>());
    drop(owner);
    assert_eq!(budget.used(), 0);
    let short = PayloadBudget::with_limit(peak - 1);
    assert_eq!(
        super::request_detection::normalize_with_budget("中文\\_é\\_尾", short.clone()).err(),
        Some(TokenError::CapacityExceeded)
    );
    assert_eq!(short.used(), 0);
    Ok(())
}

#[test]
fn normalization_owner_precharges_and_releases_on_hook_failure_or_unwind() {
    use super::payload_text::test_support::{self, Action, Phase};
    use std::{
        cell::Cell,
        panic::{catch_unwind, AssertUnwindSafe},
        rc::Rc,
    };
    for unwind in [false, true] {
        let budget = PayloadBudget::with_limit(4096);
        let seen = Rc::new(Cell::new(false));
        let observed = seen.clone();
        let _hook = test_support::install(move |event| {
            if event.phase == Phase::BeforeAllocation {
                assert!(event.used >= event.bytes);
                observed.set(true);
                if unwind {
                    panic!("synthetic allocation interruption");
                }
                return Action::Fail;
            }
            Action::Continue
        });
        let result = catch_unwind(AssertUnwindSafe(|| {
            super::request_detection::normalize_with_budget("中文\\_é", budget.clone())
        }));
        assert!(
            seen.get(),
            "normalization must allocate through the charged text owner"
        );
        if unwind {
            assert!(result.is_err());
        } else {
            assert!(matches!(result, Ok(Err(TokenError::Unavailable))));
        }
        assert_eq!(budget.used(), 0);
    }
}

#[test]
fn builtin_trigger_source_guard_forbids_old_lowercase_allocations() -> Result<(), TokenError> {
    let runtime = SecurityRuntimeConfig::default();
    assert!(!detect_spans("AWS AKIAIOSFODNN7EXAMPLE", false, &runtime)?.is_empty());
    // This is a scoped source regression guard, not runtime allocator evidence.
    let source = include_str!("request_detection.rs");
    let builtin = source
        .split_once("if matcher.is_match(")
        .unwrap()
        .1
        .split_once("if is_password_field")
        .unwrap()
        .0;
    let helper = source
        .split_once("pub(super) fn contains_ascii_case_insensitive")
        .unwrap()
        .1
        .split_once("// Compare the two borrowed parts")
        .unwrap()
        .0;
    assert!(builtin.contains("contains_ascii_case_insensitive("));
    for forbidden in [
        "String::",
        "Vec::",
        "format!",
        "to_ascii_lowercase(",
        "make_ascii_lowercase(",
        "to_owned(",
        "to_string(",
    ] {
        assert!(!builtin.contains(forbidden) && !helper.contains(forbidden));
    }
    Ok(())
}

#[test]
fn private_key_marker_source_guard_forbids_old_replacement_allocation() -> Result<(), TokenError> {
    let runtime = SecurityRuntimeConfig::default();
    let text = "pre -----BEGIN RSA PRIVATE KEY-----\nSYNTHETIC\n-----END EC PRIVATE KEY-----\n-----END RSA PRIVATE KEY----- post";
    let found = detect_spans(text, false, &runtime)?;
    let span = found
        .iter()
        .find(|span| span.rule_id == "rsa_ssh_private_key")
        .ok_or(TokenError::Unavailable)?;
    assert_eq!(span.range.end, text.len() - 5);
    // Deliberately limited to the known marker construction and helper body.
    let source = include_str!("request_detection.rs");
    let builtin = source
        .split_once("if matcher.is_match(")
        .unwrap()
        .1
        .split_once("if is_password_field")
        .unwrap()
        .0;
    let helper = source
        .split_once("pub(super) fn private_key_end")
        .unwrap()
        .1
        .split_once("#[cfg(test)]")
        .unwrap()
        .0;
    assert!(builtin.contains("private_key_end("));
    for forbidden in [
        "String::",
        "Vec::",
        "format!",
        "replacen(",
        "replace(",
        "to_owned(",
        "to_string(",
    ] {
        assert!(!builtin.contains(forbidden) && !helper.contains(forbidden));
    }
    Ok(())
}

#[test]
fn allocation_free_trigger_and_marker_helpers_match_legacy_operations() {
    use super::request_detection::{contains_ascii_case_insensitive, private_key_end};
    for text in ["", "AbC中文éK", "sk_test", "-----BEGIN", "AAA", "éÉ", "İi"] {
        for keyword in ["", "a", "ABC", "sk_", "中", "é", "É", "k", "K", "İ"] {
            assert_eq!(
                contains_ascii_case_insensitive(text, keyword),
                text.contains(keyword)
                    || text
                        .to_ascii_lowercase()
                        .contains(&keyword.to_ascii_lowercase())
            );
        }
    }
    for header in [
        "-----BEGIN PRIVATE KEY-----",
        "-----BEGIN RSA PRIVATE KEY-----",
        "-----BEGIN EC PRIVATE KEY-----",
    ] {
        let marker = header.replacen("-----BEGIN ", "-----END ", 1);
        for tail in [
            "none".to_owned(),
            marker.clone(),
            format!("-----END WRONG----- {marker} tail"),
            format!("中文{marker}{marker}"),
        ] {
            let text = format!("{header}\n{tail}");
            let expected = text[header.len()..]
                .find(&marker)
                .map(|offset| header.len() + offset + marker.len())
                .unwrap_or(text.len());
            assert_eq!(private_key_end(&text, header.len(), header), expected);
        }
    }
}

#[test]
fn detector_owner_growth_counts_complete_old_and_new_capacities() -> Result<(), TokenError> {
    let runtime = SecurityRuntimeConfig::default();
    let size = std::mem::size_of::<DetectedSpan<'_>>();
    let text = ["13800138000"; 5].join(" ");
    let budget = PayloadBudget::with_limit((4 + 8) * size);
    let found = detect_spans_with_budget(&text, false, &runtime, budget.clone())?;
    assert_eq!(found.len(), 5);
    assert_eq!(found.capacity(), 8);
    assert_eq!(budget.used(), 8 * size);
    drop(found);
    assert_eq!(budget.used(), 0);
    let short = PayloadBudget::with_limit((4 + 8) * size - 1);
    assert_eq!(
        detect_spans_with_budget(&text, false, &runtime, short.clone()).err(),
        Some(TokenError::CapacityExceeded)
    );
    assert_eq!(short.used(), 0);
    Ok(())
}

#[test]
fn detector_owner_counts_live_normalization_alongside_results() -> Result<(), TokenError> {
    let runtime = SecurityRuntimeConfig::default();
    let retained_result = 4 * std::mem::size_of::<DetectedSpan<'_>>();
    let peak = 13 + 16 * std::mem::size_of::<usize>() + retained_result;
    let budget = PayloadBudget::with_limit(peak);
    let found = detect_spans_with_budget("中文\\_é\\_尾", true, &runtime, budget.clone())?;
    assert_eq!(found.len(), 1);
    assert_eq!(found.as_slice()[0].range, 0..15);
    assert_eq!(budget.used(), retained_result);
    drop(found);
    assert_eq!(budget.used(), 0);
    let short = PayloadBudget::with_limit(peak - 1);
    assert_eq!(
        detect_spans_with_budget("中文\\_é\\_尾", true, &runtime, short.clone()).err(),
        Some(TokenError::CapacityExceeded)
    );
    assert_eq!(short.used(), 0);
    Ok(())
}

#[test]
fn detector_borrowed_and_disabled_paths_need_no_payload_capacity() -> Result<(), TokenError> {
    let budget = PayloadBudget::with_limit(0);
    let runtime = SecurityRuntimeConfig::default();
    let owner = super::request_detection::normalize_with_budget("中文é", budget.clone())?;
    assert_eq!(owner.0.as_ref().as_ptr(), "中文é".as_ptr());
    assert!(owner.1.is_none());
    assert_eq!(
        detect_spans_with_budget("nothing sensitive", false, &runtime, budget.clone())?.len(),
        0
    );
    assert_eq!(
        detect_spans_with_budget("", true, &runtime, budget.clone())?.len(),
        0
    );
    let disabled = SecurityRuntimeConfig {
        enabled: false,
        ..runtime
    };
    assert_eq!(
        detect_spans_with_budget("中文\\_13800138000", true, &disabled, budget.clone())?.len(),
        0
    );
    assert_eq!(budget.used(), 0);
    Ok(())
}

#[test]
fn detector_all_text_and_metadata_failure_points_release_partial_owners() -> Result<(), TokenError>
{
    use super::{payload_metadata::test_support as metadata, payload_text::test_support as text};
    use std::{
        cell::Cell,
        panic::{catch_unwind, AssertUnwindSafe},
        rc::Rc,
    };
    let runtime = SecurityRuntimeConfig::default();
    let fixture = "中文\\_ 13800138000 13800138000 13800138000 13800138000 13800138000";
    let calls = Rc::new(Cell::new(0));
    let observed = calls.clone();
    let metadata_calls = calls.clone();
    let budget = PayloadBudget::with_limit(1024 * 1024);
    let a = text::install(move |event| {
        assert!(event.used >= event.bytes);
        observed.set(observed.get() + 1);
        text::Action::Continue
    });
    let b = metadata::install(move |event| {
        assert!(event.used >= event.bytes);
        metadata_calls.set(metadata_calls.get() + 1);
        metadata::Action::Continue
    });
    drop(detect_spans_with_budget(
        fixture,
        false,
        &runtime,
        budget.clone(),
    )?);
    drop((a, b));
    assert_eq!(budget.used(), 0);
    assert!(calls.get() > 20);
    for unwind in [false, true] {
        for target in 1..=calls.get() {
            let visits = Rc::new(Cell::new(0));
            let text_visits = visits.clone();
            let meta_visits = visits.clone();
            let a = text::install(move |_| {
                text_visits.set(text_visits.get() + 1);
                if text_visits.get() == target {
                    if unwind {
                        panic!("synthetic text allocation interruption");
                    }
                    return text::Action::Fail;
                }
                text::Action::Continue
            });
            let b = metadata::install(move |_| {
                meta_visits.set(meta_visits.get() + 1);
                if meta_visits.get() == target {
                    if unwind {
                        panic!("synthetic metadata allocation interruption");
                    }
                    return metadata::Action::Fail;
                }
                metadata::Action::Continue
            });
            let result = catch_unwind(AssertUnwindSafe(|| {
                detect_spans_with_budget(fixture, false, &runtime, budget.clone())
            }));
            if unwind {
                assert!(result.is_err());
            } else {
                assert!(matches!(result, Ok(Err(TokenError::Unavailable))));
            }
            assert_eq!(visits.get(), target);
            assert_eq!(budget.used(), 0);
            drop((a, b));
        }
    }
    Ok(())
}

#[test]
fn keyword_missing_compiled_snapshot_fails_closed_without_request_time_fallback() {
    for action in [
        AuditAction::Allow,
        AuditAction::Audit,
        AuditAction::Redact,
        AuditAction::Block,
    ] {
        let runtime = SecurityRuntimeConfig {
            custom_rules: vec![CompiledCustomRule {
                regex: None,
                ..custom("synthetic-keyword", "keyword", action)
            }],
            ..Default::default()
        };
        let budget = PayloadBudget::with_limit(64 * 1024 * 1024);
        let result = detect_spans_with_budget(
            "unmatched ordinary fixture",
            false,
            &runtime,
            budget.clone(),
        );
        assert_eq!(result.err(), Some(TokenError::RedactionFailed));
        assert_eq!(budget.used(), 0);
    }
}

#[test]
fn keyword_missing_compiler_is_inactive_only_for_empty_disabled_or_disabled_dlp(
) -> Result<(), TokenError> {
    for (pattern, enabled, dlp) in [
        ("", true, true),
        ("synthetic", false, true),
        ("synthetic", true, false),
    ] {
        let runtime = SecurityRuntimeConfig {
            enabled: dlp,
            custom_rules: vec![CompiledCustomRule {
                enabled,
                regex: None,
                ..custom(pattern, "keyword", AuditAction::Block)
            }],
            ..Default::default()
        };
        assert!(detect_spans("synthetic", false, &runtime)?.is_empty());
    }
    Ok(())
}

#[test]
fn keyword_precompiled_unicode_literal_spans_match_legacy_casefold_and_offsets(
) -> Result<(), TokenError> {
    for (pattern, text) in [
        ("K.Σ", "尾 k.ς K.σ K.Σ kXς"),
        ("[a]+", "尾 [A]+ [a]+ aaaa"),
        ("İ", "尾 İ i I ı"),
        ("ſ", "尾 s S ſ"),
        ("秘密", "尾秘密秘密"),
        ("a_a", "尾A\\_A a_a"),
        ("aba", "尾ababa ABA"),
    ] {
        let runtime = SecurityRuntimeConfig {
            custom_rules: vec![custom(pattern, "keyword", AuditAction::Audit)],
            ..Default::default()
        };
        let normalized = text.replace("\\_", "_");
        let legacy = regex::RegexBuilder::new(&regex::escape(pattern))
            .case_insensitive(true)
            .build()
            .map_err(|_| TokenError::Unavailable)?;
        let expected = legacy.find_iter(&normalized).count();
        let found = detect_spans(text, false, &runtime)?;
        let custom: Vec<_> = found
            .iter()
            .filter(|span| span.rule_id == "synthetic-custom")
            .collect();
        assert_eq!(custom.len(), expected);
        for span in custom {
            let original = text
                .get(span.range.clone())
                .ok_or(TokenError::RedactionFailed)?;
            assert!(legacy.is_match(&original.replace("\\_", "_")));
            assert_eq!(span.action, EffectiveAction::Audit);
        }
    }
    Ok(())
}

#[test]
fn keyword_precompiled_spans_keep_exact_shared_result_budget_boundaries() -> Result<(), TokenError>
{
    let runtime = SecurityRuntimeConfig {
        custom_rules: vec![custom("K.Σ", "keyword", AuditAction::Redact)],
        ..Default::default()
    };
    let text = "k.ς k.σ k.Σ k.ς k.σ";
    let probe = PayloadBudget::with_limit(64 * 1024 * 1024);
    let found = detect_spans_with_budget(text, false, &runtime, probe.clone())?;
    assert_eq!(found.len(), 5);
    let retained = probe.used();
    drop(found);
    assert_eq!(probe.used(), 0);
    // Five results grow from four to eight slots; both full backings coexist.
    let width = std::mem::size_of::<DetectedSpan<'_>>();
    let peak = 12 * width;
    assert_eq!(retained, 8 * width);
    for limit in [peak - 1, peak] {
        let budget = PayloadBudget::with_limit(limit);
        let result = detect_spans_with_budget(text, false, &runtime, budget.clone());
        if limit < peak {
            assert_eq!(result.err(), Some(TokenError::CapacityExceeded));
        } else {
            let result = result?;
            assert_eq!(result.len(), 5);
            assert_eq!(budget.used(), retained);
            // A second detector result cannot reuse the live first result's charge.
            assert_eq!(
                detect_spans_with_budget(text, false, &runtime, budget.clone()).err(),
                Some(TokenError::CapacityExceeded)
            );
            assert_eq!(budget.used(), retained);
            drop(result);
        }
        assert_eq!(budget.used(), 0);
    }
    Ok(())
}
