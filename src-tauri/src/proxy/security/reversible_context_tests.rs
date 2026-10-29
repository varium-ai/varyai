//! Synthetic-only prerequisites for bounded completed-context dependency ownership.
use super::{
    payload_budget::PayloadBudget,
    token_store_owners::AllocationCharge,
    tokenization::{DlpTokenStore, TokenCategory, TokenError, TokenFragment},
};

#[test]
fn borrowed_dependencies_deny_before_backing_allocation_when_metadata_is_exhausted(
) -> Result<(), TokenError> {
    let store = DlpTokenStore::new();
    let mut lease = store.admit("codex", Some("synthetic-context"), 1)?;
    lease.prepare_with_budget(
        &[TokenFragment {
            raw: "synthetic-context-secret",
            whole_span_categories: &[TokenCategory::Custom],
        }],
        &[],
        PayloadBudget::with_limit(1024 * 1024),
        |_| Ok(()),
    )?;
    let logical_before = store.stats()?;
    let ledger = store.allocation_ledger_for_test()?;
    let before = ledger.used();
    let remaining = ledger.limits_for_test().metadata - before.metadata;
    let held = ledger.reserve(AllocationCharge {
        plaintext: 0,
        metadata: remaining,
    })?;
    let full = ledger.used();
    let result = lease.authorized_tokens();
    assert_eq!(
        result.err(),
        Some(TokenError::CapacityExceeded),
        "borrowed dependency backing bypassed the exhausted metadata ledger"
    );
    assert_eq!(ledger.used(), full);
    assert_eq!(store.stats()?, logical_before);
    let mut authorized = 0;
    lease.visit_authorized_tokens(|reference| {
        assert!(lease.resolve(reference)? == "synthetic-context-secret");
        authorized += 1;
        Ok(())
    })?;
    assert_eq!(authorized, 1);
    drop(held);
    assert_eq!(ledger.used(), before);
    Ok(())
}

fn seeded_dependencies(
    count: usize,
) -> Result<(DlpTokenStore, super::tokenization::DlpRequestLease), TokenError> {
    let store = DlpTokenStore::new();
    let mut lease = store.admit("codex", Some("synthetic-collector"), 1)?;
    let raw: Vec<_> = (0..count)
        .map(|index| format!("synthetic-collector-{index}"))
        .collect();
    let fragments: Vec<_> = raw
        .iter()
        .map(|raw| TokenFragment {
            raw,
            whole_span_categories: &[TokenCategory::Custom],
        })
        .collect();
    lease.prepare_with_budget(
        &fragments,
        &[],
        PayloadBudget::with_limit(16 * 1024 * 1024),
        |_| Ok(()),
    )?;
    Ok((store, lease))
}

#[test]
fn borrowed_dependencies_exact_backing_and_difference_one_preserve_authority(
) -> Result<(), TokenError> {
    for count in [1, 4, 9, 4096] {
        let (store, lease) = seeded_dependencies(count)?;
        let logical = store.stats()?;
        let ledger = store.allocation_ledger_for_test()?;
        let before = ledger.used();
        let bytes = count * std::mem::size_of::<&str>();
        for available in [bytes - 1, bytes] {
            let held = ledger.reserve(AllocationCharge {
                plaintext: 0,
                metadata: ledger.limits_for_test().metadata - before.metadata - available,
            })?;
            let baseline = ledger.used();
            let result = lease.authorized_tokens();
            if available < bytes {
                assert_eq!(result.err(), Some(TokenError::CapacityExceeded));
            } else {
                let references = result?;
                assert_eq!(references.len(), count);
                assert_eq!(ledger.used().metadata, baseline.metadata + bytes);
                assert_eq!(ledger.used().plaintext, baseline.plaintext);
                drop(references);
            }
            assert_eq!(ledger.used(), baseline);
            assert_eq!(store.stats()?, logical);
            drop(held);
            assert_eq!(ledger.used(), before);
        }
        let empty = store.admit("codex", Some("synthetic-collector"), 1)?;
        let mut count_resolved = 0;
        lease.visit_authorized_tokens(|reference| {
            lease.resolve(reference)?;
            assert_eq!(
                empty.resolve(reference).err(),
                Some(TokenError::ContextUnavailable)
            );
            count_resolved += 1;
            Ok(())
        })?;
        assert_eq!(count_resolved, count);
    }
    Ok(())
}

