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

mod text_fallback_context_tests {
    use super::super::{
        completed_context::{ActualAttemptBinding, CompletedCallSpec, CompletionWitness},
        http_restoration::{admit_http_request, RestorationAdmission, RestorationGateway},
        request_preparation::is_fresh_readable_history,
        response_restoration::{prepare_text_projection, RestoreError, TEXT_PROJECTION_NOTICE},
        restore_diagnostics::{ContextResumeReason, RestoreReason},
        runtime::{self, SecurityRuntimeConfig},
        tokenization::{DlpRequestLease, TokenStoreLimits},
    };
    use super::*;
    use crate::{app_config::AppType, proxy::providers::codex_chat_history::CodexChatHistoryStore};
    use serde_json::{json, Value};

    type TestResult = Result<(), Box<dyn std::error::Error>>;
    const SESSION: &str = "synthetic-projection-context";
    const PROJECTED_ID: &str = "resp_synthetic_projected_context";
    const PROVIDER: &str = "synthetic-projection-provider";
    const TARGET: &str = "https://synthetic.invalid/v1/responses";
    const PRINCIPAL: &str = "synthetic-projection-principal";
    const OPAQUE: &str = "synthetic-projection-opaque";
    const ORIGINAL: &str = "synthetic-projection-context-value";

    struct Fixture {
        store: DlpTokenStore,
        lease: DlpRequestLease,
        budget: PayloadBudget,
        reference: String,
    }

    fn fixture(revision: u64) -> Result<Fixture, TokenError> {
        let store = DlpTokenStore::new();
        let budget = PayloadBudget::with_limit(16 * 1024 * 1024);
        let mut lease = store.admit("codex", Some(SESSION), revision)?;
        let reference = lease.prepare_with_budget(
            &[TokenFragment {
                raw: ORIGINAL,
                whole_span_categories: &[TokenCategory::Custom],
            }],
            &[],
            budget.clone(),
            |tokens| Ok(tokens[0].as_str().to_owned()),
        )?;
        Ok(Fixture {
            store,
            lease,
            budget,
            reference,
        })
    }

    fn binding(lease: &DlpRequestLease) -> Result<ActualAttemptBinding, TokenError> {
        let binding =
            lease.bind_actual_attempt(PROVIDER, "openai_responses", TARGET, Some(PRINCIPAL))?;
        assert!(
            binding.reusable_principal,
            "fixture must permit ordinary resume"
        );
        Ok(binding)
    }

    fn response(text: &str) -> Value {
        json!({"id":PROJECTED_ID, "object":"response", "status":"completed", "output":[{
            "id":"msg_synthetic_projected_context", "type":"message", "role":"assistant", "status":"completed",
            "content":[{"type":"output_text", "text":text, "annotations":[]}]
        }]})
    }

