use super::{
    payload_budget::PayloadBudget,
    payload_text::PayloadText,
    request_plan::{EffectiveAction::*, PlanError, PlanField, SpanMatch},
    request_transaction::{prepare_fields_with_budget, PreparedFields},
    tokenization::DlpRequestLease,
    tokenization::{DlpTokenStore, TokenCategory::*, TokenError, TokenFragment},
};

fn prepare_fields<T>(
    lease: &mut DlpRequestLease,
    fields: &[PlanField<'_>],
    history: &[&str],
    validate: impl FnMut(&[PayloadText], &[&str]) -> Result<T, TokenError>,
) -> Result<PreparedFields<T>, PlanError> {
    prepare_fields_with_budget(
        lease,
        fields,
        history,
        PayloadBudget::with_limit(64 * 1024 * 1024),
        validate,
    )
}

fn restore(range: std::ops::Range<usize>) -> SpanMatch {
    SpanMatch {
        range,
        action: Restore,
        category: Phone,
    }
}

#[test]
fn logical_transaction_prepares_exact_references_and_returns_validated_candidate(
) -> Result<(), PlanError> {
    let store = DlpTokenStore::with_test_key(std::array::from_fn(|index| index as u8));
    let mut lease = store.admit("codex", Some("synthetic-session"), 1)?;
    let matches = [restore(3..14)];
    let fields = [PlanField {
        text: "pre13800138000post",
        writable: true,
        matches: &matches,
    }];
    let prepared = prepare_fields(&mut lease, &fields, &[], |candidate, proof| {
        assert_eq!(
            candidate[0].as_str(),
            "prevary_ph_413ff0a68f6f0d574cb5_aipost"
        );
        assert_eq!(proof.len(), 1);
        Ok(42usize)
    })?;
    assert_eq!(prepared.validated, 42);
    assert_eq!(prepared.reversible_occurrences, 1);
    assert_eq!(prepared.irreversible_occurrences, 0);
    assert_eq!(
        lease.resolve("vary_ph_413ff0a68f6f0d574cb5_ai")?,
        "13800138000"
    );
    assert_eq!(fields[0].text, "pre13800138000post");
    Ok(())
}

#[test]
fn failed_candidate_validation_rolls_back_mapping_and_authority() -> Result<(), PlanError> {
    let store = DlpTokenStore::new();
    let mut lease = store.admit("codex", Some("synthetic-session"), 1)?;
    let matches = [restore(0..11)];
    let fields = [PlanField {
        text: "13800138000",
        writable: true,
        matches: &matches,
    }];
    let mut proposed = String::new();
    let result = prepare_fields(&mut lease, &fields, &[], |_, proof| {
        proposed = proof[0].to_owned();
        Err::<(), _>(TokenError::RedactionFailed)
    });
    assert_eq!(
        result.err(),
        Some(PlanError::Token(TokenError::RedactionFailed))
    );
    assert_eq!(store.stats()?.entries, 0);
    assert_eq!(
        lease.resolve(&proposed).err(),
        Some(TokenError::ContextUnavailable)
    );
    Ok(())
}

#[test]
fn irreversible_containment_allocates_no_reversible_mapping() -> Result<(), PlanError> {
    let store = DlpTokenStore::new();
    let mut lease = store.admit("codex", Some("synthetic-session"), 1)?;
    let first = [SpanMatch {
        range: 0..3,
        action: Redact,
        category: PrivateKey,
    }];
    let second = [restore(0..7)];
    let prepared = prepare_fields(
        &mut lease,
        &[
            PlanField {
                text: "abc",
                writable: true,
                matches: &first,
            },
            PlanField {
                text: "x-abc-y",
                writable: true,
                matches: &second,
            },
        ],
        &[],
        |_, proof| {
            assert!(proof.is_empty());
            Ok(())
        },
    )?;
    assert_eq!(
        prepared
            .fields
            .as_slice()
            .iter()
            .map(PayloadText::as_str)
            .collect::<Vec<_>>(),
        ["[REDACTED_PRIVATE_KEY]", "[REDACTED]"]
    );
    assert_eq!(prepared.reversible_occurrences, 0);
    assert_eq!(prepared.irreversible_occurrences, 2);
    assert_eq!(store.stats()?.entries, 0);
    Ok(())
}

#[test]
fn authorized_history_is_retained_but_does_not_grant_sibling_lease_authority(
) -> Result<(), PlanError> {
    let store = DlpTokenStore::new();
    let mut lease = store.admit("codex", Some("synthetic-session"), 1)?;
    let tokens = lease.prepare(
        &[TokenFragment {
            raw: "synthetic-secret",
            whole_span_categories: &[Custom],
        }],
        &[],
        |tokens| Ok(tokens.to_vec()),
    )?;
    let fields = [PlanField {
        text: &tokens[0],
        writable: true,
        matches: &[],
    }];
    let prepared = prepare_fields(&mut lease, &fields, &[&tokens[0]], |_, proof| {
        assert_eq!(proof.len(), 1);
        Ok(())
    })?;
    assert_eq!(prepared.fields.as_slice()[0].as_str(), tokens[0]);
    let mut sibling = store.admit("codex", Some("synthetic-session"), 1)?;
    let rejected = prepare_fields(&mut sibling, &fields, &[&tokens[0]], |_, _| Ok(()));
    assert_eq!(
        rejected.err(),
        Some(PlanError::Token(TokenError::ContextUnavailable))
    );
    Ok(())
}

#[test]
fn literal_collision_rejects_atomically_before_any_candidate_commit() -> Result<(), PlanError> {
    let store = DlpTokenStore::with_test_key(std::array::from_fn(|index| index as u8));
    let mut lease = store.admit("codex", None, 1)?;
    let matches = [restore(0..11)];
    let result = prepare_fields(
        &mut lease,
        &[
            PlanField {
                text: "13800138000",
                writable: true,
                matches: &matches,
            },
            PlanField {
                text: "vary_ph_413ff0a68f6f0d574cb5_ai",
                writable: true,
                matches: &[],
            },
        ],
        &[],
        |_, _| Ok(()),
    );
    assert_eq!(
        result.err(),
        Some(PlanError::Token(TokenError::TokenCollision))
    );
    assert_eq!(store.stats()?.entries, 0);
    Ok(())
}

#[test]
fn malformed_and_unknown_exact_tokens_fail_without_issuing_a_mapping() -> Result<(), PlanError> {
    let store = DlpTokenStore::new();
    let mut lease = store.admit("codex", None, 1)?;
    for text in ["vary_ph_broken", "vary_ph_00000000000000000000_ai"] {
        let result = prepare_fields(
            &mut lease,
            &[PlanField {
                text,
                writable: true,
                matches: &[],
            }],
            &[],
            |_, _| Ok(()),
        );
        assert_eq!(
            result.err(),
            Some(PlanError::Token(TokenError::ContextUnavailable))
        );
    }
    assert_eq!(store.stats()?.entries, 0);
    Ok(())
}

#[test]
fn leased_history_conflicting_with_irreversible_text_is_policy_error() -> Result<(), PlanError> {
    let store = DlpTokenStore::new();
    let mut lease = store.admit("codex", None, 1)?;
    let tokens = lease.prepare(
        &[TokenFragment {
            raw: "synthetic-abc-secret",
            whole_span_categories: &[Custom],
        }],
        &[],
        |tokens| Ok(tokens.to_vec()),
    )?;
    let matches = [SpanMatch {
        range: 0..3,
        action: Redact,
        category: Custom,
    }];
    let result = prepare_fields(
        &mut lease,
        &[PlanField {
            text: "abc",
            writable: true,
            matches: &matches,
        }],
        &[&tokens[0]],
        |_, _| Ok(()),
    );
    assert_eq!(result.err(), Some(PlanError::PolicyConflict));
    assert_eq!(store.stats()?.entries, 1);
    Ok(())
}

#[test]
fn arbitrary_vary_identifiers_are_not_reserved_or_changed() -> Result<(), PlanError> {
    let store = DlpTokenStore::new();
    let mut lease = store.admit("codex", None, 1)?;
    let text = "vary_user_setting vary_zz_not_reserved";
    let prepared = prepare_fields(
        &mut lease,
        &[PlanField {
            text,
            writable: true,
            matches: &[],
        }],
        &[],
        |_, proof| {
            assert!(proof.is_empty());
            Ok(())
        },
    )?;
    assert_eq!(prepared.fields.as_slice()[0].as_str(), text);
    Ok(())
}

#[test]
fn repeated_exact_plaintext_reuses_one_mapping_and_one_proof() -> Result<(), PlanError> {
    let store = DlpTokenStore::new();
    let mut lease = store.admit("codex", None, 1)?;
    let matches = [restore(0..11), restore(12..23)];
    let prepared = prepare_fields(
        &mut lease,
        &[PlanField {
            text: "13800138000 13800138000",
            writable: true,
            matches: &matches,
        }],
        &[],
        |candidate, proof| {
            assert_eq!(proof.len(), 1);
            assert_eq!(candidate[0].as_str(), format!("{} {}", proof[0], proof[0]));
            Ok(())
        },
    )?;
    assert_eq!(prepared.reversible_occurrences, 2);
    assert_eq!(store.stats()?.entries, 1);
    Ok(())
}

#[test]
fn final_validation_runs_without_store_lock_and_retries_with_committed_category(
) -> Result<(), PlanError> {
    let store = DlpTokenStore::new();
    let mut lease = store.admit("codex", Some("synthetic-session"), 1)?;
    let mut concurrent = store.admit("codex", Some("synthetic-session"), 1)?;
    let matches = [restore(0..11)];
    let mut attempts = 0;
    let prepared = prepare_fields(
        &mut lease,
        &[PlanField {
            text: "13800138000",
            writable: true,
            matches: &matches,
        }],
        &[],
        |candidate, proof| {
            attempts += 1;
            assert_eq!(candidate[0].as_str(), proof[0]);
            assert_eq!(store.stats()?.entries, if attempts == 1 { 0 } else { 1 });
            if attempts == 1 {
                concurrent.prepare(
                    &[TokenFragment {
                        raw: "13800138000",
                        whole_span_categories: &[PrivateKey],
                    }],
                    &[],
                    |_| Ok(()),
                )?;
            }
            Ok(candidate[0].as_str().to_owned())
        },
    )?;
    assert_eq!(attempts, 2);
    assert!(prepared.validated.starts_with("vary_pk_"));
    assert_eq!(prepared.fields.as_slice()[0].as_str(), prepared.validated);
    assert_eq!(lease.resolve(&prepared.validated)?, "13800138000");
    assert_eq!(store.stats()?.entries, 1);
    Ok(())
}

#[test]
fn candidate_growth_limits_fail_before_validation_and_commit() -> Result<(), PlanError> {
    let store = DlpTokenStore::new();
    let mut lease = store.admit("codex", None, 1)?;
    let text = "x".repeat(1024 * 1024);
    let matches = [restore(0..1)];
    let mut validations = 0;
    let result = prepare_fields(
        &mut lease,
        &[PlanField {
            text: &text,
            writable: true,
            matches: &matches,
        }],
        &[],
        |_, _| {
            validations += 1;
            Ok(())
        },
    );
    assert_eq!(
        result.err(),
        Some(PlanError::Token(TokenError::CapacityExceeded))
    );
    assert_eq!(validations, 0);
    assert_eq!(store.stats()?.entries, 0);
    assert_eq!(lease.authority_capacity_for_test(), 0);
    Ok(())
}

#[test]
fn total_candidate_growth_is_bounded_across_individually_valid_fields() -> Result<(), PlanError> {
    let store = DlpTokenStore::new();
    let mut lease = store.admit("codex", None, 1)?;
    let text = "x".repeat(1024 * 1024 - 30);
    let matches = [restore(0..1)];
    let mut fields: Vec<_> = (0..4)
        .map(|_| PlanField {
            text: text.as_str(),
            writable: true,
            matches: &matches[..],
        })
        .collect();
    fields.push(PlanField {
        text: "z",
        writable: true,
        matches: &[],
    });
    let result = prepare_fields(&mut lease, &fields, &[], |_, _| Ok(()));
    assert_eq!(
        result.err(),
        Some(PlanError::Token(TokenError::CapacityExceeded))
    );
    assert_eq!(store.stats()?.entries, 0);
    Ok(())
}

#[test]
fn fields_and_token_occurrence_counts_fail_closed_at_the_limit() -> Result<(), PlanError> {
    let store = DlpTokenStore::new();
    let mut lease = store.admit("codex", None, 1)?;
    let mut fields: Vec<_> = (0..4096)
        .map(|_| PlanField {
            text: "",
            writable: true,
            matches: &[],
        })
        .collect();
    assert_eq!(
        prepare_fields(&mut lease, &fields, &[], |_, _| Ok(()))?
            .fields
            .len(),
        4096
    );
    fields.push(PlanField {
        text: "",
        writable: true,
        matches: &[],
    });
    assert_eq!(
        prepare_fields(&mut lease, &fields, &[], |_, _| Ok(())).err(),
        Some(PlanError::Token(TokenError::CapacityExceeded))
    );
    let tokens = lease.prepare(
        &[TokenFragment {
            raw: "synthetic-secret",
            whole_span_categories: &[Custom],
        }],
        &[],
        |tokens| Ok(tokens.to_vec()),
    )?;
    let at_limit = tokens[0].repeat(4096);
    assert!(prepare_fields(
        &mut lease,
        &[PlanField {
            text: &at_limit,
            writable: true,
            matches: &[],
        }],
        &[],
        |_, _| Ok(())
    )
    .is_ok());
    let over_limit = tokens[0].repeat(4097);
    assert_eq!(
        prepare_fields(
            &mut lease,
            &[PlanField {
                text: &over_limit,
                writable: true,
                matches: &[],
            }],
            &[],
            |_, _| Ok(())
        )
        .err(),
        Some(PlanError::Token(TokenError::CapacityExceeded))
    );
    Ok(())
}

#[test]
fn callback_unwind_rolls_back_without_poisoning_the_store() -> Result<(), PlanError> {
    let store = DlpTokenStore::new();
    let mut lease = store.admit("codex", None, 1)?;
    let matches = [restore(0..11)];
    let unwind = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _ = prepare_fields(
            &mut lease,
            &[PlanField {
                text: "13800138000",
                writable: true,
                matches: &matches,
            }],
            &[],
            |_, _| -> Result<(), TokenError> { panic!("synthetic candidate validation panic") },
        );
    }));
    assert!(unwind.is_err());
    assert_eq!(store.stats()?.entries, 0);
    assert_eq!(lease.authority_capacity_for_test(), 0);
    Ok(())
}