#[test]
fn borrowed_dependencies_distinct_collectors_charge_the_full_coexistence_peak(
) -> Result<(), TokenError> {
    let (store, lease) = seeded_dependencies(9)?;
    let ledger = store.allocation_ledger_for_test()?;
    let before = ledger.used();
    let bytes = 9 * std::mem::size_of::<&str>();
    let held = ledger.reserve(AllocationCharge {
        plaintext: 0,
        metadata: ledger.limits_for_test().metadata - before.metadata - 2 * bytes,
    })?;
    let baseline = ledger.used();
    let first = lease.authorized_tokens()?;
    assert_eq!(first.len(), 9);
    assert_eq!(ledger.used().metadata, baseline.metadata + bytes);
    let one_byte = ledger.reserve(AllocationCharge {
        plaintext: 0,
        metadata: 1,
    })?;
    assert_eq!(
        lease.authorized_tokens().err(),
        Some(TokenError::CapacityExceeded)
    );
    assert_eq!(ledger.used().metadata, baseline.metadata + bytes + 1);
    drop(one_byte);
    let second = lease.authorized_tokens()?;
    assert_eq!(second.len(), 9);
    assert_eq!(ledger.used().metadata, baseline.metadata + 2 * bytes);
    drop(first);
    assert_eq!(ledger.used().metadata, baseline.metadata + bytes);
    drop(second);
    assert_eq!(ledger.used(), baseline);
    drop(held);
    assert_eq!(ledger.used(), before);
    Ok(())
}

#[test]
fn empty_borrowed_dependencies_allocate_nothing_even_with_exhausted_metadata(
) -> Result<(), TokenError> {
    use super::token_store_owners::test_support;
    let store = DlpTokenStore::new();
    let lease = store.admit("codex", Some("synthetic-empty-collector"), 1)?;
    let ledger = store.allocation_ledger_for_test()?;
    let held = ledger.reserve(AllocationCharge {
        plaintext: 0,
        metadata: ledger.limits_for_test().metadata - ledger.used().metadata,
    })?;
    let before = ledger.used();
    let guard = test_support::install(|observation| {
        if observation.phase == test_support::Phase::BeforeAllocation {
            panic!("empty dependency collector attempted backing allocation");
        }
        Ok(())
    });
    let references = lease.authorized_tokens()?;
    assert!(references.is_empty());
    assert_eq!(ledger.used(), before);
    drop(references);
    assert_eq!(ledger.used(), before);
    drop(guard);
    drop(held);
    Ok(())
}

#[test]
fn borrowed_dependencies_precharge_and_release_backing_at_each_failure_boundary(
) -> Result<(), TokenError> {
    use super::token_store_owners::test_support::{self, Kind, Phase};
    use std::{cell::Cell, rc::Rc};
    for phase in [Phase::BeforeAllocation, Phase::AfterAllocation] {
        let (store, lease) = seeded_dependencies(4)?;
        let ledger = store.allocation_ledger_for_test()?;
        let before = ledger.used();
        let logical = store.stats()?;
        let observed = Rc::new(Cell::new(0));
        let seen = observed.clone();
        let bytes = 4 * std::mem::size_of::<&str>();
        let guard = test_support::install(move |observation| {
            if observation.kind == Kind::Vector && observation.phase == phase {
                assert_eq!(observation.bytes, bytes);
                assert_eq!(observation.used.metadata, before.metadata + bytes);
                assert_eq!(observation.used.plaintext, before.plaintext);
                seen.set(seen.get() + 1);
                return Err(TokenError::Unavailable);
            }
            Ok(())
        });
        assert_eq!(
            lease.authorized_tokens().err(),
            Some(TokenError::Unavailable)
        );
        assert_eq!(observed.get(), 1);
        assert_eq!(ledger.used(), before);
        assert_eq!(store.stats()?, logical);
        drop(guard);
    }
    Ok(())
}