    fn call(payload: &str) -> CompletedCallSpec<'_> {
        CompletedCallSpec {
            call_id: "call_synthetic_projection_cache",
            item_type: "function_call",
            name: "synthetic_history_only",
            payload,
            reasoning: Some("synthetic-cached-reasoning"),
            encrypted_reasoning: Some(OPAQUE),
        }
    }

    fn assert_no_projection_authority(
        lease: &mut DlpRequestLease,
        binding: &ActualAttemptBinding,
        reference: &str,
        call: CompletedCallSpec<'_>,
    ) -> Result<(), TokenError> {
        assert_eq!(
            lease
                .resume_completed_context(binding, Some(PROJECTED_ID), &[], &[])
                .err(),
            Some(TokenError::ContextUnavailable),
            "projection must not publish a completed cursor"
        );
        assert_eq!(
            lease.resume_completed_tokens(binding, &[reference]).err(),
            Some(TokenError::ContextUnavailable),
            "projection must not publish reference lineage"
        );
        assert_eq!(
            lease
                .resume_completed_context(binding, None, &[call], &[])
                .err(),
            Some(TokenError::ContextUnavailable),
            "a cached call alone must not grant authority"
        );
        assert_eq!(
            lease
                .resume_completed_context(binding, None, &[], &[OPAQUE])
                .err(),
            Some(TokenError::ContextUnavailable),
            "opaque state alone must not grant authority"
        );
        assert_eq!(
            lease
                .verify_output_call_ids(binding, Some(PROJECTED_ID), &[call.call_id])
                .err(),
            Some(TokenError::ContextUnavailable),
            "a cached output call ID must not grant authority"
        );
        assert_eq!(
            lease
                .resume_completed_context(binding, Some(PROJECTED_ID), &[call], &[OPAQUE])
                .err(),
            Some(TokenError::ContextUnavailable),
            "combining uncompleted dependencies must not create authority"
        );
        assert_eq!(
            lease.resolve(reference).err(),
            Some(TokenError::ContextUnavailable)
        );
        Ok(())
    }

    #[test]
    fn text_fallback_context_json_publishes_no_cursor_reference_call_or_opaque_authority(
    ) -> TestResult {
        let fixture = fixture(1)?;
        let source_binding = binding(&fixture.lease)?;
        let payload = json!({"value":fixture.reference}).to_string();
        let call = call(&payload);
        let mut raw = response(&format!("{} rredact-custom-damaged", fixture.reference));
        raw["output"][0]["content"] = json!([
            {"type":"output_text", "text":format!("{} rredact-custom-damaged", fixture.reference), "annotations":[]},
            {"type":"output_text", "text":fixture.reference, "annotations":[]}
        ]);
        let before = fixture.store.stats()?;
        let prepared = prepare_text_projection(&raw, &fixture.lease, &fixture.budget)
            .map_err(|error| std::io::Error::other(error.code()))?;
        assert!(prepared.as_value()["output"][0]["content"][0]["text"] == TEXT_PROJECTION_NOTICE);
        assert!(prepared.as_value()["output"][0]["content"][1]["text"] == ORIGINAL);
        assert!(prepared.as_value()["id"] == PROJECTED_ID);
        assert_eq!(
            fixture.store.stats()?,
            before,
            "projection must not change store authority"
        );
        assert_eq!(fixture.lease.authorized_tokens()?.len(), 1);

        let mut next = fixture.store.admit("codex", Some(SESSION), 1)?;
        let next_binding = binding(&next)?;
        assert_no_projection_authority(&mut next, &next_binding, &fixture.reference, call)?;
        assert!(next.authorized_tokens()?.is_empty());

        // The same principal and dependencies work only after ordinary verified publication.
        fixture.lease.publish_completed_context(
            &source_binding,
            &CompletionWitness::from_verified_completion(true)?,
            "resp_synthetic_strict_context",
            &[&fixture.reference],
            &[call],
            &[OPAQUE],
        )?;
        next.resume_completed_context(
            &next_binding,
            Some("resp_synthetic_strict_context"),
            &[call],
            &[OPAQUE],
        )?;
        next.verify_output_call_ids(
            &next_binding,
            Some("resp_synthetic_strict_context"),
            &[call.call_id],
        )?;
        assert!(next.resolve(&fixture.reference)? == ORIGINAL);
        drop(prepared);
        assert_eq!(fixture.budget.used(), 0);
        Ok(())
    }
    fn expect_resume_reason(
        lease: &mut DlpRequestLease,
        binding: &ActualAttemptBinding,
        cursor: Option<&str>,
        calls: &[CompletedCallSpec<'_>],
        opaques: &[&str],
        expected: ContextResumeReason,
    ) -> TestResult {
        let mut reason = None;
        assert_eq!(
            lease
                .resume_completed_context_diagnosed(binding, cursor, calls, opaques, &mut reason,)
                .err(),
            Some(TokenError::ContextUnavailable),
            "diagnostics must not change the refusal"
        );
        assert_eq!(reason, Some(expected));
        Ok(())
    }

    #[test]
    fn context_resume_diagnostics_identify_fixed_predicates_without_changing_authority(
    ) -> TestResult {
        let fixture = fixture(1)?;
        let source_binding = binding(&fixture.lease)?;
        let payload = json!({"value":fixture.reference}).to_string();
        let call = call(&payload);
        fixture.lease.publish_completed_context(
            &source_binding,
            &CompletionWitness::from_verified_completion(true)?,
            PROJECTED_ID,
            &[&fixture.reference],
            &[call],
            &[OPAQUE],
        )?;
        let mut next = fixture.store.admit("codex", Some(SESSION), 1)?;
        let next_binding = binding(&next)?;
        let mut reason = None;
        next.resume_completed_context_diagnosed(
            &next_binding,
            Some(PROJECTED_ID),
            &[call],
            &[OPAQUE],
            &mut reason,
        )?;
        assert_eq!(reason, None, "successful recovery needs no diagnostic");

        expect_resume_reason(
            &mut next,
            &next_binding,
            Some("invalid cursor"),
            &[],
            &[],
            ContextResumeReason::InvalidCursor,
        )?;
        expect_resume_reason(
            &mut next,
            &next_binding,
            Some("resp_synthetic_missing"),
            &[],
            &[],
            ContextResumeReason::CursorNotFound,
        )?;
        let foreign_binding = next.bind_actual_attempt(
            "synthetic-other-provider",
            "openai_responses",
            TARGET,
            Some(PRINCIPAL),
        )?;
        expect_resume_reason(
            &mut next,
            &foreign_binding,
            Some(PROJECTED_ID),
            &[],
            &[],
            ContextResumeReason::ProviderMismatch,
        )?;
        expect_resume_reason(
            &mut next,
            &next_binding,
            Some(PROJECTED_ID),
            &[CompletedCallSpec {
                payload: "synthetic-changed-arguments",
                ..call
            }],
            &[],
            ContextResumeReason::CallSnapshotMismatch,
        )?;
        expect_resume_reason(
            &mut next,
            &next_binding,
            Some(PROJECTED_ID),
            &[CompletedCallSpec {
                call_id: "call_synthetic_missing",
                ..call
            }],
            &[],
            ContextResumeReason::CallNotFound,
        )?;
        expect_resume_reason(
            &mut next,
            &next_binding,
            Some(PROJECTED_ID),
            &[],
            &["synthetic-unknown-opaque"],
            ContextResumeReason::OpaqueNotFound,
        )?;
        expect_resume_reason(
            &mut next,
            &next_binding,
            Some(PROJECTED_ID),
            &[CompletedCallSpec {
                call_id: "",
                ..call
            }],
            &[],
            ContextResumeReason::InvalidCall,
        )?;
        expect_resume_reason(
            &mut next,
            &next_binding,
            Some(PROJECTED_ID),
            &[],
            &[""],
            ContextResumeReason::InvalidOpaque,
        )?;
        let request_only = next.bind_actual_attempt(PROVIDER, "openai_responses", TARGET, None)?;
        expect_resume_reason(
            &mut next,
            &request_only,
            Some(PROJECTED_ID),
            &[],
            &[],
            ContextResumeReason::PrincipalUnavailable,
        )?;

        fixture.lease.publish_completed_context(
            &source_binding,
            &CompletionWitness::from_verified_completion(true)?,
            "resp_synthetic_second_context",
            &[&fixture.reference],
            &[call],
            &[OPAQUE],
        )?;
        expect_resume_reason(
            &mut next,
            &next_binding,
            None,
            &[call],
            &[],
            ContextResumeReason::AmbiguousCall,
        )?;

        let other_store = DlpTokenStore::new();
        let other_lease = other_store.admit("codex", Some(SESSION), 1)?;
        let other_binding = binding(&other_lease)?;
        expect_resume_reason(
            &mut next,
            &other_binding,
            Some(PROJECTED_ID),
            &[],
            &[],
            ContextResumeReason::LeaseBinding,
        )?;

        let empty_store = DlpTokenStore::new();
        let mut empty = empty_store.admit("codex", Some(SESSION), 1)?;
        let empty_binding = binding(&empty)?;
        expect_resume_reason(
            &mut empty,
            &empty_binding,
            Some(PROJECTED_ID),
            &[],
            &[],
            ContextResumeReason::ContextsEmpty,
        )?;

        let excess = vec![call; 129];
        let mut reason = None;
        assert_eq!(
            next.resume_completed_context_diagnosed(
                &next_binding,
                Some(PROJECTED_ID),
                &excess,
                &[],
                &mut reason,
            ),
            Err(TokenError::CapacityExceeded)
        );
        assert_eq!(
            reason, None,
            "capacity errors retain their original boundary"
        );
        Ok(())
    }

    #[tokio::test]
    async fn text_fallback_context_cached_calls_and_references_do_not_authorize_projected_turn(
    ) -> TestResult {
        let fixture = fixture(1)?;
        let payload = json!({"value":fixture.reference}).to_string();
        let cache = CodexChatHistoryStore::default();
        // A legacy cache entry is independent data, not a tool in the eligible text-only response.
        let cached_call = json!({"type":"function_call", "call_id":call(&payload).call_id,
            "name":"synthetic_history_only", "arguments":payload,
            "reasoning_content":"synthetic-cached-reasoning", "encrypted_reasoning":OPAQUE});
        assert_eq!(
            cache
                .record_response(&json!({"id":PROJECTED_ID, "output":[cached_call]}))
                .await,
            1
        );
        let prepared = prepare_text_projection(
            &response(&format!("{} rredact-custom-damaged", fixture.reference)),
            &fixture.lease,
            &fixture.budget,
        )
        .map_err(|error| std::io::Error::other(error.code()))?;
        assert!(prepared.as_value()["output"][0]["content"][0]["text"] == TEXT_PROJECTION_NOTICE);
        assert_eq!(cache.record_response(prepared.as_value()).await, 0);

        for with_cursor in [true, false] {
            let mut request = json!({"input":[{"type":"function_call_output",
                "call_id":call(&payload).call_id, "output":"synthetic-cached-output"}]});
            if with_cursor {
                request["previous_response_id"] = json!(PROJECTED_ID);
            }
            assert_eq!(cache.enrich_request(&mut request).await, 1);
            assert!(
                request["input"][0] == cached_call,
                "fixture must actually retrieve cached dependencies"
            );
            assert!(!is_fresh_readable_history(&request, &fixture.budget)?);
            let cached_payload = request["input"][0]["arguments"]
                .as_str()
                .ok_or(TokenError::Unavailable)?;
            let mut next = fixture.store.admit("codex", Some(SESSION), 1)?;
            let next_binding = binding(&next)?;
            assert_no_projection_authority(
                &mut next,
                &next_binding,
                &fixture.reference,
                call(cached_payload),
            )?;
            assert!(next.authorized_tokens()?.is_empty());
        }
        Ok(())
    }

    #[test]
    fn text_fallback_context_actual_provider_attempt_binding_cannot_inherit_projection(
    ) -> TestResult {
        let fixture = fixture(1)?;
        let prepared = prepare_text_projection(
            &response(&format!("{} rredact-custom-damaged", fixture.reference)),
            &fixture.lease,
            &fixture.budget,
        )
        .map_err(|error| std::io::Error::other(error.code()))?;
        assert!(prepared.as_value()["id"] == PROJECTED_ID);
        let mut strict = fixture.store.admit("codex", Some(SESSION), 1)?;
        let strict_reference = strict.prepare_with_budget(
            &[TokenFragment {
                raw: "synthetic-strict-only-value",
                whole_span_categories: &[TokenCategory::Custom],
            }],
            &[],
            fixture.budget.clone(),
            |tokens| Ok(tokens[0].as_str().to_owned()),
        )?;
        let source_binding = binding(&strict)?;
        strict.publish_completed_context(
            &source_binding,
            &CompletionWitness::from_verified_completion(true)?,
            "resp_synthetic_binding_control",
            &[&strict_reference],
            &[],
            &[],
        )?;
        for (provider, protocol, target, principal, matches_source) in [
            (PROVIDER, "openai_responses", TARGET, PRINCIPAL, true),
            (
                "synthetic-other-provider",
                "openai_responses",
                TARGET,
                PRINCIPAL,
                false,
            ),
            (PROVIDER, "openai_chat", TARGET, PRINCIPAL, false),
            (
                PROVIDER,
                "openai_responses",
                "https://synthetic.invalid/other/responses",
                PRINCIPAL,
                false,
            ),
            (
                PROVIDER,
                "openai_responses",
                TARGET,
                "synthetic-other-principal",
                false,
            ),
        ] {
            let base = fixture.store.admit("codex", Some(SESSION), 1)?;
            for _ in 0..2 {
                let mut attempt = base.fork_attempt()?;
                let actual =
                    attempt.bind_actual_attempt(provider, protocol, target, Some(principal))?;
                assert!(actual.reusable_principal);
                assert_eq!(
                    actual.provider_fingerprint == source_binding.provider_fingerprint,
                    matches_source
                );
                assert!(attempt.authorized_tokens()?.is_empty());
                assert_no_projection_authority(
                    &mut attempt,
                    &actual,
                    &fixture.reference,
                    call("{}"),
                )?;
                assert!(attempt.authorized_tokens()?.is_empty());
                let control = attempt.resume_completed_context(
                    &actual,
                    Some("resp_synthetic_binding_control"),
                    &[],
                    &[],
                );
                if matches_source {
                    control?;
                    assert!(attempt.resolve(&strict_reference)? == "synthetic-strict-only-value");
                    assert_eq!(
                        attempt.resolve(&fixture.reference).err(),
                        Some(TokenError::ContextUnavailable)
                    );
                } else {
                    assert_eq!(control.err(), Some(TokenError::ContextUnavailable));
                }
            }
            assert!(
                base.authorized_tokens()?.is_empty(),
                "attempts must not mutate base authority"
            );
        }
        Ok(())
    }

    #[test]
    #[serial_test::serial]
    fn text_fallback_context_independent_new_history_admission_and_strict_completion_work(
    ) -> TestResult {
        struct RuntimeReset(SecurityRuntimeConfig);
        impl Drop for RuntimeReset {
            fn drop(&mut self) {
                runtime::set_security_runtime_for_test(self.0.clone());
            }
        }
        let _reset = RuntimeReset((*runtime::get_security_runtime()).clone());
        runtime::set_security_runtime_for_test(SecurityRuntimeConfig::default());
        let revision = runtime::admit_security_runtime()?.restoration_policy_revision;
        let fixture = fixture(revision)?;
        let prepared = prepare_text_projection(
            &response(&format!("{} rredact-custom-damaged", fixture.reference)),
            &fixture.lease,
            &fixture.budget,
        )
        .map_err(|error| std::io::Error::other(error.code()))?;
        let mut history = json!({"model":"synthetic-model", "input":[
            {"type":"message", "role":"assistant", "content":[{"type":"output_text", "text":TEXT_PROJECTION_NOTICE}]},
            {"type":"function_call", "call_id":"call_synthetic_independent", "name":"synthetic_history_only", "arguments":"{\"label\":\"synthetic-independent-history\"}"},
            {"type":"function_call_output", "call_id":"call_synthetic_independent", "output":"synthetic-readable-output"},
            {"role":"user", "content":"Call 13800138000"}
        ]});
        assert!(is_fresh_readable_history(&history, &fixture.budget)?);
        let original_history = history.clone();
        let gateway = RestorationGateway::with_test_store_and_rules(
            fixture.store.clone(),
            &["chinese_phone_number"],
            fixture.budget.clone(),
        );
        let mut headers = http::HeaderMap::new();
        headers.insert("session-id", SESSION.parse()?);
        let request = match admit_http_request(
            &gateway,
            &AppType::Codex,
            true,
            false,
            false,
            &mut history,
            &headers,
        )? {
            RestorationAdmission::Admitted(request) => request,
            _ => return Err("independent synthetic history was not admitted".into()),
        };
        assert!(
            history == original_history,
            "admission must preserve independent source history"
        );
        assert!(matches!(
            request.chat_history_provenance(),
            Ok(super::super::chat_enrichment::ChatHistoryProvenance::IndependentReadable)
        ));
        let transformed = std::cell::Cell::new(0);
        let (mut attempt, _input_owner) = request.prepare_owned_attempt_for_proxy(
            PROVIDER,
            "openai_responses",
            TARGET,
            Some(PRINCIPAL),
            &mut history,
            |candidate| {
                transformed.set(transformed.get() + 1);
                assert!(
                    candidate["input"][0]["content"][0]["text"]
                        == prepared.as_value()["output"][0]["content"][0]["text"]
                );
                assert!(candidate["input"][1] == original_history["input"][1]);
                assert!(candidate["input"][2] == original_history["input"][2]);
                assert!(candidate["input"][3]["content"]
                    .as_str()
                    .is_some_and(|text| text.starts_with("Call rredact-cn_phone-")));
                Ok(candidate)
            },
        )?;
        assert_eq!(transformed.get(), 1);
        let authorized = attempt.lease.authorized_tokens()?;
        assert_eq!(authorized.len(), 1);
        let new_reference = authorized.as_slice()[0].to_owned();
        drop(authorized);
        assert!(attempt.lease.resolve(&new_reference)? == "13800138000");
        assert_no_projection_authority(
            &mut attempt.lease,
            &attempt.binding,
            &fixture.reference,
            call("{}"),
        )?;
        assert!(attempt.lease.resolve(&new_reference)? == "13800138000");
        attempt.lease.publish_completed_context(
            &attempt.binding,
            &CompletionWitness::from_verified_completion(true)?,
            "resp_synthetic_independent_completed",
            &[&new_reference],
            &[],
            &[],
        )?;
        let mut fixture_following = fixture.store.admit("codex", Some(SESSION), revision)?;
        let fixture_binding = binding(&fixture_following)?;
        assert!(
            matches!(
                fixture_following.resume_completed_context(
                    &fixture_binding,
                    Some("resp_synthetic_independent_completed"),
                    &[],
                    &[],
                ),
                Err(TokenError::ContextUnavailable)
            ),
            "fixture namespaces must not inherit published completion authority"
        );
        let mut following =
            fixture
                .store
                .admit_published_policy("codex", Some(SESSION), revision)?;
        let following_binding = binding(&following)?;
        following.resume_completed_context(
            &following_binding,
            Some("resp_synthetic_independent_completed"),
            &[],
            &[],
        )?;
        assert!(following.resolve(&new_reference)? == "13800138000");
        assert_eq!(
            following.resolve(&fixture.reference).err(),
            Some(TokenError::ContextUnavailable)
        );
        Ok(())
    }

    #[test]
    fn text_fallback_context_stale_generation_remains_hard_after_projection() -> TestResult {
        let fixture = fixture(1)?;
        let old_binding = binding(&fixture.lease)?;
        let prepared = prepare_text_projection(
            &response(&format!("{} rredact-custom-damaged", fixture.reference)),
            &fixture.lease,
            &fixture.budget,
        )
        .map_err(|error| std::io::Error::other(error.code()))?;
        assert!(prepared.as_value()["output"][0]["content"][0]["text"] == TEXT_PROJECTION_NOTICE);
        drop(fixture.lease);
        fixture.store.expire_idle_at(
            std::time::Instant::now()
                + TokenStoreLimits::default().idle_ttl
                + std::time::Duration::from_secs(1),
        )?;
        assert_eq!(
            fixture.store.stats()?.namespaces,
            0,
            "projection must not pin an expired namespace"
        );
        let mut next = fixture.store.admit("codex", Some(SESSION), 1)?;
        let actual = binding(&next)?;
        assert!(
            actual.generation != old_binding.generation,
            "fixture must recreate the namespace generation"
        );
        assert_no_projection_authority(&mut next, &old_binding, &fixture.reference, call("{}"))?;
        assert_no_projection_authority(&mut next, &actual, &fixture.reference, call("{}"))?;
        for text in [
            format!("{} rredact-custom-damaged", fixture.reference),
            format!("rredact-custom-damaged {}", fixture.reference),
        ] {
            assert_eq!(
                prepare_text_projection(&response(&text), &next, &fixture.budget).err(),
                Some(RestoreError::FailedBecause(RestoreReason::TokenContext)),
                "a stale complete reference must remain hard before or after format damage"
            );
        }
        // A live, independent format-only field remains eligible without reviving old authority.
        let fresh =
            prepare_text_projection(&response("rredact-custom-damaged"), &next, &fixture.budget)
                .map_err(|error| std::io::Error::other(error.code()))?;
        assert!(fresh.as_value()["output"][0]["content"][0]["text"] == TEXT_PROJECTION_NOTICE);
        assert!(next.authorized_tokens()?.is_empty());
        assert_eq!(
            next.resolve(&fixture.reference).err(),
            Some(TokenError::ContextUnavailable)
        );
        drop(fresh);
        drop(prepared);
        assert_eq!(fixture.budget.used(), 0);
        Ok(())
    }
}

