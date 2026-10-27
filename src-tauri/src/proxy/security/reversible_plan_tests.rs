fn plan_request(
    fields: &[super::request_plan::PlanField<'_>],
    historical_plaintexts: &[&str],
) -> Result<
    super::payload_metadata::PayloadVec<super::request_plan::PlannedField>,
    super::request_plan::PlanError,
> {
    super::request_plan::plan_request_with_budget(
        fields,
        historical_plaintexts,
        super::payload_budget::PayloadBudget::with_limit(64 * 1024 * 1024),
    )
}

use super::request_plan::{EffectiveAction::*, PlanError, PlanField, SpanMatch};
use super::tokenization::{TokenCategory::*, TokenError};

fn hit(
    range: std::ops::Range<usize>,
    action: super::request_plan::EffectiveAction,
    category: super::tokenization::TokenCategory,
) -> SpanMatch {
    SpanMatch {
        range,
        action,
        category,
    }
}

#[test]
fn overlapping_components_choose_irreversible_and_keep_private_key_placeholder(
) -> Result<(), PlanError> {
    let matches = [hit(0..6, Restore, PrivateKey), hit(4..10, Redact, Custom)];
    let plan = plan_request(
        &[PlanField {
            text: "abcdefghij",
            writable: true,
            matches: &matches,
        }],
        &[],
    )?;
    assert_eq!(plan.as_slice()[0].spans.len(), 1);
    assert_eq!(plan.as_slice()[0].spans.as_slice()[0].range, 0..10);
    assert_eq!(plan.as_slice()[0].spans.as_slice()[0].action, Redact);
    assert!(plan.as_slice()[0].spans.as_slice()[0].private_key_placeholder);
    Ok(())
}

#[test]
fn category_selection_uses_complete_span_coverage_only() -> Result<(), PlanError> {
    let mixed = [
        hit(0..6, Restore, PrivateKey),
        hit(4..10, Restore, Database),
    ];
    let covered = [
        hit(0..10, Restore, Phone),
        hit(0..10, Restore, Password),
        hit(2..5, Restore, PrivateKey),
    ];
    let plan = plan_request(
        &[
            PlanField {
                text: "abcdefghij",
                writable: true,
                matches: &mixed,
            },
            PlanField {
                text: "klmnopqrst",
                writable: true,
                matches: &covered,
            },
        ],
        &[],
    )?;
    assert_eq!(plan.as_slice()[0].spans.as_slice()[0].category, Mixed);
    assert_eq!(plan.as_slice()[1].spans.as_slice()[0].category, Password);
    Ok(())
}

#[test]
fn adjacent_nonoverlapping_matches_do_not_merge() -> Result<(), PlanError> {
    let matches = [hit(0..2, Restore, Phone), hit(2..4, Restore, IdCard)];
    let plan = plan_request(
        &[PlanField {
            text: "abcd",
            writable: true,
            matches: &matches,
        }],
        &[],
    )?;
    assert_eq!(plan.as_slice()[0].spans.len(), 2);
    Ok(())
}

#[test]
fn containment_propagates_iteratively_across_fields_in_both_directions() -> Result<(), PlanError> {
    // The final field is related only after the middle field becomes irreversible.
    let matches = [hit(0..3, Redact, Custom)];
    let large = [hit(0..11, Restore, Custom)];
    let remote = [hit(0..4, Restore, Custom)];
    let plan = plan_request(
        &[
            PlanField {
                text: "abc",
                writable: true,
                matches: &matches,
            },
            PlanField {
                text: "tail",
                writable: true,
                matches: &remote,
            },
            PlanField {
                text: "abc--z-tail",
                writable: true,
                matches: &large,
            },
        ],
        &[],
    )?;
    assert!(plan.as_slice().iter().all(|field| field
        .spans
        .as_slice()
        .iter()
        .all(|span| span.action == Redact)));
    Ok(())
}

#[test]
fn equal_plaintext_cannot_be_both_reversible_and_irreversible() -> Result<(), PlanError> {
    let redact = [hit(0..4, Redact, Custom)];
    let restore = [hit(0..4, Restore, Phone)];
    let plan = plan_request(
        &[
            PlanField {
                text: "same",
                writable: true,
                matches: &redact,
            },
            PlanField {
                text: "same",
                writable: true,
                matches: &restore,
            },
        ],
        &[],
    )?;
    assert_eq!(plan.as_slice()[1].spans.as_slice()[0].action, Redact);
    Ok(())
}

#[test]
fn conflicting_historical_mapping_rejects_instead_of_expanding_plaintext() {
    let matches = [hit(0..3, Redact, Custom)];
    let result = plan_request(
        &[PlanField {
            text: "abc",
            writable: true,
            matches: &matches,
        }],
        &["old-abc-secret"],
    );
    assert_eq!(result.err(), Some(PlanError::PolicyConflict));
}

#[test]
fn block_wins_before_any_nonwritable_redaction_failure() {
    let readonly = [hit(0..3, Restore, Custom)];
    let blocked = [hit(0..3, Block, Custom)];
    let result = plan_request(
        &[
            PlanField {
                text: "abc",
                writable: false,
                matches: &readonly,
            },
            PlanField {
                text: "def",
                writable: true,
                matches: &blocked,
            },
        ],
        &[],
    );
    assert_eq!(result.err(), Some(PlanError::Blocked));
}

#[test]
fn protected_nonwritable_values_fail_but_audit_and_allow_do_not_mutate() {
    let protected = [hit(0..3, Redact, Custom)];
    assert_eq!(
        plan_request(
            &[PlanField {
                text: "abc",
                writable: false,
                matches: &protected
            }],
            &[]
        )
        .err(),
        Some(PlanError::Unsupported)
    );
    let observed = [hit(0..3, Audit, Custom), hit(0..3, Allow, PrivateKey)];
    let result = plan_request(
        &[PlanField {
            text: "abc",
            writable: false,
            matches: &observed,
        }],
        &[],
    );
    assert!(result.is_ok_and(|fields| fields.as_slice()[0].spans.as_slice().is_empty()));
}

#[test]
fn invalid_utf8_boundaries_empty_and_out_of_range_spans_fail_closed() {
    for range in [1..3, 0..0, 0..99, 5..2] {
        let matches = [hit(range, Restore, Custom)];
        assert_eq!(
            plan_request(
                &[PlanField {
                    text: "中文",
                    writable: true,
                    matches: &matches
                }],
                &[]
            )
            .err(),
            Some(PlanError::Token(TokenError::RedactionFailed))
        );
    }
}

#[test]
fn valid_utf8_offsets_remain_exact_and_input_order_is_not_semantic() -> Result<(), PlanError> {
    let matches = [hit(6..9, Restore, Phone), hit(0..6, Restore, Custom)];
    let plan = plan_request(
        &[PlanField {
            text: "中文abc",
            writable: true,
            matches: &matches,
        }],
        &[],
    )?;
    assert_eq!(plan.as_slice()[0].spans.as_slice()[0].range, 0..6);
    assert_eq!(plan.as_slice()[0].spans.as_slice()[1].range, 6..9);
    Ok(())
}

#[test]
fn planning_rejects_unbounded_match_metadata_before_allocating_components() {
    let matches: Vec<_> = (0..4097).map(|_| hit(0..1, Restore, Custom)).collect();
    assert_eq!(
        plan_request(
            &[PlanField {
                text: "a",
                writable: true,
                matches: &matches
            }],
            &[]
        )
        .err(),
        Some(PlanError::Token(TokenError::CapacityExceeded))
    );
}

#[test]
fn field_count_accepts_4096_and_rejects_4097() -> Result<(), PlanError> {
    let fields: Vec<_> = (0..4097)
        .map(|_| PlanField {
            text: "",
            writable: true,
            matches: &[],
        })
        .collect();
    assert_eq!(plan_request(&fields[..4096], &[])?.len(), 4096);
    assert_eq!(
        plan_request(&fields, &[]).err(),
        Some(PlanError::Token(TokenError::CapacityExceeded))
    );
    Ok(())
}

#[test]
fn historical_count_accepts_4096_and_rejects_4097() -> Result<(), PlanError> {
    let matches = [hit(0..1, Redact, Custom)];
    let fields = [PlanField {
        text: "a",
        writable: true,
        matches: &matches,
    }];
    let history = vec!["b"; 4097];
    assert!(plan_request(&fields, &history[..4096]).is_ok());
    assert_eq!(
        plan_request(&fields, &history).err(),
        Some(PlanError::Token(TokenError::CapacityExceeded))
    );
    Ok(())
}

#[test]
fn logical_field_one_mib_boundary_is_exact() -> Result<(), PlanError> {
    let text = "x".repeat(1024 * 1024 + 1);
    assert!(plan_request(
        &[PlanField {
            text: &text[..1024 * 1024],
            writable: true,
            matches: &[]
        }],
        &[]
    )
    .is_ok());
    assert_eq!(
        plan_request(
            &[PlanField {
                text: &text,
                writable: true,
                matches: &[]
            }],
            &[]
        )
        .err(),
        Some(PlanError::Token(TokenError::CapacityExceeded))
    );
    Ok(())
}

#[test]
fn comparison_work_budget_exhaustion_is_fail_closed() {
    let text = "a".repeat(1024 * 1024);
    let historical = "b".repeat(1024 * 1024);
    let matches = [hit(0..text.len(), Redact, Custom)];
    let fields = [PlanField {
        text: &text,
        writable: true,
        matches: &matches,
    }];
    let history = vec![historical.as_str(); 17];
    assert!(plan_request(&fields, &history[..16]).is_ok());
    assert_eq!(
        plan_request(&fields, &history).err(),
        Some(PlanError::Token(TokenError::CapacityExceeded))
    );
}

#[test]
fn propagation_exposes_indirect_historical_policy_conflict() {
    let redact = [hit(0..3, Redact, Custom)];
    let restore = [hit(0..11, Restore, Custom)];
    // The history is unrelated to the original irreversible span, but related
    // to the larger span after cross-field propagation makes it irreversible.
    let result = plan_request(
        &[
            PlanField {
                text: "abc",
                writable: true,
                matches: &redact,
            },
            PlanField {
                text: "abc--z-tail",
                writable: true,
                matches: &restore,
            },
        ],
        &["tail"],
    );
    assert_eq!(result.err(), Some(PlanError::PolicyConflict));
}

#[test]
fn review_regression_original_irreversible_subspan_survives_overlap_merging(
) -> Result<(), PlanError> {
    let overlapping = [hit(0..3, Redact, Custom), hit(2..6, Restore, Custom)];
    let candidate = [hit(0..5, Restore, Custom)];
    let plan = plan_request(
        &[
            PlanField {
                text: "abcdef",
                writable: true,
                matches: &overlapping,
            },
            PlanField {
                text: "abc-X",
                writable: true,
                matches: &candidate,
            },
        ],
        &[],
    )?;
    assert_eq!(plan.as_slice()[0].spans.as_slice()[0].action, Redact);
    assert_eq!(plan.as_slice()[1].spans.as_slice()[0].action, Redact);
    Ok(())
}

#[test]
fn review_regression_original_irreversible_subspan_rejects_conflicting_history() {
    let overlapping = [hit(0..3, Redact, Custom), hit(2..6, Restore, Custom)];
    let result = plan_request(
        &[PlanField {
            text: "abcdef",
            writable: true,
            matches: &overlapping,
        }],
        &["old-abc-secret"],
    );
    assert_eq!(result.err(), Some(PlanError::PolicyConflict));
}

fn one_span_plan_budget() -> (usize, usize) {
    use super::request_plan::{PlannedField, PlannedSpan};
    let retained = 4 * std::mem::size_of::<PlannedField>() + 4 * std::mem::size_of::<PlannedSpan>();
    let ordered = 4 * std::mem::size_of::<&SpanMatch>();
    let propagation = 4 * std::mem::size_of::<(usize, usize)>();
    (retained, retained + ordered.max(propagation))
}

#[test]
fn plan_owner_precharges_metadata_and_retains_only_final_plan() -> Result<(), PlanError> {
    use super::{
        payload_budget::PayloadBudget,
        payload_metadata::test_support::{self, Action},
        request_plan::plan_request_with_budget,
    };
    let matches = [hit(0..3, Restore, Phone)];
    let fields = [PlanField {
        text: "abc",
        writable: true,
        matches: &matches,
    }];
    let (retained, peak) = one_span_plan_budget();
    let budget = PayloadBudget::with_limit(peak);
    let hits = std::rc::Rc::new(std::cell::Cell::new(0));
    let seen = std::rc::Rc::clone(&hits);
    let _hook = test_support::install(move |event| {
        assert!(event.used >= event.bytes && event.used <= peak);
        seen.set(seen.get() + 1);
        Action::Continue
    });
    let plan = plan_request_with_budget(&fields, &[], budget.clone())?;
    assert_eq!(plan.len(), 1);
    assert_eq!(budget.used(), retained);
    assert!(hits.get() >= 6);
    drop(plan);
    assert_eq!(budget.used(), 0);
    Ok(())
}

#[test]
fn plan_owner_difference_one_peak_and_zero_budget_fail_without_retention() {
    use super::{payload_budget::PayloadBudget, request_plan::plan_request_with_budget};
    let matches = [hit(0..3, Restore, Phone)];
    let fields = [PlanField {
        text: "abc",
        writable: true,
        matches: &matches,
    }];
    let (_, peak) = one_span_plan_budget();
    for limit in [0, peak - 1] {
        let budget = PayloadBudget::with_limit(limit);
        assert_eq!(
            plan_request_with_budget(&fields, &[], budget.clone()).err(),
            Some(PlanError::Token(TokenError::CapacityExceeded))
        );
        assert_eq!(budget.used(), 0);
    }
}

#[test]
fn plan_owner_allocation_errors_and_unwinds_release_partial_plans() {
    use super::{
        payload_budget::PayloadBudget,
        payload_metadata::test_support::{self, Action, Phase},
        request_plan::plan_request_with_budget,
    };
    let matches = [hit(0..3, Restore, Phone)];
    let fields = [PlanField {
        text: "abc",
        writable: true,
        matches: &matches,
    }];
    let (_, peak) = one_span_plan_budget();
    for phase in [Phase::BeforeAllocation, Phase::AfterAllocation] {
        for unwind in [false, true] {
            let budget = PayloadBudget::with_limit(peak);
            let hits = std::rc::Rc::new(std::cell::Cell::new(0));
            let seen = std::rc::Rc::clone(&hits);
            let _hook = test_support::install(move |event| {
                if event.phase == phase {
                    seen.set(seen.get() + 1);
                    if unwind {
                        panic!("synthetic plan allocation unwind");
                    }
                    return Action::Fail;
                }
                Action::Continue
            });
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                plan_request_with_budget(&fields, &[], budget.clone())
            }));
            assert_eq!(hits.get(), 1);
            if unwind {
                assert!(result.is_err());
            } else {
                assert_eq!(
                    result.ok().and_then(Result::err),
                    Some(PlanError::Token(TokenError::Unavailable))
                );
            }
            assert_eq!(budget.used(), 0);
        }
    }
}