#[test]
fn transaction_owner_retains_candidate_charge_until_drop() -> Result<(), PlanError> {
    use super::{
        payload_budget::PayloadBudget, payload_text::PayloadText,
        request_transaction::prepare_fields_with_budget,
    };
    let budget = PayloadBudget::with_limit(1 << 20);
    let store = DlpTokenStore::new();
    let mut lease = store.admit("codex", None, 1)?;
    let matches = [restore(0..11)];
    let prepared = prepare_fields_with_budget(
        &mut lease,
        &[PlanField {
            text: "13800138000",
            writable: true,
            matches: &matches,
        }],
        &[],
        budget.clone(),
        |_, _| {
            assert!(budget.used() >= 31 + 4 * std::mem::size_of::<PayloadText>());
            assert_eq!(store.stats()?.entries, 0);
            Ok(())
        },
    )?;
    assert_eq!(budget.used(), 31 + 4 * std::mem::size_of::<PayloadText>());
    drop(prepared);
    assert_eq!(budget.used(), 0);
    Ok(())
}

#[test]
fn transaction_owner_zero_budget_rejects_before_validation_and_commit() -> Result<(), PlanError> {
    use super::{payload_budget::PayloadBudget, request_transaction::prepare_fields_with_budget};
    let budget = PayloadBudget::with_limit(0);
    let store = DlpTokenStore::new();
    let mut lease = store.admit("codex", None, 1)?;
    let matches = [restore(0..11)];
    let mut called = false;
    let result = prepare_fields_with_budget(
        &mut lease,
        &[PlanField {
            text: "13800138000",
            writable: true,
            matches: &matches,
        }],
        &[],
        budget.clone(),
        |_, _| {
            called = true;
            Ok(())
        },
    );
    assert_eq!(
        result.err(),
        Some(PlanError::Token(TokenError::CapacityExceeded))
    );
    assert!(!called);
    assert_eq!(store.stats()?.entries, 0);
    assert_eq!(lease.authority_capacity_for_test(), 0);
    assert_eq!(budget.used(), 0);
    Ok(())
}