#[test]
fn output_text_exact_nonmatching_completed_text_never_adds_reference_authority(
) -> Result<(), Box<dyn std::error::Error>> {
    use super::{completed_context::CompletionWitness, response_restoration::stage_json_response};
    use serde_json::json;

    let (store, lease) = seeded_dependencies(1)?;
    let budget = PayloadBudget::with_limit(16 * 1024 * 1024);
    let mut foreign = store.admit("codex", Some("synthetic-exact-foreign-context"), 1)?;
    let foreign_reference = foreign.prepare_with_budget(
        &[TokenFragment {
            raw: "synthetic-exact-context-foreign-value",
            whole_span_categories: &[TokenCategory::Custom],
        }],
        &[],
        budget.clone(),
        |tokens| Ok(tokens[0].as_str().to_owned()),
    )?;
    let binding = lease.bind_actual_attempt(
        "synthetic-exact-context-provider",
        "openai_responses",
        "https://synthetic.invalid/v1/responses",
        Some("synthetic-exact-principal"),
    )?;
    let nonmatching = format!(
        "Literal rredact-custom-document and VARY_cu_sample; tail {}",
        &foreign_reference[..18]
    );
    let source = json!({"id":"resp_exact_nonmatching_context","object":"response","status":"completed",
        "output":[{"id":"msg_exact_nonmatching_context","type":"message","role":"assistant","status":"completed",
            "content":[{"type":"output_text","text":nonmatching,"annotations":[]}]}]});
    assert_eq!(lease.authorized_tokens()?.len(), 1);
    assert!(
        lease.resolve(&foreign_reference).is_err(),
        "foreign reference must start unauthorized"
    );
    let staged = stage_json_response(&source, false, &lease, &budget)
        .map_err(|error| std::io::Error::other(error.code()))?;
    assert!(
        staged.as_value()["output"][0]["content"][0]["text"] == nonmatching,
        "nonmatching context text must remain byte-for-byte unchanged"
    );
    let restored = staged
        .publish(
            &lease,
            &binding,
            &CompletionWitness::from_verified_completion(true)?,
            &budget,
        )
        .map_err(|error| std::io::Error::other(error.code()))?;
    assert_eq!(restored.restored_occurrences(), 0);
    assert_eq!(lease.authorized_tokens()?.len(), 1);
    assert!(
        lease.resolve(&foreign_reference).is_err(),
        "preserved nonmatches must not expand source authority"
    );
    let mut next = store.admit("codex", Some("synthetic-collector"), 1)?;
    assert!(
        next.authorized_tokens()?.is_empty(),
        "a fresh lease must start without authority"
    );
    let actual = next.bind_actual_attempt(
        "synthetic-exact-context-provider",
        "openai_responses",
        "https://synthetic.invalid/v1/responses",
        Some("synthetic-exact-principal"),
    )?;
    next.resume_completed_context(&actual, Some("resp_exact_nonmatching_context"), &[], &[])?;
    assert_eq!(
        next.authorized_tokens()?.len(),
        1,
        "only original lineage may resume"
    );
    assert!(
        next.resolve(&foreign_reference).is_err(),
        "nonmatching snapshots must not authorize a complete foreign reference"
    );
    assert_eq!(
        next.resume_completed_tokens(&actual, &[&foreign_reference])
            .err(),
        Some(TokenError::ContextUnavailable)
    );
    assert_eq!(
        next.authorized_tokens()?.len(),
        1,
        "failed foreign resume must not expand authority"
    );
    Ok(())
}

