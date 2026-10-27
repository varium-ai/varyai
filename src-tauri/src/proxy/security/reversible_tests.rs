//! Synthetic-only tests for the private reversible DLP core.
use super::tokenization::{
    make_token, scan_token_references, select_category, TokenCategory, TokenError,
};

fn test_key() -> [u8; 32] {
    std::array::from_fn(|index| index as u8)
}

#[test]
fn token_matches_the_specification_vector_and_exact_length() -> Result<(), TokenError> {
    let token = make_token(&test_key(), "13800138000", TokenCategory::Phone)?;
    assert!(token == "vary_ph_413ff0a68f6f0d574cb5_ai");
    assert_eq!(token.len(), 31);
    assert!(token.is_ascii());
    Ok(())
}

#[test]
fn digest_preserves_exact_decoded_bytes_and_excludes_category() -> Result<(), TokenError> {
    let key = test_key();
    let first = make_token(&key, "fixture", TokenCategory::Phone)?;
    let other_category = make_token(&key, "fixture", TokenCategory::Custom)?;
    assert!(first[8..28] == other_category[8..28]);
    for different in [" fixture", "fixture ", "FIXTURE", "fixture\n"] {
        assert!(first != make_token(&key, different, TokenCategory::Phone)?);
    }
    assert!(
        make_token(&key, "\u{00e9}", TokenCategory::Custom)?
            != make_token(&key, "e\u{0301}", TokenCategory::Custom)?
    );
    assert!(first != make_token(&[42; 32], "fixture", TokenCategory::Phone)?);
    Ok(())
}

#[test]
fn full_span_categories_have_stable_precedence_and_mixed_fallback() {
    use TokenCategory::*;
    let ordered = [
        PrivateKey,
        Database,
        ApiKey,
        Password,
        IdCard,
        Phone,
        Infrastructure,
        Personal,
        Custom,
    ];
    for (index, expected) in ordered.iter().enumerate() {
        assert_eq!(select_category(&ordered[index..]), *expected);
        let reversed: Vec<_> = ordered[index..].iter().copied().rev().collect();
        assert_eq!(select_category(&reversed), *expected);
    }
    assert_eq!(select_category(&[]), Mixed);
}

#[test]
fn scanner_finds_adjacent_tokens_with_original_utf8_byte_offsets() -> Result<(), TokenError> {
    let token = make_token(&test_key(), "fixture", TokenCategory::Custom)?;
    let text = format!("前缀{token}{token}末尾");
    let ranges = scan_token_references(&text)?;
    assert_eq!(ranges.len(), 2);
    assert!(ranges[0] == (6..37) && ranges[1] == (37..68));
    assert!(ranges
        .iter()
        .all(|range| text.get(range.clone()) == Some(token.as_str())));
    assert!(scan_token_references("vary_identifier vary_zz_bad_ai vary_ph")?.is_empty());
    Ok(())
}

#[test]
fn scanner_rejects_malformed_candidates_only_after_a_known_header() {
    for candidate in [
        "vary_ph_",
        "vary_ph_413ff0a68f6f0d574cb5_Ai",
        "vary_ph_413FF0A68F6F0D574CB5_ai",
        "vary_ph_413ff0a68f6f0d574cb_ai",
        "vary_ph_413ff0a68f6f0d574cb5_xx",
    ] {
        assert!(matches!(
            scan_token_references(candidate),
            Err(TokenError::ContextUnavailable)
        ));
    }
}

#[test]
fn core_errors_expose_fixed_codes_only() {
    for (error, code) in [
        (TokenError::Unavailable, "dlp_unavailable"),
        (TokenError::ContextUnavailable, "dlp_context_unavailable"),
        (TokenError::CapacityExceeded, "dlp_capacity_exceeded"),
        (TokenError::TokenCollision, "dlp_token_collision"),
        (TokenError::RedactionFailed, "dlp_redaction_failed"),
    ] {
        assert_eq!(error.code(), code);
        assert_eq!(error.to_string(), code);
    }
}

use super::tokenization::{DlpTokenStore, TokenFragment, TokenStoreLimits};
use std::time::{Duration, Instant};

fn fragment(raw: &str) -> TokenFragment<'_> {
    TokenFragment {
        raw,
        whole_span_categories: &[TokenCategory::Custom],
    }
}

#[test]
fn mappings_reuse_the_first_category_but_not_another_requests_authority() -> Result<(), TokenError>
{
    let store = DlpTokenStore::new();
    let mut first = store.admit("codex", Some("session-a"), 1)?;
    let tokens = first.prepare(&[fragment("fixture")], &[], |tokens| Ok(tokens.to_vec()))?;
    assert!(first.resolve(&tokens[0])? == "fixture");
    let mut second = store.admit("codex", Some("session-a"), 1)?;
    assert!(matches!(
        second.resolve(&tokens[0]),
        Err(TokenError::ContextUnavailable)
    ));
    let reused = second.prepare(
        &[TokenFragment {
            raw: "fixture",
            whole_span_categories: &[TokenCategory::PrivateKey],
        }],
        &[],
        |tokens| Ok(tokens.to_vec()),
    )?;
    assert!(reused == tokens);
    assert!(second.resolve(&reused[0])? == "fixture");
    assert_eq!(store.stats()?.entries, 1);
    assert_eq!(store.stats()?.plaintext_bytes, "fixture".len());
    Ok(())
}

#[test]
fn namespace_keys_isolate_apps_sessions_policy_generations_and_requests() -> Result<(), TokenError>
{
    let store = DlpTokenStore::new();
    let mut tokens: Vec<String> = Vec::new();
    for (app, session, revision) in [
        ("codex", Some("session-a"), 1),
        ("codex", Some("session-b"), 1),
        ("other", Some("session-a"), 1),
        ("codex", Some("session-a"), 2),
        ("codex", None, 1),
        ("codex", None, 1),
    ] {
        let mut lease = store.admit(app, session, revision)?;
        let issued = lease.prepare(&[fragment("fixture")], &[], |tokens| Ok(tokens.to_vec()))?;
        assert!(tokens.iter().all(|prior| prior != &issued[0]));
        for prior in &tokens {
            assert!(matches!(
                lease.resolve(prior),
                Err(TokenError::ContextUnavailable)
            ));
        }
        tokens.push(issued[0].clone());
    }
    assert_eq!(store.stats()?.namespaces, 6);
    Ok(())
}

#[test]
fn invalid_stable_identities_never_create_multi_turn_continuity() -> Result<(), TokenError> {
    let store = DlpTokenStore::new();
    let oversized = "x".repeat(257);
    for invalid in ["", " ", "has space", "has\ncontrol", oversized.as_str()] {
        let mut first = store.admit("codex", Some(invalid), 1)?;
        let issued = first.prepare(&[fragment("fixture")], &[], |tokens| Ok(tokens.to_vec()))?;
        let second = store.admit("codex", Some(invalid), 1)?;
        assert!(matches!(
            second.resolve(&issued[0]),
            Err(TokenError::ContextUnavailable)
        ));
    }
    Ok(())
}

#[test]
fn rejected_candidates_do_not_commit_entries_or_authority() -> Result<(), TokenError> {
    let store = DlpTokenStore::new();
    let mut lease = store.admit("codex", Some("session-a"), 1)?;
    let before = store.stats()?;
    let mut rejected = Vec::new();
    let result: Result<(), TokenError> =
        lease.prepare(&[fragment("first"), fragment("second")], &[], |tokens| {
            rejected = tokens.to_vec();
            Err(TokenError::RedactionFailed)
        });
    assert!(matches!(result, Err(TokenError::RedactionFailed)));
    assert_eq!(store.stats()?, before);
    for token in rejected {
        assert!(matches!(
            lease.resolve(&token),
            Err(TokenError::ContextUnavailable)
        ));
    }
    Ok(())
}

#[test]
fn a_batch_exceeding_capacity_is_atomic_and_existing_mappings_survive() -> Result<(), TokenError> {
    let limits = TokenStoreLimits {
        entries_per_namespace: 2,
        ..Default::default()
    };
    let store = DlpTokenStore::with_test_limits(limits);
    let mut lease = store.admit("codex", Some("session-a"), 1)?;
    let original = lease.prepare(&[fragment("first")], &[], |tokens| Ok(tokens.to_vec()))?;
    let before = store.stats()?;
    assert!(matches!(
        lease.prepare(&[fragment("second"), fragment("third")], &[], |_| Ok(())),
        Err(TokenError::CapacityExceeded)
    ));
    assert_eq!(store.stats()?, before);
    assert!(lease.resolve(&original[0])? == "first");
    Ok(())
}