#[test]
fn borrowed_dependencies_unwind_releases_backing_without_releasing_source_authority(
) -> Result<(), TokenError> {
    use super::token_store_owners::test_support::{self, Kind, Phase};
    use std::panic::{catch_unwind, AssertUnwindSafe};
    for phase in [Phase::AfterAllocation, Phase::BeforeDrop] {
        let (store, lease) = seeded_dependencies(4)?;
        let ledger = store.allocation_ledger_for_test()?;
        let before = ledger.used();
        let logical = store.stats()?;
        let bytes = 4 * std::mem::size_of::<&str>();
        let guard = test_support::install(move |observation| {
            if observation.kind == Kind::Vector && observation.phase == phase {
                assert_eq!(observation.used.metadata, before.metadata + bytes);
                panic!("synthetic dependency collector unwind");
            }
            Ok(())
        });
        let outcome = catch_unwind(AssertUnwindSafe(|| {
            let references = lease.authorized_tokens()?;
            assert_eq!(references.len(), 4);
            drop(references);
            Ok::<_, TokenError>(())
        }));
        assert!(outcome.is_err());
        assert_eq!(ledger.used(), before);
        assert_eq!(store.stats()?, logical);
        drop(guard);
        let mut count = 0;
        lease.visit_authorized_tokens(|reference| {
            lease.resolve(reference)?;
            count += 1;
            Ok(())
        })?;
        assert_eq!(count, 4);
    }
    Ok(())
}
#[test]
fn completed_context_isolates_attempts_and_denies_cross_boundary_or_replaced_snapshots(
) -> Result<(), TokenError> {
    use super::completed_context::{CompletedCallSpec, CompletionWitness};

    let store_a = DlpTokenStore::new();
    let store_b = DlpTokenStore::new();
    let budget = PayloadBudget::with_limit(16 * 1024 * 1024);

    // Base lease in session A.
    let mut base_a = store_a.admit("codex", Some("session-alpha"), 1)?;
    let token_base = base_a.prepare_with_budget(
        &[TokenFragment {
            raw: "synthetic-base-secret",
            whole_span_categories: &[TokenCategory::Custom],
        }],
        &[],
        budget.clone(),
        |tokens| Ok(tokens[0].as_str().to_owned()),
    )?;

    // Fork two independent attempt leases from base_a.
    let mut attempt_failed = base_a.fork_attempt()?;
    let attempt_ok = base_a.fork_attempt()?;
    assert_eq!(
        attempt_failed.resolve(&token_base)?,
        "synthetic-base-secret"
    );
    assert_eq!(attempt_ok.resolve(&token_base)?, "synthetic-base-secret");

    // Failed attempt adds a secret that never completes; it must not contaminate base_a, attempt_ok, or later resumes!
    let token_uncompleted = attempt_failed.prepare_with_budget(
        &[TokenFragment {
            raw: "synthetic-uncompleted-attempt-secret",
            whole_span_categories: &[TokenCategory::Custom],
        }],
        &[],
        budget.clone(),
        |tokens| Ok(tokens[0].as_str().to_owned()),
    )?;
    assert_eq!(
        base_a.resolve(&token_uncompleted).err(),
        Some(TokenError::ContextUnavailable)
    );
    assert_eq!(
        attempt_ok.resolve(&token_uncompleted).err(),
        Some(TokenError::ContextUnavailable)
    );
    drop(attempt_failed);

    // Publish completed context on attempt_ok with a specific provider, call snapshot, reasoning, and opaque item.
    let binding_p1 = attempt_ok.bind_actual_attempt(
        "provider-primary",
        "responses",
        "https://api.example.invalid/v1/responses",
        Some("principal-primary"),
    )?;
    let witness = CompletionWitness::from_verified_completion(true)?;
    let call_spec = CompletedCallSpec {
        call_id: "call_alpha_1",
        item_type: "function_call",
        name: "exec_cmd",
        payload: &format!("{{\"arg\":\"{token_base}\"}}"),
        reasoning: Some("thinking-step-1"),
        encrypted_reasoning: Some("enc-reasoning-v1"),
    };
    let _handle = attempt_ok.publish_completed_context(
        &binding_p1,
        &witness,
        "resp_alpha_1",
        &[&token_base],
        &[call_spec],
        &["opaque-compaction-v1"],
    )?;

    let mut violations = Vec::new();

    // 1. Fresh admit in same session still starts with empty authority before resume.
    let fresh = store_a.admit("codex", Some("session-alpha"), 1)?;
    if fresh.resolve(&token_base).is_ok() {
        violations.push("new-lease-not-empty");
    }

    // 2. Resuming with valid cursor, call snapshot, and opaque succeeds and transfers ONLY completed lineage (not token_uncompleted).
    let mut resume_ok = fresh.fork_attempt()?;
    let resume_binding = resume_ok.bind_actual_attempt(
        "provider-primary",
        "responses",
        "https://api.example.invalid/v1/responses",
        Some("principal-primary"),
    )?;
    if resume_ok
        .resume_completed_context(
            &resume_binding,
            Some("resp_alpha_1"),
            &[call_spec],
            &["opaque-compaction-v1"],
        )
        .is_err()
    {
        violations.push("valid-resume-failed");
    }
    if resume_ok.resolve(&token_base).ok() != Some("synthetic-base-secret") {
        violations.push("valid-resume-missing-base-token");
    }
    if resume_ok.resolve(&token_uncompleted).is_ok() {
        violations.push("uncompleted-token-leaked-into-resume");
    }

    // 3. Cross-provider denial: same response_id/call_id on a different provider/target/principal must fail.
    let mut resume_wrong_provider = fresh.fork_attempt()?;
    let binding_p2 = resume_wrong_provider.bind_actual_attempt(
        "provider-secondary",
        "responses",
        "https://api.example.invalid/v1/responses",
        Some("principal-primary"),
    )?;
    if resume_wrong_provider
        .resume_completed_context(&binding_p2, Some("resp_alpha_1"), &[call_spec], &[])
        .is_ok()
    {
        violations.push("cross-provider-admitted");
    }

    // 4. Missing/unstable auth principal cannot publish or resume cross-request context.
    let binding_no_principal = attempt_ok.bind_actual_attempt(
        "provider-primary",
        "responses",
        "https://api.example.invalid/v1/responses",
        None,
    )?;
    if attempt_ok
        .publish_completed_context(
            &binding_no_principal,
            &witness,
            "resp_no_principal",
            &[],
            &[],
            &[],
        )
        .is_ok()
    {
        violations.push("missing-principal-published");
    }

    // 5. Anonymous request cannot publish or resume completed context.
    let anon = store_a.admit("codex", None, 1)?;
    if let Ok(anon_binding) = anon.bind_actual_attempt(
        "provider-primary",
        "responses",
        "https://api.example.invalid/v1/responses",
        Some("principal-primary"),
    ) {
        if anon
            .publish_completed_context(&anon_binding, &witness, "resp_anon", &[], &[], &[])
            .is_ok()
        {
            violations.push("anonymous-published");
        }
    }

    // 6. Cross-store and cross-session denial.
    let other_session = store_a.admit("codex", Some("session-beta"), 1)?;
    let mut other_attempt = other_session.fork_attempt()?;
    let other_binding = other_attempt.bind_actual_attempt(
        "provider-primary",
        "responses",
        "https://api.example.invalid/v1/responses",
        Some("principal-primary"),
    )?;
    if other_attempt
        .resume_completed_context(&other_binding, Some("resp_alpha_1"), &[call_spec], &[])
        .is_ok()
    {
        violations.push("cross-session-admitted");
    }
    let other_store_lease = store_b.admit("codex", Some("session-alpha"), 1)?;
    let mut other_store_attempt = other_store_lease.fork_attempt()?;
    if other_store_attempt
        .resume_completed_context(&resume_binding, Some("resp_alpha_1"), &[call_spec], &[])
        .is_ok()
    {
        violations.push("cross-store-binding-admitted");
    }

    // 7. Replaced/mutated call arguments or reasoning snapshot with same call_id must be rejected.
    let mut resume_tampered_args = fresh.fork_attempt()?;
    let tampered_args = CompletedCallSpec {
        payload: "{\"arg\":\"tampered\"}",
        ..call_spec
    };
    if resume_tampered_args
        .resume_completed_context(&resume_binding, Some("resp_alpha_1"), &[tampered_args], &[])
        .is_ok()
    {
        violations.push("tampered-call-payload-admitted");
    }

    let mut resume_tampered_reasoning = fresh.fork_attempt()?;
    let tampered_reasoning = CompletedCallSpec {
        reasoning: Some("tampered-thinking"),
        ..call_spec
    };
    if resume_tampered_reasoning
        .resume_completed_context(
            &resume_binding,
            Some("resp_alpha_1"),
            &[tampered_reasoning],
            &[],
        )
        .is_ok()
    {
        violations.push("tampered-reasoning-admitted");
    }

    // 8. Unknown response_id or unknown opaque blob must fail closed.
    let mut resume_unknown_cursor = fresh.fork_attempt()?;
    if resume_unknown_cursor
        .resume_completed_context(&resume_binding, Some("resp_unknown"), &[], &[])
        .is_ok()
    {
        violations.push("unknown-cursor-admitted");
    }
    let mut resume_unknown_opaque = fresh.fork_attempt()?;
    if resume_unknown_opaque
        .resume_completed_context(
            &resume_binding,
            Some("resp_alpha_1"),
            &[],
            &["opaque-unknown"],
        )
        .is_ok()
    {
        violations.push("unknown-opaque-admitted");
    }

    // 9. Same-ID conflicting republication must be rejected (immutable snapshot).
    let conflicting_call = CompletedCallSpec {
        payload: "{\"arg\":\"replaced\"}",
        ..call_spec
    };
    if attempt_ok
        .publish_completed_context(
            &binding_p1,
            &witness,
            "resp_alpha_1",
            &[&token_base],
            &[conflicting_call],
            &["opaque-compaction-v1"],
        )
        .is_ok()
    {
        violations.push("same-id-conflicting-republish-admitted");
    }

    assert!(
        violations.is_empty(),
        "completed-context contract violations: {violations:?}"
    );
    Ok(())
}