#[tokio::test]
#[serial_test::serial]
async fn output_text_exact_request_side_literals_preserve_independent_pii_detection(
) -> Result<(), Box<dyn std::error::Error>> {
    use super::{
        http_restoration::{admit_http_request, RestorationAdmission, RestorationGateway},
        runtime::{self, SecurityRuntimeConfig},
    };
    use crate::app_config::AppType;
    use serde_json::json;

    struct RuntimeReset(SecurityRuntimeConfig);
    impl Drop for RuntimeReset {
        fn drop(&mut self) {
            runtime::set_security_runtime_for_test(self.0.clone());
        }
    }
    let _reset = RuntimeReset((*runtime::get_security_runtime()).clone());
    runtime::set_security_runtime_for_test(Default::default());
    let revision = runtime::admit_security_runtime()?.restoration_policy_revision;
    let store = DlpTokenStore::new();
    let mut foreign = store.admit("codex", Some("synthetic-exact-strict-foreign"), revision)?;
    let reference = foreign.prepare_with_budget(
        &[TokenFragment {
            raw: "synthetic-exact-strict-foreign-value",
            whole_span_categories: &[TokenCategory::Custom],
        }],
        &[],
        PayloadBudget::with_limit(1024 * 1024),
        |tokens| Ok(tokens[0].as_str().to_owned()),
    )?;
    let gateway = RestorationGateway::with_test_store_and_rules(
        store,
        &["chinese_phone_number"],
        PayloadBudget::with_limit(64 * 1024 * 1024),
    );
    let mut headers = http::HeaderMap::new();
    headers.insert("session-id", "synthetic-exact-strict-request".parse()?);
    for text in [
        "Literal rredact-custom-document".to_owned(),
        "Field tail rredact-custom-123".to_owned(),
        format!("Literal rredact-custom-document then {reference}"),
        reference,
    ] {
        let mut body = json!({"model":"synthetic-model","input":format!("Call 13800138000; {text}"),"stream":false});
        let original = body.clone();
        let request = match admit_http_request(
            &gateway,
            &AppType::Codex,
            true,
            false,
            false,
            &mut body,
            &headers,
        )? {
            RestorationAdmission::Admitted(request) => request,
            _ => return Err("literal_admission_rejected".into()),
        };
        let attempt = request
            .prepare_attempt(
                "synthetic-provider",
                "openai_responses",
                "https://synthetic.invalid/v1/responses",
                Some("synthetic-principal"),
                &mut body,
                |candidate| Ok(candidate.clone()),
            )
            .map_err(|_| "literal_preparation_rejected")?;
        assert!(attempt.has_active_tokens(), "independent_pii_not_protected");
        let output = body["input"].as_str().ok_or("literal_output_missing")?;
        assert!(output.contains(&text), "literal_bytes_changed");
        assert!(
            !output.contains("13800138000"),
            "independent_pii_not_redacted"
        );
        assert!(body != original, "independent_detector_skipped");
    }
    Ok(())
}