#[test]
fn transaction_owner_failure_and_unwind_release_candidate_before_commit() -> Result<(), PlanError> {
    use super::{
        payload_budget::PayloadBudget,
        payload_text::test_support::{self, Action, Phase},
        request_transaction::prepare_fields_with_budget,
    };
    let budget = PayloadBudget::with_limit(1 << 20);
    let store = DlpTokenStore::new();
    let mut lease = store.admit("codex", None, 1)?;
    let matches = [restore(0..11)];
    for unwind in [false, true] {
        let guard = test_support::install(move |seen| {
            if seen.phase == Phase::BeforeCopy {
                if unwind {
                    panic!("synthetic transaction allocation unwind");
                }
                return Action::Fail;
            }
            Action::Continue
        });
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            prepare_fields_with_budget(
                &mut lease,
                &[PlanField {
                    text: "13800138000",
                    writable: true,
                    matches: &matches,
                }],
                &[],
                budget.clone(),
                |_, _| Ok(()),
            )
        }));
        drop(guard);
        if unwind {
            assert!(result.is_err());
        } else {
            assert_eq!(
                result.ok().and_then(Result::err),
                Some(PlanError::Token(TokenError::Unavailable))
            );
        }
        assert_eq!(budget.used(), 0);
        assert_eq!(store.stats()?.entries, 0);
        assert_eq!(lease.authority_capacity_for_test(), 0);
    }
    Ok(())
}