#[test]
fn completed_context_zero_visible_output_retains_lineage_and_unique_call_fallback_checks_snapshot(
) -> Result<(), TokenError> {
    use super::completed_context::{CompletedCallSpec, CompletionWitness};

    let store = DlpTokenStore::new();
    let budget = PayloadBudget::with_limit(16 * 1024 * 1024);
    let mut base = store.admit("codex", Some("session-compact"), 1)?;
    let inherited_token = base.prepare_with_budget(
        &[TokenFragment {
            raw: "synthetic-compact-inherited",
            whole_span_categories: &[TokenCategory::Custom],
        }],
        &[],
        budget,
        |tokens| Ok(tokens[0].as_str().to_owned()),
    )?;

    let attempt1 = base.fork_attempt()?;
    let binding = attempt1.bind_actual_attempt(
        "provider-primary",
        "responses",
        "https://api.example.invalid/v1/responses/compact",
        Some("principal-1"),
    )?;
    let witness = CompletionWitness::from_verified_completion(true)?;
    // Zero visible output tokens, but a valid compaction opaque item.
    attempt1.publish_completed_context(
        &binding,
        &witness,
        "resp_compact_1",
        &[],
        &[],
        &["opaque-compact-blob-1"],
    )?;

    // Follow-up turn resumes via opaque-compact-blob-1 without previous_response_id;
    // inherited_token must still be preserved in the lineage!
    let turn2_base = store.admit("codex", Some("session-compact"), 1)?;
    let mut turn2_attempt = turn2_base.fork_attempt()?;
    let turn2_binding = turn2_attempt.bind_actual_attempt(
        "provider-primary",
        "responses",
        "https://api.example.invalid/v1/responses/compact",
        Some("principal-1"),
    )?;
    turn2_attempt.resume_completed_context(
        &turn2_binding,
        None,
        &[],
        &["opaque-compact-blob-1"],
    )?;
    assert_eq!(
        turn2_attempt.resolve(&inherited_token)?,
        "synthetic-compact-inherited"
    );

    // Publish two separate completed responses with the same call_id -> unique-call fallback must reject ambiguity!
    let call_dup = CompletedCallSpec {
        call_id: "call_ambiguous",
        item_type: "function_call",
        name: "tool_a",
        payload: "{}",
        reasoning: None,
        encrypted_reasoning: None,
    };
    attempt1.publish_completed_context(&binding, &witness, "resp_dup_1", &[], &[call_dup], &[])?;
    attempt1.publish_completed_context(&binding, &witness, "resp_dup_2", &[], &[call_dup], &[])?;

    let mut turn3_attempt = turn2_base.fork_attempt()?;
    assert_eq!(
        turn3_attempt
            .resume_completed_context(&turn2_binding, None, &[call_dup], &[])
            .err(),
        Some(TokenError::ContextUnavailable),
        "ambiguous call_id without previous_response_id must be rejected"
    );
    Ok(())
}