#[tokio::test]
#[serial_test::serial]
async fn reference_format_legacy_request_literals_preserve_disabled_and_admitted_paths(
) -> Result<(), Box<dyn std::error::Error>> {
    use super::{
        http_restoration::{admit_http_request, RestorationAdmission, RestorationGateway},
        runtime::{self, SecurityRuntimeConfig},
    };
    use crate::app_config::AppType;
    use serde_json::json;
    struct RuntimeReset(SecurityRuntimeConfig);
    impl Drop for RuntimeReset {
        fn drop(&mut self) {
            runtime::set_security_runtime_for_test(self.0.clone());
        }
    }
    let _reset = RuntimeReset((*runtime::get_security_runtime()).clone());
    for enabled in [false, true] {
        runtime::set_security_runtime_for_test(SecurityRuntimeConfig {
            enabled,
            ..Default::default()
        });
        let gateway =
            RestorationGateway::with_test_rules(&["chinese_phone_number"], 64 * 1024 * 1024);
        for (case, body) in [
            (
                "complete",
                json!({"input":"vary_cu_00000000000000000000_ai"}),
            ),
            ("malformed", json!({"input":"vary_cu_broken"})),
            (
                "nested_key",
                json!({"input":"synthetic", "metadata":{"vary_cu_00000000000000000000_ai":"synthetic"}}),
            ),
            (
                "escaped_argument",
                json!({"input":[{"type":"function_call","call_id":"synthetic-legacy-call","name":"non_executing_fixture","arguments":"{\"value\":\"\\u0076ary_cu_00000000000000000000_ai\"}"}]}),
            ),
            (
                "escaped_new_argument",
                json!({"input":[{"type":"function_call","call_id":"synthetic-new-call","name":"non_executing_fixture","arguments":"{\"value\":\"\\u0072redact-custom-00000000000000000000-ai\"}"}]}),
            ),
        ] {
            let mut source = body.clone();
            match admit_http_request(
                &gateway,
                &AppType::Codex,
                true,
                false,
                false,
                &mut source,
                &Default::default(),
            ) {
                Ok(RestorationAdmission::Admitted(request)) => {
                    let attempt = request
                        .prepare_attempt(
                            "synthetic-provider",
                            "openai_responses",
                            "https://synthetic.invalid/v1/responses",
                            None,
                            &mut source,
                            |candidate| Ok(candidate.clone()),
                        )
                        .map_err(|_| "literal_preparation_rejected")?;
                    assert!(!attempt.has_active_tokens(), "literal_granted_authority");
                }
                Ok(RestorationAdmission::Passthrough) if !enabled => {}
                Ok(RestorationAdmission::Intercepted(_)) => {
                    return Err(format!("literal_intercepted_{enabled}_{case}").into())
                }
                Ok(RestorationAdmission::Passthrough) => {
                    return Err(format!("literal_passthrough_{enabled}_{case}").into())
                }
                Err(crate::proxy::ProxyError::DlpForward(
                    super::http_restoration::DlpForwardError::ContextUnavailable,
                )) if enabled && matches!(case, "escaped_argument" | "escaped_new_argument") => {
                    // A lone call without its output is not independent completed history.
                }
                Err(_) => return Err(format!("literal_error_{enabled}_{case}").into()),
            }
            assert!(
                source == body,
                "legacy refusal must not mutate request input"
            );
        }
        if enabled {
            let mut source = json!({"input":"synthetic-safe-input"});
            let request = match admit_http_request(
                &gateway,
                &AppType::Codex,
                true,
                false,
                false,
                &mut source,
                &Default::default(),
            )? {
                RestorationAdmission::Admitted(request) => request,
                _ => return Err("synthetic control request must be admitted".into()),
            };
            source["metadata"] = json!({"nested":{"vary_cu_00000000000000000000_ai":"synthetic"}});
            let original = source.clone();
            let attempt = request
                .prepare_attempt(
                    "synthetic-provider",
                    "openai_responses",
                    "https://synthetic.invalid/v1/responses",
                    None,
                    &mut source,
                    |candidate| Ok(candidate.clone()),
                )
                .map_err(|_| "late_literal_preparation_rejected")?;
            assert!(
                !attempt.has_active_tokens(),
                "late_literal_granted_authority"
            );
            assert!(source == original, "late_literal_bytes_changed");
        }
    }
    Ok(())
}