#[test]
fn transaction_owner_exact_peak_and_difference_one_are_independent() -> Result<(), PlanError> {
    use super::request_plan::{PlannedField, PlannedSpan};
    use std::mem::size_of;
    let retained = 31 + 4 * size_of::<PayloadText>();
    let live_plan = 4 * size_of::<PlannedField>() + 4 * size_of::<PlannedSpan>();
    let scratch = 4 * size_of::<[u8; 10]>()
        + 4 * size_of::<Option<super::token_store_owners::SharedAllocation<()>>>()
        + 8 * size_of::<usize>()
        + 31
        + 4 * size_of::<PayloadText>();
    let peak =
        live_plan + 4 * size_of::<TokenFragment<'_>>() + retained + 4 * size_of::<&str>() + scratch;
    for limit in [peak - 1, peak] {
        let store = DlpTokenStore::new();
        let mut lease = store.admit("codex", None, 1)?;
        let budget = PayloadBudget::with_limit(limit);
        let matches = [restore(0..11)];
        let mut validated = false;
        let result = prepare_fields_with_budget(
            &mut lease,
            &[PlanField {
                text: "13800138000",
                writable: true,
                matches: &matches,
            }],
            &[],
            budget.clone(),
            |_, _| {
                assert_eq!(budget.used(), peak);
                validated = true;
                Ok(())
            },
        );
        if limit == peak {
            let prepared = result?;
            assert!(validated);
            assert_eq!(budget.used(), retained);
            assert_eq!(store.stats()?.entries, 1);
            drop(prepared);
        } else {
            assert_eq!(
                result.err(),
                Some(PlanError::Token(TokenError::CapacityExceeded))
            );
            assert!(!validated);
            assert_eq!(store.stats()?.entries, 0);
            assert_eq!(lease.authority_capacity_for_test(), 0);
        }
        assert_eq!(budget.used(), 0);
    }
    Ok(())
}