#[test]
fn fragment_plaintext_global_and_metadata_limits_are_enforced() -> Result<(), TokenError> {
    let limits_to_check = [
        TokenStoreLimits {
            fragment_bytes: 3,
            ..Default::default()
        },
        TokenStoreLimits {
            plaintext_per_namespace: 3,
            ..Default::default()
        },
        TokenStoreLimits {
            global_plaintext_bytes: 3,
            ..Default::default()
        },
        TokenStoreLimits {
            global_entries: 0,
            ..Default::default()
        },
    ];
    for limits in limits_to_check {
        let store = DlpTokenStore::with_test_limits(limits);
        let mut lease = store.admit("codex", None, 1)?;
        let before = store.stats()?;
        assert!(matches!(
            lease.prepare(&[fragment("four")], &[], |_| Ok(())),
            Err(TokenError::CapacityExceeded)
        ));
        assert_eq!(store.stats()?, before);
    }
    let store = DlpTokenStore::with_test_limits(TokenStoreLimits {
        global_metadata_bytes: 0,
        ..Default::default()
    });
    assert!(matches!(
        store.admit("codex", None, 1),
        Err(TokenError::CapacityExceeded)
    ));
    assert_eq!(store.stats()?.namespaces, 0);
    Ok(())
}

#[test]
fn same_plaintext_concurrent_commits_reuse_the_winning_category() -> Result<(), TokenError> {
    let store = DlpTokenStore::new();
    let first = store.admit("codex", Some("session-a"), 1)?;
    let second = store.admit("codex", Some("session-a"), 1)?;
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let mut threads = Vec::new();
    for (mut lease, category) in [
        (first, TokenCategory::Phone),
        (second, TokenCategory::ApiKey),
    ] {
        let barrier = barrier.clone();
        threads.push(std::thread::spawn(move || {
            let mut wait_once = true;
            lease.prepare(
                &[TokenFragment {
                    raw: "fixture",
                    whole_span_categories: &[category],
                }],
                &[],
                |tokens| {
                    if wait_once {
                        wait_once = false;
                        barrier.wait();
                    }
                    Ok(tokens.to_vec())
                },
            )
        }));
    }
    let mut issued = Vec::new();
    for thread in threads {
        issued.push(thread.join().map_err(|_| TokenError::Unavailable)??);
    }
    assert!(issued[0] == issued[1]);
    assert_eq!(store.stats()?.entries, 1);
    Ok(())
}

#[test]
fn validation_runs_without_holding_store_locks() -> Result<(), TokenError> {
    let store = DlpTokenStore::new();
    let mut lease = store.admit("codex", None, 1)?;
    lease.prepare(&[fragment("fixture")], &[], |_| {
        assert_eq!(store.stats()?.entries, 0);
        Ok(())
    })?;
    Ok(())
}

#[test]
fn literal_reference_collisions_fail_without_committing_anything() -> Result<(), TokenError> {
    let store = DlpTokenStore::with_test_key(test_key());
    let mut lease = store.admit("codex", Some("session-a"), 1)?;
    let literal = make_token(&test_key(), "fixture", TokenCategory::Custom)?;
    let before = store.stats()?;
    assert!(matches!(
        lease.prepare(&[fragment("fixture")], &[literal.as_str()], |_| Ok(())),
        Err(TokenError::TokenCollision)
    ));
    assert_eq!(store.stats()?, before);
    Ok(())
}

#[test]
fn truncated_digest_collisions_never_overwrite_an_existing_value() -> Result<(), TokenError> {
    let store = DlpTokenStore::with_test_digest([7; 10]);
    let mut lease = store.admit("codex", Some("session-a"), 1)?;
    let original = lease.prepare(&[fragment("first")], &[], |tokens| Ok(tokens.to_vec()))?;
    let before = store.stats()?;
    assert!(matches!(
        lease.prepare(&[fragment("second")], &[], |_| Ok(())),
        Err(TokenError::TokenCollision)
    ));
    assert_eq!(store.stats()?, before);
    assert!(lease.resolve(&original[0])? == "first");
    Ok(())
}

#[test]
fn idle_expiry_never_evicts_an_active_lease_and_releases_capacity_after_drop(
) -> Result<(), TokenError> {
    let store = DlpTokenStore::with_test_limits(TokenStoreLimits {
        namespaces: 1,
        idle_ttl: Duration::from_secs(1),
        ..Default::default()
    });
    let mut lease = store.admit("codex", Some("session-a"), 1)?;
    let issued = lease.prepare(&[fragment("fixture")], &[], |tokens| Ok(tokens.to_vec()))?;
    let future = Instant::now() + Duration::from_secs(60);
    store.expire_idle_at(future)?;
    assert_eq!(store.stats()?.namespaces, 1);
    assert!(lease.resolve(&issued[0])? == "fixture");
    assert!(matches!(
        store.admit("codex", Some("session-b"), 1),
        Err(TokenError::CapacityExceeded)
    ));
    drop(lease);
    store.expire_idle_at(future)?;
    assert_eq!(store.stats()?.namespaces, 0);
    assert_eq!(store.stats()?.plaintext_bytes, 0);
    assert_eq!(store.stats()?.metadata_bytes, 0);
    let replacement = store.admit("codex", Some("session-b"), 1)?;
    assert!(matches!(
        replacement.resolve(&issued[0]),
        Err(TokenError::ContextUnavailable)
    ));
    Ok(())
}

#[test]
fn callback_unwind_releases_reservations_without_poisoning_the_store() -> Result<(), TokenError> {
    let store = DlpTokenStore::new();
    let mut lease = store.admit("codex", None, 1)?;
    let before = store.stats()?;
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _: Result<(), TokenError> = lease.prepare(&[fragment("fixture")], &[], |_| {
            panic!("synthetic candidate cancellation");
        });
    }));
    assert!(result.is_err());
    assert_eq!(store.stats()?, before);
    lease.prepare(&[fragment("fixture")], &[], |_| Ok(()))?;
    assert_eq!(store.stats()?.entries, 1);
    Ok(())
}

#[test]
fn a_category_changed_reference_cannot_borrow_an_authorized_digest() -> Result<(), TokenError> {
    let store = DlpTokenStore::with_test_key(test_key());
    let mut lease = store.admit("codex", None, 1)?;
    lease.prepare(&[fragment("fixture")], &[], |_| Ok(()))?;
    let other = make_token(&test_key(), "fixture", TokenCategory::PrivateKey)?;
    assert!(matches!(
        lease.resolve(&other),
        Err(TokenError::ContextUnavailable)
    ));
    Ok(())
}

#[test]
fn limits_remain_reserved_while_a_candidate_is_being_validated() -> Result<(), TokenError> {
    let store = DlpTokenStore::with_test_limits(TokenStoreLimits {
        global_plaintext_bytes: 7,
        ..Default::default()
    });
    let mut first = store.admit("codex", Some("session-a"), 1)?;
    let mut second = store.admit("codex", Some("session-b"), 1)?;
    first.prepare(&[fragment("fixture")], &[], |_| {
        assert!(matches!(
            second.prepare(&[fragment("another")], &[], |_| Ok(())),
            Err(TokenError::CapacityExceeded)
        ));
        Ok(())
    })?;
    assert_eq!(store.stats()?.plaintext_bytes, 7);
    Ok(())
}

#[test]
fn all_supported_categories_are_recognized_without_granting_authority() -> Result<(), TokenError> {
    use TokenCategory::*;
    let store = DlpTokenStore::new();
    let lease = store.admit("codex", None, 1)?;
    for category in [
        Phone,
        IdCard,
        PrivateKey,
        ApiKey,
        Database,
        Password,
        Infrastructure,
        Personal,
        Custom,
        Mixed,
    ] {
        let token = make_token(&test_key(), "fixture", category)?;
        assert!(scan_token_references(&token)? == [0..31]);
        assert!(matches!(
            lease.resolve(&token),
            Err(TokenError::ContextUnavailable)
        ));
    }
    Ok(())
}

#[test]
fn resolved_plaintext_is_borrowed_from_the_pinned_request_lease() -> Result<(), TokenError> {
    fn borrowed<'a>(
        lease: &'a super::tokenization::DlpRequestLease,
        token: &str,
    ) -> Result<&'a str, TokenError> {
        lease.resolve(token)
    }
    let store = DlpTokenStore::new();
    let mut lease = store.admit("codex", None, 1)?;
    let issued = lease.prepare(&[fragment("fixture")], &[], |tokens| Ok(tokens.to_vec()))?;
    assert!(borrowed(&lease, &issued[0])? == "fixture");
    Ok(())
}