#[test]
fn reference_format_escaped_unleased_arguments_preserve_decoded_literals() {
    use super::http_restoration::validate_unleased_enriched_body;
    use serde_json::json;

    for reference in [
        "\\u0076ary_cu_00000000000000000000_ai",
        "\\u0072redact-custom-00000000000000000000-ai",
        "\\u0076ary_cu_broken",
        "\\u0072redact-custom-broken",
    ] {
        for arguments in [
            format!("{{\"nested\":[{{\"value\":\"{reference}\"}}]}}"),
            format!("{{\"nested\":[{{\"{reference}\":\"synthetic\"}}]}}"),
        ] {
            let body = json!({"input":[{"type":"function_call","arguments":arguments}]});
            assert!(
                validate_unleased_enriched_body(&body).is_ok(),
                "decoded_literal_rejected"
            );
        }
    }
}

#[test]
fn reference_format_escaped_unleased_arguments_preserve_noncarriers() {
    use super::http_restoration::validate_unleased_enriched_body;
    use serde_json::json;

    for body in [
        json!({"input":"\\u0072redact-custom-00000000000000000000-ai"}),
        json!({"input":[{"type":"custom_tool_call","input":"{\"value\":\"\\u0072redact-custom-00000000000000000000-ai\"}"}]}),
        json!({"input":[{"type":"function_call","arguments":"{\"value\":\"synthetic\\ntext\"}"}]}),
        json!({"metadata":{"arguments":"{\"value\":\"\\u0072redact-custom-00000000000000000000-ai\"}"}}),
    ] {
        let before = body.clone();
        assert!(
            validate_unleased_enriched_body(&body).is_ok(),
            "nondependencies must remain unchanged"
        );
        assert!(
            body == before,
            "dependency inspection must not mutate input"
        );
    }
}