#[test]
fn transaction_owner_history_reference_containers_remain_charged_during_validation(
) -> Result<(), PlanError> {
    let store = DlpTokenStore::new();
    let mut lease = store.admit("codex", None, 1)?;
    let tokens = lease.prepare(
        &[TokenFragment {
            raw: "synthetic-secret",
            whole_span_categories: &[Custom],
        }],
        &[],
        |tokens| Ok(tokens.to_vec()),
    )?;
    let peak = 8 * std::mem::size_of::<&str>();
    let budget = PayloadBudget::with_limit(peak);
    let prepared = prepare_fields_with_budget(
        &mut lease,
        &[],
        &[&tokens[0]],
        budget.clone(),
        |candidate, proof| {
            assert!(candidate.is_empty());
            assert_eq!(proof, [&tokens[0]]);
            assert_eq!(budget.used(), peak);
            Ok(())
        },
    )?;
    assert_eq!(budget.used(), 0);
    drop(prepared);
    assert_eq!(lease.resolve(&tokens[0])?, "synthetic-secret");
    Ok(())
}

#[test]
fn transaction_store_scratch_is_live_under_original_request_budget() -> Result<(), PlanError> {
    use super::request_plan::{PlannedField, PlannedSpan};
    use std::mem::size_of;
    let retained = 31 + 4 * size_of::<PayloadText>();
    let scratch = 4 * size_of::<[u8; 10]>()
        + 4 * size_of::<Option<super::token_store_owners::SharedAllocation<()>>>()
        + 8 * size_of::<usize>()
        + 31
        + 4 * size_of::<PayloadText>();
    let peak = 4 * size_of::<PlannedField>()
        + 4 * size_of::<PlannedSpan>()
        + 4 * size_of::<TokenFragment<'_>>()
        + retained
        + 4 * size_of::<&str>()
        + scratch;
    let store = DlpTokenStore::new();
    let mut lease = store.admit("codex", None, 1)?;
    let budget = PayloadBudget::with_limit(peak);
    let matches = [restore(0..11)];
    let prepared = prepare_fields_with_budget(
        &mut lease,
        &[PlanField {
            text: "13800138000",
            writable: true,
            matches: &matches,
        }],
        &[],
        budget.clone(),
        |_, _| {
            assert_eq!(budget.used(), peak);
            assert_eq!(store.stats()?.entries, 0);
            Ok(())
        },
    )?;
    assert_eq!(budget.used(), retained);
    drop(prepared);
    assert_eq!(budget.used(), 0);
    Ok(())
}