#[test]
fn version_retry_failure_does_not_retain_uncharged_authority_capacity() -> Result<(), TokenError> {
    let store = DlpTokenStore::new();
    let mut first = store.admit("codex", Some("session-a"), 1)?;
    let mut concurrent = store.admit("codex", Some("session-a"), 1)?;
    let values: Vec<_> = (0..128).map(|index| format!("synthetic-{index}")).collect();
    let fragments: Vec<_> = values.iter().map(|value| fragment(value)).collect();
    let before = store.stats()?;
    let mut validations = 0;
    let result = first.prepare(&fragments, &[], |_| {
        validations += 1;
        if validations == 1 {
            concurrent.prepare(&[], &[], |_| Ok(()))?;
            Ok(())
        } else {
            Err(TokenError::RedactionFailed)
        }
    });
    assert!(matches!(result, Err(TokenError::RedactionFailed)));
    assert_eq!(validations, 2);
    assert_eq!(store.stats()?, before);
    assert_eq!(first.authority_capacity_for_test(), 0);
    Ok(())
}

#[test]
fn mapping_allocation_failure_does_not_retain_uncharged_authority_capacity(
) -> Result<(), TokenError> {
    for seed_existing in [false, true] {
        let store = DlpTokenStore::new();
        let mut lease = store.admit("codex", Some("session-a"), 1)?;
        let original = if seed_existing {
            lease.prepare(&[fragment("original")], &[], |tokens| Ok(tokens.to_vec()))?
        } else {
            Vec::new()
        };
        let before = store.stats()?;
        let before_capacity = lease.authority_capacity_for_test();
        let values: Vec<_> = (0..128).map(|index| format!("synthetic-{index}")).collect();
        let fragments: Vec<_> = values.iter().map(|value| fragment(value)).collect();
        store.fail_next_mapping_reserve_for_test();
        let mut rejected = Vec::new();
        let result = lease.prepare(&fragments, &[], |tokens| {
            rejected = tokens.to_vec();
            Ok(())
        });
        assert!(matches!(result, Err(TokenError::CapacityExceeded)));
        assert_eq!(store.stats()?, before);
        assert_eq!(lease.authority_capacity_for_test(), before_capacity);
        for token in &rejected {
            assert!(matches!(
                lease.resolve(token),
                Err(TokenError::ContextUnavailable)
            ));
        }
        for token in &original {
            assert!(lease.resolve(token)? == "original");
        }
        lease.prepare(&fragments, &[], |_| Ok(()))?;
        assert_eq!(store.stats()?.entries, 128 + usize::from(seed_existing));
    }
    Ok(())
}

#[test]
fn expired_namespace_index_capacity_remains_charged_until_reclaimed() -> Result<(), TokenError> {
    let store = DlpTokenStore::with_test_limits(TokenStoreLimits {
        idle_ttl: Duration::from_secs(1),
        ..Default::default()
    });
    let active = store.admit("codex", Some("active"), 1)?;
    for _ in 0..31 {
        drop(store.admit("codex", None, 1)?);
    }
    let future = Instant::now() + Duration::from_secs(60);
    store.expire_idle_at(future)?;
    assert_eq!(store.stats()?.namespaces, 1);
    assert!(store.stats()?.metadata_bytes >= store.namespace_index_bytes_for_test()?);
    let before_reuse = store.stats()?.metadata_bytes;
    drop(store.admit("codex", None, 1)?);
    store.expire_idle_at(future)?;
    assert_eq!(store.stats()?.metadata_bytes, before_reuse);
    drop(active);
    store.expire_idle_at(future)?;
    assert_eq!(store.stats()?.metadata_bytes, 0);
    assert_eq!(store.namespace_index_bytes_for_test()?, 0);
    Ok(())
}

#[test]
fn version_conflict_exhaustion_releases_all_temporary_capacity() -> Result<(), TokenError> {
    let store = DlpTokenStore::new();
    let mut first = store.admit("codex", Some("session-a"), 1)?;
    let mut concurrent = store.admit("codex", Some("session-a"), 1)?;
    let before = store.stats()?;
    let mut validations = 0;
    let result = first.prepare(&[fragment("fixture")], &[], |_| {
        validations += 1;
        concurrent.prepare(&[], &[], |_| Ok(()))
    });
    assert!(matches!(result, Err(TokenError::Unavailable)));
    assert_eq!(validations, 32);
    assert_eq!(store.stats()?, before);
    assert_eq!(first.authority_capacity_for_test(), 0);
    first.prepare(&[fragment("fixture")], &[], |_| Ok(()))?;
    assert_eq!(store.stats()?.entries, 1);
    Ok(())
}

#[test]
fn poisoned_store_rejects_all_operations_including_previously_authorized_restore(
) -> Result<(), TokenError> {
    let store = DlpTokenStore::new();
    let mut lease = store.admit("codex", None, 1)?;
    let issued = lease.prepare(&[fragment("fixture")], &[], |tokens| Ok(tokens.to_vec()))?;
    store.poison_for_test();
    assert!(matches!(store.stats(), Err(TokenError::Unavailable)));
    assert!(matches!(
        store.admit("codex", None, 1),
        Err(TokenError::Unavailable)
    ));
    assert!(matches!(
        store.expire_idle_at(Instant::now()),
        Err(TokenError::Unavailable)
    ));
    assert!(matches!(
        lease.prepare(&[fragment("another")], &[], |_| Ok(())),
        Err(TokenError::Unavailable)
    ));
    assert!(matches!(
        lease.resolve(&issued[0]),
        Err(TokenError::Unavailable)
    ));
    Ok(())
}

#[test]
fn lease_drop_releases_authority_before_unpinning_or_releasing_its_budget() -> Result<(), TokenError>
{
    use std::sync::{atomic::AtomicUsize, atomic::Ordering, Arc};
    let store = DlpTokenStore::new();
    let mut lease = store.admit("codex", None, 1)?;
    lease.prepare(&[fragment("fixture")], &[], |_| Ok(()))?;
    let observed = Arc::new(AtomicUsize::new(usize::MAX));
    lease.observe_drop_authority_capacity_for_test(observed.clone());
    drop(lease);
    assert_eq!(observed.load(Ordering::SeqCst), 0);
    store.expire_idle_at(Instant::now() + Duration::from_secs(3600))?;
    assert_eq!(store.stats()?.metadata_bytes, 0);
    assert_eq!(store.stats()?.plaintext_bytes, 0);
    Ok(())
}

#[test]
fn review_token_scanner_rejects_reference_overflow_before_later_malformed_input(
) -> Result<(), TokenError> {
    let token = make_token(&[0; 32], "synthetic", TokenCategory::Custom)?;
    let boundary = token.repeat(4096);
    assert_eq!(scan_token_references(&boundary)?.len(), 4096);
    let overflow = format!("{boundary}{token} vary_ph_invalid");
    assert_eq!(
        scan_token_references(&overflow).err(),
        Some(TokenError::CapacityExceeded)
    );
    Ok(())
}

#[test]
fn reference_visitor_reports_utf8_spans_without_a_result_container() -> Result<(), TokenError> {
    use super::tokenization::visit_token_references;
    let token = make_token(&test_key(), "synthetic-visitor", TokenCategory::Custom)?;
    let text = format!("中{token}{token}🦀");
    let mut ranges = [0..0, 0..0];
    let mut count = 0;
    visit_token_references(&text, |range| {
        assert!(count < 2);
        ranges[count] = range;
        count += 1;
        Ok(())
    })?;
    assert_eq!(count, 2);
    assert_eq!(ranges, [3..34, 34..65]);
    assert!(ranges
        .iter()
        .all(|range| text.get(range.clone()) == Some(token.as_str())));
    visit_token_references("vary_identifier vary_zz_bad_ai vary_ph", |_| {
        panic!("unrecognized header must not visit")
    })?;
    Ok(())
}

#[test]
fn reference_visitor_preserves_fixed_malformed_errors_and_stops_at_4097() -> Result<(), TokenError>
{
    use super::tokenization::visit_token_references;
    for text in [
        "vary_ph_",
        "vary_ph_413FF0A68F6F0D574CB5_ai",
        "vary_ph_413ff0a68f6f0d574cb5_xx",
    ] {
        let mut count = 0;
        assert_eq!(
            visit_token_references(text, |_| {
                count += 1;
                Ok(())
            })
            .err(),
            Some(TokenError::ContextUnavailable)
        );
        assert_eq!(count, 0);
    }
    let token = make_token(&test_key(), "synthetic-visitor", TokenCategory::Custom)?;
    for (n, expected) in [(4096, None), (4097, Some(TokenError::CapacityExceeded))] {
        let text = token.repeat(n);
        let mut count = 0;
        assert_eq!(
            visit_token_references(&text, |_| {
                count += 1;
                Ok(())
            })
            .err(),
            expected
        );
        assert_eq!(count, 4096);
    }
    Ok(())
}