#[test]
fn reversible_closure_req2_lineage_keeps_exact_send_and_refuses_identity_changes(
) -> Result<(), TokenError> {
    use super::completed_context::CompletionWitness;
    use crate::proxy::providers::CodexAdapter;
    let store = DlpTokenStore::new();
    let mut first = store.admit("codex", Some("synthetic-route-session"), 1)?;
    let token = first.prepare_with_budget(
        &[TokenFragment {
            raw: "synthetic-route-secret",
            whole_span_categories: &[TokenCategory::Custom],
        }],
        &[],
        PayloadBudget::with_limit(1024 * 1024),
        |tokens| Ok(tokens[0].as_str().to_owned()),
    )?;
    let base = "https://synthetic.invalid/prefix";
    let source_url = "https://synthetic.invalid/prefix/responses?a=1&b=2";
    let source_route = CodexAdapter::resolve_dlp_operation_descriptor(
        base,
        "/responses?a=1&b=2",
        "openai_responses",
        source_url,
        false,
    )
    .ok_or(TokenError::ContextUnavailable)?;
    let source = first.bind_actual_operation_attempt(
        "synthetic-provider",
        "openai_responses",
        source_url,
        Some("synthetic-auth"),
        Some(&source_route),
    )?;
    first.publish_completed_context(
        &source,
        &CompletionWitness::from_verified_completion(true)?,
        "resp_synthetic_route",
        &[&token],
        &[],
        &["synthetic-route-opaque"],
    )?;
    let target_url = "https://synthetic.invalid/prefix/responses/compact?a=1&b=2";
    let target_route = CodexAdapter::resolve_dlp_operation_descriptor(
        base,
        "/responses/compact?a=1&b=2",
        "openai_responses",
        target_url,
        false,
    )
    .ok_or(TokenError::ContextUnavailable)?;
    let mut resumed = store.admit("codex", Some("synthetic-route-session"), 1)?;
    let target = resumed.bind_actual_operation_attempt(
        "synthetic-provider",
        "openai_responses",
        target_url,
        Some("synthetic-auth"),
        Some(&target_route),
    )?;
    assert!(
        source.provider_fingerprint != target.provider_fingerprint,
        "cross-operation lineage must not replace the exact actual-send fingerprint"
    );
    resumed.resume_completed_context(
        &target,
        Some("resp_synthetic_route"),
        &[],
        &["synthetic-route-opaque"],
    )?;
    assert_eq!(resumed.resolve(&token)?, "synthetic-route-secret");
    for (provider, protocol, auth, session, revision, prefix, endpoint, url, full) in [
        (
            "synthetic-other",
            "openai_responses",
            "synthetic-auth",
            "synthetic-route-session",
            1,
            base,
            "/responses/compact?a=1&b=2",
            target_url,
            false,
        ),
        (
            "synthetic-provider",
            "openai_chat",
            "synthetic-auth",
            "synthetic-route-session",
            1,
            base,
            "/responses/compact?a=1&b=2",
            "https://synthetic.invalid/prefix/chat/completions?a=1&b=2",
            false,
        ),
        (
            "synthetic-provider",
            "openai_responses",
            "synthetic-other-auth",
            "synthetic-route-session",
            1,
            base,
            "/responses/compact?a=1&b=2",
            target_url,
            false,
        ),
        (
            "synthetic-provider",
            "openai_responses",
            "synthetic-auth",
            "synthetic-other-session",
            1,
            base,
            "/responses/compact?a=1&b=2",
            target_url,
            false,
        ),
        (
            "synthetic-provider",
            "openai_responses",
            "synthetic-auth",
            "synthetic-route-session",
            1,
            base,
            "/responses/compact?b=2&a=1",
            "https://synthetic.invalid/prefix/responses/compact?b=2&a=1",
            false,
        ),
        (
            "synthetic-provider",
            "openai_responses",
            "synthetic-auth",
            "synthetic-route-session",
            1,
            "https://synthetic.invalid/other",
            "/responses/compact?a=1&b=2",
            "https://synthetic.invalid/other/responses/compact?a=1&b=2",
            false,
        ),
        (
            "synthetic-provider",
            "openai_responses",
            "synthetic-auth",
            "synthetic-route-session",
            1,
            base,
            "/responses/compact?a=1&b=2",
            target_url,
            true,
        ),
        (
            "synthetic-provider",
            "openai_responses",
            "synthetic-auth",
            "synthetic-route-session",
            2,
            base,
            "/responses/compact?a=1&b=2",
            target_url,
            false,
        ),
    ] {
        let mut request = store.admit("codex", Some(session), revision)?;
        let descriptor =
            CodexAdapter::resolve_dlp_operation_descriptor(prefix, endpoint, protocol, url, full);
        let binding = request.bind_actual_operation_attempt(
            provider,
            protocol,
            url,
            Some(auth),
            descriptor.as_ref(),
        )?;
        assert_eq!(
            request
                .resume_completed_context(&binding, Some("resp_synthetic_route"), &[], &[])
                .err(),
            Some(TokenError::ContextUnavailable),
            "changed identity must not authorize cursor lineage"
        );
        assert_eq!(
            request.resume_completed_tokens(&binding, &[&token]).err(),
            Some(TokenError::ContextUnavailable),
            "changed identity must not authorize token lineage"
        );
    }
    Ok(())
}

#[test]
fn reversible_closure_req2_ambiguous_routes_issue_no_descriptor() {
    use crate::proxy::providers::CodexAdapter;
    for (base, endpoint, target, full) in [
        (
            "https://synthetic.invalid/v1/responses",
            "/responses/compact",
            "https://synthetic.invalid/v1/responses/compact",
            false,
        ),
        (
            "https://synthetic.invalid/custom",
            "/responses/compact",
            "https://synthetic.invalid/custom",
            true,
        ),
        (
            "https://synthetic.invalid?route=1",
            "/responses/compact",
            "https://synthetic.invalid/responses/compact?route=1",
            false,
        ),
        (
            "https://synthetic.invalid",
            "/arbitrary/compact",
            "https://synthetic.invalid/arbitrary/compact",
            false,
        ),
        (
            "https://synthetic.invalid",
            "/responses/compact",
            "https://other.invalid/v1/responses/compact",
            false,
        ),
    ] {
        assert!(
            CodexAdapter::resolve_dlp_operation_descriptor(
                base,
                endpoint,
                "openai_responses",
                target,
                full
            )
            .is_none(),
            "ambiguous or mismatched routes must stay exact-send-only"
        );
    }
}