#[test]
#[serial_test::serial]
fn reference_format_escaped_admission_uses_shared_budget_and_releases_parse_owners(
) -> Result<(), Box<dyn std::error::Error>> {
    use super::{
        http_restoration::{admit_http_request, RestorationAdmission, RestorationGateway},
        runtime::{self, SecurityRuntimeConfig},
    };
    use crate::app_config::AppType;
    use serde_json::json;
    struct RuntimeReset(SecurityRuntimeConfig);
    impl Drop for RuntimeReset {
        fn drop(&mut self) {
            runtime::set_security_runtime_for_test(self.0.clone());
        }
    }
    let _reset = RuntimeReset((*runtime::get_security_runtime()).clone());
    runtime::set_security_runtime_for_test(SecurityRuntimeConfig::default());
    let budget = PayloadBudget::with_limit(64 * 1024);
    let gateway =
        RestorationGateway::with_test_store_and_rules(DlpTokenStore::new(), &[], budget.clone());
    let safe =
        json!({"input":[{"type":"function_call","arguments":"{\"value\":\"synthetic\\ntext\"}"}]});
    let held = budget.reserve(budget.limit())?;
    let mut body = safe.clone();
    assert!(
        matches!(
            admit_http_request(
                &gateway,
                &AppType::Codex,
                true,
                false,
                false,
                &mut body,
                &Default::default()
            ),
            Ok(RestorationAdmission::Intercepted(_))
        ),
        "escaped arguments must not bypass the shared budget"
    );
    assert!(body == safe, "budget refusal must preserve input");
    assert!(
        budget.used() == budget.limit(),
        "failed parsing must not leak reservations"
    );
    drop(held);
    assert!(budget.used() == 0, "released budget must be empty");
    assert!(
        matches!(
            admit_http_request(
                &gateway,
                &AppType::Codex,
                true,
                false,
                false,
                &mut body,
                &Default::default()
            ),
            Ok(RestorationAdmission::Passthrough)
        ),
        "safe decoded arguments must retain passthrough"
    );
    assert!(
        budget.used() == 0,
        "successful inspection must release reservations"
    );
    for arguments in [
        "{\"value\":\"\\u0072redact-custom-00000000000000000000-ai\"}",
        "{\"\\u0072redact-custom-00000000000000000000-ai\":\"synthetic\"}",
        "{\"value\":\"\\u0076ary_cu_00000000000000000000_ai\"}",
        "{\"value\":\"synthetic\\q\"}",
    ] {
        let mut source = json!({"input":[{"type":"function_call","arguments":arguments}]});
        let before = source.clone();
        let invalid_json = arguments.contains("\\q");
        let result = admit_http_request(
            &gateway,
            &AppType::Codex,
            true,
            false,
            false,
            &mut source,
            &Default::default(),
        );
        assert!(
            if invalid_json {
                matches!(result, Ok(RestorationAdmission::Intercepted(_)))
            } else {
                matches!(result, Ok(RestorationAdmission::Passthrough))
            },
            "decoded_literal_or_protocol_contract_changed"
        );
        assert!(source == before, "inspection refusal must preserve input");
        assert!(
            budget.used() == 0,
            "inspection refusal must release reservations"
        );
    }
    Ok(())
}