#[test]
fn reference_visitor_propagates_callback_failure_without_scanning_later_text(
) -> Result<(), TokenError> {
    use super::tokenization::visit_token_references;
    let token = make_token(&test_key(), "synthetic-visitor", TokenCategory::Custom)?;
    let text = format!("{token}vary_ph_");
    let mut count = 0;
    assert_eq!(
        visit_token_references(&text, |_| {
            count += 1;
            Err(TokenError::TokenCollision)
        })
        .err(),
        Some(TokenError::TokenCollision)
    );
    assert_eq!(count, 1);
    Ok(())
}

#[test]
fn authority_visitor_exposes_only_lease_dependencies_and_runs_without_store_lock(
) -> Result<(), TokenError> {
    let store = DlpTokenStore::new();
    let mut first = store.admit("codex", Some("synthetic-visitor-session"), 1)?;
    let empty = store.admit("codex", Some("synthetic-visitor-session"), 1)?;
    empty
        .visit_authorized_tokens(|_| panic!("empty lease must not inherit namespace authority"))?;
    let tokens = first.prepare(
        &[fragment("synthetic-first"), fragment("synthetic-second")],
        &[],
        |tokens| Ok(tokens.to_vec()),
    )?;
    let mut borrowed = [""; 2];
    let mut count = 0;
    first.visit_authorized_tokens(|token| {
        assert!(count < 2);
        // stats locks the store; successful reentry proves the visitor holds no lock.
        assert_eq!(store.stats()?.entries, 2);
        assert!(first.resolve(token).is_ok());
        borrowed[count] = token;
        count += 1;
        Ok(())
    })?;
    assert_eq!(count, 2);
    assert!(tokens
        .iter()
        .all(|token| borrowed.contains(&token.as_str())));
    empty.visit_authorized_tokens(|_| panic!("namespace membership must remain insufficient"))?;
    let mut count = 0;
    assert_eq!(
        first
            .visit_authorized_tokens(|_| {
                count += 1;
                Err(TokenError::RedactionFailed)
            })
            .err(),
        Some(TokenError::RedactionFailed)
    );
    assert_eq!(count, 1);
    Ok(())
}

#[test]
fn identity_namespace_retains_only_complete_fixed_fingerprints() -> Result<(), TokenError> {
    let store = DlpTokenStore::new();
    let _lease = store.admit("synthetic-app", Some("synthetic-session"), 1)?;
    assert_eq!(store.identity_storage_for_test()?, (0, 32));
    Ok(())
}

#[test]
fn identity_work_is_observed_outside_store_state_lock() -> Result<(), TokenError> {
    use super::tokenization::identity_test_support::{self, Phase};
    use std::{cell::Cell, rc::Rc};
    let calls = Rc::new(Cell::new(0));
    let observed = calls.clone();
    let _hook = identity_test_support::install(move |phase, unlocked| {
        if phase == Phase::IdentityWork {
            observed.set(observed.get() + 1);
            assert!(
                unlocked,
                "identity hashing/comparison must not hold the state lock"
            );
        }
        Ok(())
    });
    let store = DlpTokenStore::new();
    let _first = store.admit("codex", Some("synthetic-session"), 1)?;
    let _second = store.admit("codex", Some("synthetic-session"), 1)?;
    assert_eq!(calls.get(), 2);
    Ok(())
}

#[test]
fn identity_digest_uses_full_domain_separated_length_framed_exact_utf8() -> Result<(), TokenError> {
    use super::tokenization::namespace_identity_digest;
    let key = test_key();
    // Independent HMAC fixture generated from the documented byte encoding.
    let expected: [u8; 32] = [
        71, 227, 234, 116, 221, 233, 15, 138, 58, 87, 90, 116, 218, 229, 81, 78, 155, 177, 72, 205,
        198, 199, 211, 183, 122, 175, 48, 30, 24, 135, 77, 84,
    ];
    assert!(namespace_identity_digest(&key, "codex", "synthetic-session")? == expected);
    for (app, session) in [
        ("co", "dexsynthetic-session"),
        ("codex", "synthetic-session "),
        ("codex", "SYNTHETIC-session"),
    ] {
        assert!(namespace_identity_digest(&key, app, session)? != expected);
    }
    assert!(
        namespace_identity_digest(&key, "ab", "c")? != namespace_identity_digest(&key, "a", "bc")?
    );
    assert!(
        namespace_identity_digest(&key, "codex", "é")?
            != namespace_identity_digest(&key, "codex", "e\u{0301}")?
    );
    assert!(
        namespace_identity_digest(&key, "codex", "synthetic-session")?
            != namespace_identity_digest(&[42; 32], "codex", "synthetic-session")?
    );
    Ok(())
}

#[test]
fn identity_key_is_stable_shared_by_clones_and_independent_of_token_test_key(
) -> Result<(), TokenError> {
    let first = DlpTokenStore::with_test_key(test_key());
    let second = DlpTokenStore::with_test_key(test_key());
    let fingerprint = first.identity_fingerprint_for_test("codex", "synthetic-session")?;
    assert!(
        fingerprint
            == first
                .clone()
                .identity_fingerprint_for_test("codex", "synthetic-session")?
    );
    assert!(fingerprint != second.identity_fingerprint_for_test("codex", "synthetic-session")?);
    assert!(
        fingerprint
            != super::tokenization::namespace_identity_digest(
                &test_key(),
                "codex",
                "synthetic-session"
            )?
    );
    assert!(fingerprint != first.identity_fingerprint_for_test("other-app", "synthetic-session")?);
    assert!(fingerprint != first.identity_fingerprint_for_test("codex", "other-session")?);
    Ok(())
}

#[test]
fn identity_entropy_failure_is_sticky_and_admission_leaves_no_state() -> Result<(), TokenError> {
    use super::tokenization::identity_test_support::{self, Phase};
    use std::{cell::Cell, rc::Rc};
    let calls = Rc::new(Cell::new(0));
    let observed = calls.clone();
    let store = DlpTokenStore::new();
    let _hook = identity_test_support::install(move |phase, unlocked| {
        if phase == Phase::Entropy {
            assert!(unlocked);
            observed.set(observed.get() + 1);
            return Err(TokenError::Unavailable);
        }
        Ok(())
    });
    let before = store.stats()?;
    assert_eq!(
        store.admit("codex", Some("synthetic-session"), 1).err(),
        Some(TokenError::Unavailable)
    );
    drop(_hook);
    assert_eq!(
        store
            .clone()
            .admit("codex", Some("synthetic-session"), 1)
            .err(),
        Some(TokenError::Unavailable)
    );
    assert_eq!(
        store.admit("codex", None, 1).err(),
        Some(TokenError::Unavailable)
    );
    assert_eq!(calls.get(), 1);
    assert_eq!(store.stats()?, before);
    Ok(())
}

#[test]
fn identity_namespace_source_guard_excludes_original_identity_fields_and_copies() {
    // A structural guard, not a process-memory or zeroization assertion.
    let source = include_str!("tokenization.rs");
    let namespace = source
        .split_once("struct Namespace {")
        .unwrap()
        .1
        .split_once("\n}")
        .unwrap()
        .0;
    assert!(namespace.contains("identity: Option<[u8; 32]>"));
    for forbidden in ["app:", "session:", "String", "Arc<str>", "Vec<"] {
        assert!(!namespace.contains(forbidden));
    }
    let admission = source
        .split_once("pub(super) fn admit(")
        .unwrap()
        .1
        .split_once("pub(super) fn expire_idle_at")
        .unwrap()
        .0;
    for forbidden in [
        "namespace.app",
        "namespace.session",
        "app.to_owned",
        "session.map(str::to_owned)",
    ] {
        assert!(!admission.contains(forbidden));
    }
}

#[test]
fn identity_concurrent_first_admission_initializes_one_key_and_one_namespace(
) -> Result<(), TokenError> {
    use super::tokenization::identity_test_support::{self, Phase};
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Barrier,
    };
    const WORKERS: usize = 16;
    let store = DlpTokenStore::new();
    let start = Arc::new(Barrier::new(WORKERS + 1));
    let entropy = Arc::new(AtomicUsize::new(0));
    let hashes = Arc::new(AtomicUsize::new(0));
    let mut workers = Vec::new();
    for _ in 0..WORKERS {
        let (store, start, entropy, hashes) = (
            store.clone(),
            start.clone(),
            entropy.clone(),
            hashes.clone(),
        );
        workers.push(std::thread::spawn(move || {
            let _hook = identity_test_support::install(move |phase, _| {
                // Another worker may own the state lock; try_lock is not a
                // current-thread ownership observation under contention.
                match phase {
                    Phase::Entropy => {
                        entropy.fetch_add(1, Ordering::SeqCst);
                    }
                    Phase::IdentityWork => {
                        hashes.fetch_add(1, Ordering::SeqCst);
                    }
                }
                Ok(())
            });
            start.wait();
            store.admit("codex", Some("synthetic-concurrent-identity"), 7)
        }));
    }
    start.wait();
    let mut leases = Vec::new();
    for worker in workers {
        leases.push(worker.join().map_err(|_| TokenError::Unavailable)??);
    }
    assert_eq!(entropy.load(Ordering::SeqCst), 1);
    assert_eq!(hashes.load(Ordering::SeqCst), WORKERS);
    assert_eq!(store.stats()?.namespaces, 1);
    assert_eq!(
        store.stats()?.metadata_bytes,
        1024 + "codex".len() + "synthetic-concurrent-identity".len() + 256 * WORKERS
    );
    let issued = leases[0].prepare(
        &[fragment("synthetic-concurrent-plaintext")],
        &[],
        |tokens| Ok(tokens.to_vec()),
    )?;
    for lease in &mut leases[1..] {
        assert_eq!(
            lease.resolve(&issued[0]).err(),
            Some(TokenError::ContextUnavailable)
        );
        let reused = lease.prepare(
            &[fragment("synthetic-concurrent-plaintext")],
            &[],
            |tokens| Ok(tokens.to_vec()),
        )?;
        assert!(reused == issued);
    }
    assert_eq!(store.stats()?.entries, 1);
    Ok(())
}

#[test]
fn identity_concurrent_entropy_error_is_published_once_and_closes_all_admissions(
) -> Result<(), TokenError> {
    use super::tokenization::identity_test_support::{self, Phase};
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Barrier,
    };
    const WORKERS: usize = 16;
    let store = DlpTokenStore::new();
    let start = Arc::new(Barrier::new(WORKERS + 1));
    let entropy = Arc::new(AtomicUsize::new(0));
    let mut workers = Vec::new();
    for index in 0..WORKERS {
        let (store, start, entropy) = (store.clone(), start.clone(), entropy.clone());
        workers.push(std::thread::spawn(move || {
            let _hook = identity_test_support::install(move |phase, _| {
                assert!(
                    phase == Phase::Entropy,
                    "failed initialization must precede hashing"
                );
                entropy.fetch_add(1, Ordering::SeqCst);
                Err(TokenError::Unavailable)
            });
            start.wait();
            store
                .admit(
                    "codex",
                    (index % 2 == 0).then_some("synthetic-concurrent-failure"),
                    1,
                )
                .err()
        }));
    }
    start.wait();
    for worker in workers {
        assert_eq!(
            worker.join().map_err(|_| TokenError::Unavailable)?,
            Some(TokenError::Unavailable)
        );
    }
    assert_eq!(entropy.load(Ordering::SeqCst), 1);
    assert_eq!(store.stats()?, Default::default());
    assert_eq!(
        store
            .clone()
            .admit("codex", Some("synthetic-later"), 2)
            .err(),
        Some(TokenError::Unavailable)
    );
    assert_eq!(
        store.admit("codex", None, 1).err(),
        Some(TokenError::Unavailable)
    );
    Ok(())
}

#[test]
fn identity_actual_admission_preserves_utf8_boundaries_framing_and_policy_selection(
) -> Result<(), TokenError> {
    let store = DlpTokenStore::new();
    let identities = [
        ("é".repeat(128), "🦀".repeat(64), 1),
        ("codex".into(), "é".into(), 1),
        ("codex".into(), "e\u{0301}".into(), 1),
        ("Codex".into(), "é".into(), 1),
        ("codex".into(), "É".into(), 1),
        ("codex".into(), "é".into(), 2),
        ("ab".into(), "c".into(), 1),
        ("a".into(), "bc".into(), 1),
    ];
    let mut previous: Vec<String> = Vec::new();
    for (index, (app, session, revision)) in identities.iter().enumerate() {
        let mut first = store.admit(app, Some(session), *revision)?;
        let issued = first.prepare(&[fragment("synthetic-boundary")], &[], |tokens| {
            Ok(tokens.to_vec())
        })?;
        for prior in &previous {
            assert_eq!(
                first.resolve(prior).err(),
                Some(TokenError::ContextUnavailable)
            );
        }
        let mut second = store.clone().admit(app, Some(session), *revision)?;
        assert_eq!(
            second.resolve(&issued[0]).err(),
            Some(TokenError::ContextUnavailable)
        );
        assert!(
            second.prepare(&[fragment("synthetic-boundary")], &[], |tokens| Ok(
                tokens.to_vec()
            ))? == issued
        );
        assert_eq!(store.stats()?.namespaces, index + 1);
        assert_eq!(store.stats()?.entries, index + 1);
        previous.push(issued[0].clone());
    }
    let before = store.stats()?;
    for invalid_app in [String::new(), "é".repeat(128) + "x"] {
        assert_eq!(
            store.admit(&invalid_app, Some("synthetic-valid"), 1).err(),
            Some(TokenError::ContextUnavailable)
        );
        assert_eq!(store.stats()?, before);
    }
    Ok(())
}

#[test]
fn identity_invalid_utf8_length_whitespace_and_control_sessions_remain_independent(
) -> Result<(), TokenError> {
    let store = DlpTokenStore::new();
    let over_bytes = "é".repeat(128) + "x";
    let invalid = [
        None,
        Some(""),
        Some("has space"),
        Some("has\ncontrol"),
        Some("has\u{0085}space"),
        Some("has\u{2003}space"),
        Some("has\u{0000}control"),
        Some(over_bytes.as_str()),
    ];
    for (index, session) in invalid.into_iter().enumerate() {
        let mut first = store.admit("codex", session, 1)?;
        let issued = first.prepare(&[fragment("synthetic-anonymous")], &[], |tokens| {
            Ok(tokens.to_vec())
        })?;
        let second = store.admit("codex", session, 1)?;
        assert_eq!(
            second.resolve(&issued[0]).err(),
            Some(TokenError::ContextUnavailable)
        );
        assert_eq!(store.stats()?.namespaces, (index + 1) * 2);
    }
    Ok(())
}

#[test]
fn identity_admission_preserves_exact_conservative_metadata_and_failure_atomicity(
) -> Result<(), TokenError> {
    let (app, session) = ("é-app", "synthetic-session");
    let exact = 1024 + app.len() + session.len() + 256;
    let denied = DlpTokenStore::with_test_limits(TokenStoreLimits {
        global_metadata_bytes: exact - 1,
        ..Default::default()
    });
    assert_eq!(
        denied.admit(app, Some(session), 1).err(),
        Some(TokenError::CapacityExceeded)
    );
    assert_eq!(denied.stats()?, Default::default());
    let store = DlpTokenStore::with_test_limits(TokenStoreLimits {
        global_metadata_bytes: exact,
        ..Default::default()
    });
    let first = store.admit(app, Some(session), 1)?;
    assert_eq!(store.stats()?.metadata_bytes, exact);
    let before = store.stats()?;
    for (other_app, other_session, revision) in [
        (app, session, 1),
        (app, "other-session", 1),
        ("other-app", session, 1),
        (app, session, 2),
    ] {
        assert_eq!(
            store.admit(other_app, Some(other_session), revision).err(),
            Some(TokenError::CapacityExceeded)
        );
        assert_eq!(store.stats()?, before);
    }
    drop(first);
    assert_eq!(store.stats()?.metadata_bytes, exact - 256);
    let second = store.admit(app, Some(session), 1)?;
    assert_eq!(store.stats()?, before);
    drop(second);
    store.expire_idle_at(Instant::now() + Duration::from_secs(3600))?;
    assert_eq!(store.stats()?, Default::default());
    let anonymous = store.admit(app, Some("invalid session"), 1)?;
    assert_eq!(store.stats()?.metadata_bytes, 1024 + app.len() + 256);
    drop(anonymous);
    Ok(())
}

#[test]
fn identity_namespace_capacity_failures_do_not_add_leases_or_widen_authority(
) -> Result<(), TokenError> {
    let store = DlpTokenStore::with_test_limits(TokenStoreLimits {
        namespaces: 1,
        ..Default::default()
    });
    let mut first = store.admit("codex", Some("synthetic-capacity"), 1)?;
    let issued = first.prepare(&[fragment("synthetic-pinned")], &[], |tokens| {
        Ok(tokens.to_vec())
    })?;
    let before = store.stats()?;
    for (app, session, revision) in [
        ("other-app", Some("synthetic-capacity"), 1),
        ("codex", Some("other-session"), 1),
        ("codex", Some("synthetic-capacity"), 2),
        ("codex", None, 1),
    ] {
        assert_eq!(
            store.admit(app, session, revision).err(),
            Some(TokenError::CapacityExceeded)
        );
        assert_eq!(store.stats()?, before);
        assert!(first.resolve(&issued[0])? == "synthetic-pinned");
    }
    let second = store.admit("codex", Some("synthetic-capacity"), 1)?;
    assert_eq!(
        second.resolve(&issued[0]).err(),
        Some(TokenError::ContextUnavailable)
    );
    drop(second);
    drop(first);
    store.expire_idle_at(Instant::now() + Duration::from_secs(3600))?;
    assert_eq!(store.stats()?, Default::default());
    Ok(())
}

#[test]
fn identity_ttl_rotates_generation_without_changing_stable_identity_or_inheriting_authority(
) -> Result<(), TokenError> {
    let store = DlpTokenStore::with_test_limits(TokenStoreLimits {
        namespaces: 1,
        idle_ttl: Duration::from_secs(1),
        ..Default::default()
    });
    let fingerprint = store.identity_fingerprint_for_test("codex", "synthetic-ttl")?;
    let mut first = store.admit("codex", Some("synthetic-ttl"), 1)?;
    let issued = first.prepare(&[fragment("synthetic-ttl-plaintext")], &[], |tokens| {
        Ok(tokens.to_vec())
    })?;
    let future = Instant::now() + Duration::from_secs(60);
    store.expire_idle_at(future)?;
    assert!(first.resolve(&issued[0])? == "synthetic-ttl-plaintext");
    drop(first);
    store.expire_idle_at(future)?;
    assert_eq!(store.stats()?, Default::default());
    assert!(fingerprint == store.identity_fingerprint_for_test("codex", "synthetic-ttl")?);
    let mut replacement = store.admit("codex", Some("synthetic-ttl"), 1)?;
    assert_eq!(
        replacement.resolve(&issued[0]).err(),
        Some(TokenError::ContextUnavailable)
    );
    let reissued = replacement.prepare(&[fragment("synthetic-ttl-plaintext")], &[], |tokens| {
        Ok(tokens.to_vec())
    })?;
    assert!(reissued != issued);
    Ok(())
}

#[test]
fn identity_initializer_unwind_publishes_neither_key_nor_namespace() -> Result<(), TokenError> {
    use super::tokenization::identity_test_support::{self, Phase};
    let store = DlpTokenStore::new();
    let hook = identity_test_support::install(|phase, unlocked| {
        assert!(phase == Phase::Entropy && unlocked);
        panic!("synthetic identity initialization interruption");
    });
    let interrupted = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        store.admit("codex", Some("synthetic-interruption"), 1)
    }));
    assert!(interrupted.is_err());
    assert_eq!(store.stats()?, Default::default());
    drop(hook);
    // OnceLock retries only an unpublished initializer, not a stored Err or key.
    let first = store.admit("codex", Some("synthetic-interruption"), 1)?;
    let before = store.identity_fingerprint_for_test("codex", "synthetic-interruption")?;
    let second = store
        .clone()
        .admit("codex", Some("synthetic-interruption"), 1)?;
    assert!(before == store.identity_fingerprint_for_test("codex", "synthetic-interruption")?);
    assert_eq!(store.stats()?.namespaces, 1);
    drop((first, second));
    Ok(())
}

#[test]
fn identity_poisoned_admission_fails_closed_without_new_leases_or_authority(
) -> Result<(), TokenError> {
    let store = DlpTokenStore::new();
    let mut lease = store.admit("codex", Some("synthetic-poison"), 1)?;
    let issued = lease.prepare(&[fragment("synthetic-poison-plaintext")], &[], |tokens| {
        Ok(tokens.to_vec())
    })?;
    let before = store.admission_counts_for_test();
    store.poison_for_test();
    for (app, session, revision) in [
        ("codex", Some("synthetic-poison"), 1),
        ("other-app", Some("synthetic-poison"), 1),
        ("codex", None, 2),
    ] {
        assert_eq!(
            store.admit(app, session, revision).err(),
            Some(TokenError::Unavailable)
        );
        assert_eq!(store.admission_counts_for_test(), before);
    }
    assert_eq!(
        lease.resolve(&issued[0]).err(),
        Some(TokenError::Unavailable)
    );
    Ok(())
}

#[test]
fn identity_hash_unwind_preserves_published_key_namespace_and_existing_authority(
) -> Result<(), TokenError> {
    use super::tokenization::identity_test_support::{self, Phase};
    let store = DlpTokenStore::new();
    let mut first = store.admit("codex", Some("synthetic-hash-interruption"), 1)?;
    let fingerprint =
        store.identity_fingerprint_for_test("codex", "synthetic-hash-interruption")?;
    let issued = first.prepare(&[fragment("synthetic-existing-authority")], &[], |tokens| {
        Ok(tokens.to_vec())
    })?;
    let before = store.stats()?;
    let hook = identity_test_support::install(|phase, unlocked| {
        assert!(phase == Phase::IdentityWork && unlocked);
        panic!("synthetic identity hash interruption");
    });
    let interrupted = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        store.admit("codex", Some("synthetic-hash-interruption"), 1)
    }));
    assert!(interrupted.is_err());
    drop(hook);
    assert_eq!(store.stats()?, before);
    assert!(
        fingerprint
            == store.identity_fingerprint_for_test("codex", "synthetic-hash-interruption")?
    );
    assert!(first.resolve(&issued[0])? == "synthetic-existing-authority");
    let second = store.admit("codex", Some("synthetic-hash-interruption"), 1)?;
    assert_eq!(store.stats()?.namespaces, 1);
    assert_eq!(
        second.resolve(&issued[0]).err(),
        Some(TokenError::ContextUnavailable)
    );
    Ok(())
}

#[test]
fn preparation_scratch_zero_budget_rejects_before_callback_and_preserves_authority(
) -> Result<(), TokenError> {
    use super::payload_budget::PayloadBudget;
    let store = DlpTokenStore::new();
    let mut lease = store.admit("codex", Some("synthetic-scratch"), 1)?;
    let prior = lease.prepare(&[fragment("synthetic-pinned")], &[], |tokens| {
        Ok(tokens.to_vec())
    })?;
    let before = store.stats()?;
    let budget = PayloadBudget::with_limit(0);
    let mut called = false;
    let result =
        lease.prepare_with_budget(&[fragment("synthetic-new")], &[], budget.clone(), |_| {
            called = true;
            Ok(())
        });
    assert_eq!(result.err(), Some(TokenError::CapacityExceeded));
    assert!(!called);
    assert_eq!(store.stats()?, before);
    assert!(lease.resolve(&prior[0])? == "synthetic-pinned");
    assert_eq!(budget.used(), 0);
    Ok(())
}

#[test]
fn preparation_scratch_digest_capacity_difference_one_rejects_atomically() -> Result<(), TokenError>
{
    use super::payload_budget::PayloadBudget;
    let store = DlpTokenStore::new();
    let mut lease = store.admit("codex", None, 1)?;
    let before = store.stats()?;
    let budget = PayloadBudget::with_limit(4 * std::mem::size_of::<[u8; 10]>() - 1);
    let mut called = false;
    let result = lease.prepare_with_budget(
        &[fragment("synthetic-digest-capacity")],
        &[],
        budget.clone(),
        |_| {
            called = true;
            Ok(())
        },
    );
    assert_eq!(result.err(), Some(TokenError::CapacityExceeded));
    assert!(!called);
    assert_eq!(store.stats()?, before);
    assert_eq!(budget.used(), 0);
    Ok(())
}

#[test]
fn preparation_scratch_existing_snapshot_capacity_is_charged_alongside_digests(
) -> Result<(), TokenError> {
    use super::payload_budget::PayloadBudget;
    let store = DlpTokenStore::new();
    let mut lease = store.admit("codex", None, 1)?;
    let before = store.stats()?;
    let digest_bytes = 4 * std::mem::size_of::<[u8; 10]>();
    let existing_bytes =
        4 * std::mem::size_of::<Option<super::token_store_owners::SharedAllocation<()>>>();
    let budget = PayloadBudget::with_limit(digest_bytes + existing_bytes - 1);
    let mut called = false;
    let result = lease.prepare_with_budget(
        &[fragment("synthetic-snapshot-capacity")],
        &[],
        budget.clone(),
        |_| {
            called = true;
            Ok(())
        },
    );
    assert_eq!(result.err(), Some(TokenError::CapacityExceeded));
    assert!(!called);
    assert_eq!(store.stats()?, before);
    assert_eq!(budget.used(), 0);
    Ok(())
}

fn scratch_snapshot_bytes() -> usize {
    4 * std::mem::size_of::<[u8; 10]>()
        + 4 * std::mem::size_of::<Option<super::token_store_owners::SharedAllocation<()>>>()
}

#[test]
fn preparation_scratch_ordered_indices_difference_one_rejects_before_callback(
) -> Result<(), TokenError> {
    use super::payload_budget::PayloadBudget;
    let store = DlpTokenStore::new();
    let mut lease = store.admit("codex", None, 1)?;
    let before = store.stats()?;
    let budget =
        PayloadBudget::with_limit(scratch_snapshot_bytes() + 4 * std::mem::size_of::<usize>() - 1);
    let mut called = false;
    assert_eq!(
        lease
            .prepare_with_budget(&[fragment("synthetic-index")], &[], budget.clone(), |_| {
                called = true;
                Ok(())
            })
            .err(),
        Some(TokenError::CapacityExceeded)
    );
    assert!(!called);
    assert_eq!(store.stats()?, before);
    assert_eq!(budget.used(), 0);
    Ok(())
}

#[test]
fn preparation_scratch_new_indices_coexist_with_ordered_indices_and_snapshot(
) -> Result<(), TokenError> {
    use super::payload_budget::PayloadBudget;
    let store = DlpTokenStore::new();
    let mut lease = store.admit("codex", None, 1)?;
    let before = store.stats()?;
    let budget =
        PayloadBudget::with_limit(scratch_snapshot_bytes() + 8 * std::mem::size_of::<usize>() - 1);
    let mut called = false;
    assert_eq!(
        lease
            .prepare_with_budget(
                &[fragment("synthetic-new-index")],
                &[],
                budget.clone(),
                |_| {
                    called = true;
                    Ok(())
                }
            )
            .err(),
        Some(TokenError::CapacityExceeded)
    );
    assert!(!called);
    assert_eq!(store.stats()?, before);
    assert_eq!(budget.used(), 0);
    Ok(())
}

fn scratch_indices_bytes() -> usize {
    scratch_snapshot_bytes() + 8 * std::mem::size_of::<usize>()
}

#[test]
fn preparation_scratch_token_text_and_outer_capacity_difference_one_reject(
) -> Result<(), TokenError> {
    use super::{payload_budget::PayloadBudget, payload_text::PayloadText};
    let exact = scratch_indices_bytes() + 31 + 4 * std::mem::size_of::<PayloadText>();
    for limit in [scratch_indices_bytes() + 30, exact - 1] {
        let store = DlpTokenStore::new();
        let mut lease = store.admit("codex", None, 1)?;
        let before = store.stats()?;
        let budget = PayloadBudget::with_limit(limit);
        let mut called = false;
        assert_eq!(
            lease
                .prepare_with_budget(
                    &[fragment("synthetic-token-owner")],
                    &[],
                    budget.clone(),
                    |_| {
                        called = true;
                        Ok(())
                    }
                )
                .err(),
            Some(TokenError::CapacityExceeded)
        );
        assert!(!called);
        assert_eq!(store.stats()?, before);
        assert_eq!(budget.used(), 0);
    }
    Ok(())
}

#[test]
fn preparation_scratch_exact_all_owners_live_through_callback_then_release(
) -> Result<(), TokenError> {
    use super::{payload_budget::PayloadBudget, payload_text::PayloadText};
    let exact = scratch_indices_bytes() + 31 + 4 * std::mem::size_of::<PayloadText>();
    let store = DlpTokenStore::new();
    let mut lease = store.admit("codex", None, 1)?;
    let budget = PayloadBudget::with_limit(exact);
    lease.prepare_with_budget(
        &[fragment("synthetic-token-owner")],
        &[],
        budget.clone(),
        |tokens| {
            assert_eq!(budget.used(), exact);
            assert_eq!(tokens.len(), 1);
            assert_eq!(tokens[0].as_str().len(), 31);
            assert_eq!(store.stats()?.entries, 0);
            Ok(())
        },
    )?;
    assert_eq!(budget.used(), 0);
    assert_eq!(store.stats()?.entries, 1);
    Ok(())
}

// Independent live-allocation model for the prevalidation scratch sequence.
// This deliberately excludes postvalidation Arc/table backing and store accounting.
fn scratch_profile(count: usize, new_count: usize) -> (usize, usize) {
    use super::payload_text::PayloadText;
    fn push(used: &mut usize, peak: &mut usize, len: usize, cap: &mut usize, width: usize) {
        if len == *cap {
            let next = (*cap * 2).max(4);
            *peak = (*peak).max(*used + next * width);
            *used += (next - *cap) * width;
            *cap = next;
        }
    }
    let mut used = 0;
    let mut peak = 0;
    for width in [
        std::mem::size_of::<[u8; 10]>(),
        std::mem::size_of::<Option<super::token_store_owners::SharedAllocation<()>>>(),
        std::mem::size_of::<usize>(),
    ] {
        let mut cap = 0;
        for len in 0..count {
            push(&mut used, &mut peak, len, &mut cap, width);
        }
    }
    let mut indices_cap = 0;
    let mut token_cap = 0;
    for len in 0..count {
        if len < new_count {
            push(
                &mut used,
                &mut peak,
                len,
                &mut indices_cap,
                std::mem::size_of::<usize>(),
            );
        }
        used += 31;
        peak = peak.max(used);
        push(
            &mut used,
            &mut peak,
            len,
            &mut token_cap,
            std::mem::size_of::<PayloadText>(),
        );
    }
    (used, peak)
}

#[test]
fn preparation_scratch_growth_exact_peaks_and_difference_one_are_atomic() -> Result<(), TokenError>
{
    use super::payload_budget::PayloadBudget;
    for count in [1, 4, 5, 8, 9, 4096] {
        let raw: Vec<_> = (0..count)
            .map(|i| format!("synthetic-growth-{i}"))
            .collect();
        let fragments: Vec<_> = raw.iter().map(|text| fragment(text)).collect();
        let (retained, peak) = scratch_profile(count, count);
        for limit in [peak - 1, peak] {
            let store = DlpTokenStore::new();
            let mut lease = store.admit("codex", None, 1)?;
            let before = store.stats()?;
            let budget = PayloadBudget::with_limit(limit);
            let mut called = false;
            let result = lease.prepare_with_budget(&fragments, &[], budget.clone(), |tokens| {
                called = true;
                assert_eq!(tokens.len(), count);
                assert_eq!(budget.used(), retained);
                assert_eq!(store.stats()?, before);
                Ok(())
            });
            if limit == peak {
                result?;
                assert!(called);
                assert_eq!(store.stats()?.entries, count);
                assert_eq!(lease.authorized_tokens()?.len(), count);
            } else {
                assert_eq!(result.err(), Some(TokenError::CapacityExceeded));
                assert!(!called);
                assert_eq!(store.stats()?, before);
                assert!(lease.authorized_tokens()?.is_empty());
            }
            assert_eq!(budget.used(), 0);
        }
    }
    let store = DlpTokenStore::new();
    let mut lease = store.admit("codex", None, 1)?;
    let fragments: Vec<_> = (0..4097).map(|_| fragment("synthetic-boundary")).collect();
    let budget = PayloadBudget::with_limit(0);
    let before = store.stats()?;
    assert_eq!(
        lease
            .prepare_with_budget(
                &fragments,
                &[],
                budget.clone(),
                |_| -> Result<(), TokenError> {
                    panic!("over-limit fragments must not reach validation")
                }
            )
            .err(),
        Some(TokenError::CapacityExceeded)
    );
    assert_eq!(budget.used(), 0);
    assert_eq!(store.stats()?, before);
    Ok(())
}

#[test]
fn preparation_scratch_repeated_and_existing_tokens_keep_distinct_owners_and_first_category(
) -> Result<(), TokenError> {
    use super::payload_budget::PayloadBudget;
    let store = DlpTokenStore::new();
    let mut lease = store.admit("codex", Some("synthetic-repeated"), 1)?;
    let raw = "synthetic-repeated-value";
    let fragments = [
        TokenFragment {
            raw,
            whole_span_categories: &[TokenCategory::Phone],
        },
        TokenFragment {
            raw,
            whole_span_categories: &[TokenCategory::PrivateKey],
        },
        TokenFragment {
            raw,
            whole_span_categories: &[TokenCategory::ApiKey],
        },
        TokenFragment {
            raw,
            whole_span_categories: &[TokenCategory::Custom],
        },
        TokenFragment {
            raw,
            whole_span_categories: &[],
        },
    ];
    let mut first_token = String::new();
    for new_count in [1, 0] {
        let (retained, peak) = scratch_profile(fragments.len(), new_count);
        let budget = PayloadBudget::with_limit(peak);
        lease.prepare_with_budget(&fragments, &[], budget.clone(), |tokens| {
            assert_eq!(budget.used(), retained);
            assert!(tokens[0].as_str().starts_with("vary_ph_"));
            for token in &tokens[1..] {
                assert!(token.as_str() == tokens[0].as_str());
                assert!(token.as_str().as_ptr() != tokens[0].as_str().as_ptr());
            }
            if first_token.is_empty() {
                first_token = tokens[0].as_str().to_owned();
            } else {
                assert!(first_token == tokens[0].as_str());
            }
            Ok(())
        })?;
        assert_eq!(budget.used(), 0);
        assert_eq!(store.stats()?.entries, 1);
        assert_eq!(lease.authorized_tokens()?.len(), 1);
    }
    assert!(lease.resolve(&first_token)? == raw);
    Ok(())
}

#[test]
fn preparation_scratch_shared_budget_denial_preserves_existing_authority() -> Result<(), TokenError>
{
    use super::payload_budget::PayloadBudget;
    let store = DlpTokenStore::new();
    let mut lease = store.admit("codex", None, 1)?;
    let old = lease.prepare(&[fragment("synthetic-pinned")], &[], |tokens| {
        Ok(tokens[0].clone())
    })?;
    let before = store.stats()?;
    let (_, peak) = scratch_profile(1, 1);
    let budget = PayloadBudget::with_limit(peak);
    let other_request = budget.reserve(1)?;
    assert_eq!(
        lease
            .prepare_with_budget(
                &[fragment("synthetic-denied")],
                &[],
                budget.clone(),
                |_| -> Result<(), TokenError> {
                    panic!("shared-budget denial must precede validation")
                }
            )
            .err(),
        Some(TokenError::CapacityExceeded)
    );
    assert_eq!(budget.used(), 1);
    assert_eq!(store.stats()?, before);
    assert_eq!(lease.authorized_tokens()?.len(), 1);
    assert!(lease.resolve(&old)? == "synthetic-pinned");
    drop(other_request);
    lease.prepare_with_budget(&[fragment("synthetic-denied")], &[], budget.clone(), |_| {
        Ok(())
    })?;
    assert_eq!(budget.used(), 0);
    assert_eq!(store.stats()?.entries, 2);
    Ok(())
}

#[test]
fn preparation_scratch_conflicted_attempt_releases_before_retry_and_respects_ceiling(
) -> Result<(), TokenError> {
    use super::{payload_budget::PayloadBudget, payload_text::PayloadText};
    let store = DlpTokenStore::new();
    let mut lease = store.admit("codex", Some("synthetic-retry"), 1)?;
    let mut writer = store.admit("codex", Some("synthetic-retry"), 1)?;
    let budget = PayloadBudget::with_limit(scratch_profile(1, 1).1 + 7);
    let calls = std::cell::Cell::new(0);
    let result = lease.prepare_with_budget(
        &[fragment("synthetic-retry-value")],
        &[],
        budget.clone(),
        |_| {
            let attempt = calls.get();
            calls.set(attempt + 1);
            let expected = scratch_profile(1, usize::from(attempt == 0)).0;
            assert_eq!(budget.used(), expected);
            writer.prepare(&[fragment("synthetic-retry-value")], &[], |_| Ok(()))?;
            PayloadText::try_from_str("fixture", budget.clone(), 7)
        },
    );
    assert_eq!(result.err(), Some(TokenError::Unavailable));
    assert_eq!(calls.get(), 32);
    assert_eq!(budget.used(), 0);
    assert!(lease.authorized_tokens()?.is_empty());
    assert_eq!(store.stats()?.entries, 1);
    lease.prepare_with_budget(
        &[fragment("synthetic-retry-value")],
        &[],
        budget.clone(),
        |_| Ok(()),
    )?;
    assert_eq!(budget.used(), 0);
    assert_eq!(lease.authorized_tokens()?.len(), 1);
    Ok(())
}

#[test]
fn preparation_scratch_all_allocation_and_copy_boundaries_release_on_error_or_unwind(
) -> Result<(), TokenError> {
    use super::{
        payload_budget::PayloadBudget, payload_metadata::test_support as metadata,
        payload_text::test_support as text,
    };
    use std::{
        cell::Cell,
        panic::{catch_unwind, AssertUnwindSafe},
        rc::Rc,
    };
    // Five distinct values exercise both initial four-slot and replacement eight-slot vectors.
    let raw: Vec<_> = (0..5).map(|i| format!("synthetic-fault-{i}")).collect();
    let fragments: Vec<_> = raw.iter().map(|raw| fragment(raw)).collect();
    let expected_events = 5 * 2 * 2 + 5 * 3;
    for unwind in [false, true] {
        for fail_at in 0..=expected_events {
            let store = DlpTokenStore::new();
            let mut lease = store.admit("codex", None, 1)?;
            let old = lease.prepare(&[fragment("synthetic-fault-pinned")], &[], |tokens| {
                Ok(tokens[0].clone())
            })?;
            let before = store.stats()?;
            let budget = PayloadBudget::with_limit(scratch_profile(5, 5).1);
            let hits = Rc::new(Cell::new(0));
            let vector_hits = hits.clone();
            let reentrant_store = store.clone();
            let metadata_hook = metadata::install(move |_| {
                // Successful same-thread reentry, not a contested try_lock result.
                assert_eq!(reentrant_store.stats().ok(), Some(before));
                vector_hits.set(vector_hits.get() + 1);
                if vector_hits.get() == fail_at {
                    if unwind {
                        panic!("synthetic vector boundary interruption");
                    }
                    metadata::Action::Fail
                } else {
                    metadata::Action::Continue
                }
            });
            let text_hits = hits.clone();
            let reentrant_store = store.clone();
            let text_hook = text::install(move |_| {
                assert_eq!(reentrant_store.stats().ok(), Some(before));
                text_hits.set(text_hits.get() + 1);
                if text_hits.get() == fail_at {
                    if unwind {
                        panic!("synthetic text boundary interruption");
                    }
                    text::Action::Fail
                } else {
                    text::Action::Continue
                }
            });
            let mut validated = false;
            let result = catch_unwind(AssertUnwindSafe(|| {
                lease.prepare_with_budget(&fragments, &[], budget.clone(), |_| {
                    validated = true;
                    assert_eq!(store.stats()?, before);
                    Ok(())
                })
            }));
            drop(text_hook);
            drop(metadata_hook);
            assert_eq!(budget.used(), 0);
            if fail_at == 0 {
                assert_eq!(hits.get(), expected_events);
                assert!(matches!(result, Ok(Ok(()))));
                assert!(validated);
                assert_eq!(store.stats()?.entries, before.entries + 5);
            } else {
                assert_eq!(hits.get(), fail_at);
                assert!(!validated);
                if unwind {
                    assert!(result.is_err());
                } else {
                    assert!(matches!(result, Ok(Err(TokenError::Unavailable))));
                }
                assert_eq!(store.stats()?, before);
                assert_eq!(lease.authorized_tokens()?.len(), 1);
                assert!(lease.resolve(&old)? == "synthetic-fault-pinned");
                // Interrupted allocation hooks must not poison the store or retain budget.
                lease.prepare_with_budget(&fragments, &[], budget.clone(), |_| Ok(()))?;
                assert_eq!(budget.used(), 0);
            }
        }
    }
    Ok(())
}

#[test]
fn preparation_scratch_validation_error_and_unwind_leave_prior_authority_intact(
) -> Result<(), TokenError> {
    use super::payload_budget::PayloadBudget;
    use std::panic::{catch_unwind, AssertUnwindSafe};
    for unwind in [false, true] {
        let store = DlpTokenStore::new();
        let mut lease = store.admit("codex", None, 1)?;
        let old = lease.prepare(&[fragment("synthetic-validation-pinned")], &[], |tokens| {
            Ok(tokens[0].clone())
        })?;
        let before = store.stats()?;
        let budget = PayloadBudget::with_limit(scratch_profile(1, 1).1);
        let mut validated = false;
        let result = catch_unwind(AssertUnwindSafe(|| {
            lease.prepare_with_budget(
                &[fragment("synthetic-validation-new")],
                &[],
                budget.clone(),
                |_| -> Result<(), TokenError> {
                    validated = true;
                    assert_eq!(budget.used(), scratch_profile(1, 1).0);
                    assert_eq!(store.stats()?, before);
                    if unwind {
                        panic!("synthetic validation interruption");
                    }
                    Err(TokenError::RedactionFailed)
                },
            )
        }));
        assert!(validated);
        if unwind {
            assert!(result.is_err());
        } else {
            assert!(matches!(result, Ok(Err(TokenError::RedactionFailed))));
        }
        assert_eq!(budget.used(), 0);
        assert_eq!(store.stats()?, before);
        assert_eq!(lease.authorized_tokens()?.len(), 1);
        assert!(lease.resolve(&old)? == "synthetic-validation-pinned");
    }
    Ok(())
}
