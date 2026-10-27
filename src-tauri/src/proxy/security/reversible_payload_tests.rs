//! Synthetic-only tests for restoration payload accounting; no payload integration is implied.
use super::{
    payload_budget::{PayloadBudget, Reservation},
    tokenization::TokenError,
};
use std::{
    future::{pending, Future},
    panic::{catch_unwind, AssertUnwindSafe},
    sync::{Arc, Barrier},
    task::{Context, Poll, Wake, Waker},
    thread,
};

#[test]
fn default_handles_share_the_exact_global_64_mib_ceiling() -> Result<(), TokenError> {
    let first = PayloadBudget::default();
    let second = PayloadBudget::default();
    assert_eq!(first.limit(), 64 * 1024 * 1024);
    assert_eq!(second.limit(), first.limit());
    assert_eq!(first.used(), 0);
    let reservation = first.reserve(first.limit())?;
    let independent = PayloadBudget::with_limit(3);
    let independent_owner = independent.reserve(3)?;
    assert_eq!(second.used(), first.limit());
    assert!(matches!(
        second.reserve(1),
        Err(TokenError::CapacityExceeded)
    ));
    drop(reservation);
    assert_eq!(second.used(), 0);
    assert_eq!(independent.used(), 3);
    let next = second.reserve(second.limit())?;
    drop(independent_owner);
    assert_eq!(independent.used(), 0);
    assert_eq!(first.used(), first.limit());
    drop(next);
    // Exercise the production default-budget entry point only in this global probe.
    {
        let store = super::tokenization::DlpTokenStore::new();
        let mut lease = store.admit("codex", None, 1)?;
        let prepared = super::request_preparation::prepare_codex_request(
            &mut lease,
            &serde_json::json!({"input": "synthetic"}),
            &super::runtime::SecurityRuntimeConfig::default(),
            &[],
            &[],
            |_, bytes, _| {
                let (step, leaf, _) = super::request_carriers::metadata_sizes();
                let carrier_copies = 2 * (2 * 128 + 512 + 5 + 9 + 4 * step + 5 + 4 * leaf);
                let logical_owners = 4 * super::request_preparation::logical_field_size()
                    + 4 * std::mem::size_of::<super::request_plan::PlanField<'_>>()
                    + 4 * std::mem::size_of::<super::request_plan::PlannedField>()
                    + 4 * std::mem::size_of::<super::payload_text::PayloadText>()
                    + 9;
                assert_eq!(
                    first.used(),
                    bytes.len() + carrier_copies + (2 * 128 + 512 + 5 + 9) + logical_owners
                );
                Ok(())
            },
        )
        .map_err(|_| TokenError::Unavailable)?;
        assert_eq!(
            first.used(),
            prepared.encoded.capacity() + (2 * 128 + 512 + 5 + 9)
        );
        assert!(first.used() > 0);
        drop(prepared);
    }
    {
        // The sole global probe also covers the restoration staging path.
        let store = super::tokenization::DlpTokenStore::new();
        let mut lease = store.admit("codex", None, 1)?;
        let blocked = first.reserve(first.limit())?;
        let body = serde_json::json!({"input": "13800138000"});
        let runtime = super::runtime::SecurityRuntimeConfig::default();
        let denied = super::request_preparation::prepare_codex_request(
            &mut lease,
            &body,
            &runtime,
            &["chinese_phone_number"],
            &[],
            |_, _, _| -> Result<(), TokenError> {
                panic!("exhausted global budget must not reach validation")
            },
        );
        assert!(matches!(
            denied,
            Err(super::request_plan::PlanError::Token(
                TokenError::CapacityExceeded
            ))
        ));
        assert_eq!(store.stats()?.entries, 0);
        assert_eq!(first.used(), first.limit());
        drop(blocked);
        let prepared = super::request_preparation::prepare_codex_request(
            &mut lease,
            &body,
            &runtime,
            &["chinese_phone_number"],
            &[],
            |_, bytes, proof| {
                let (step, leaf, _) = super::request_carriers::metadata_sizes();
                let carriers = 2 * (2 * 128 + 512 + 5 + 4 * step + 5 + 4 * leaf) + 11 + 31;
                let owners = 4 * super::request_preparation::logical_field_size()
                    + 4 * std::mem::size_of::<super::request_plan::PlanField<'_>>()
                    + 4 * std::mem::size_of::<super::request_plan::PlannedField>()
                    + 4 * std::mem::size_of::<super::payload_text::PayloadText>()
                    + 31
                    + 4 * std::mem::size_of::<super::request_plan::SpanMatch>()
                    + 4 * std::mem::size_of::<super::request_plan::PlannedSpan>()
                    + 4 * std::mem::size_of::<super::tokenization::TokenFragment<'_>>()
                    + 4 * std::mem::size_of::<&str>();
                let scratch = 4 * std::mem::size_of::<[u8; 10]>()
                    + 4 * std::mem::size_of::<
                        Option<super::token_store_owners::SharedAllocation<()>>,
                    >()
                    + 8 * std::mem::size_of::<usize>()
                    + 31
                    + 4 * std::mem::size_of::<super::payload_text::PayloadText>();
                assert_eq!(proof.len(), 1);
                assert_eq!(store.stats()?.entries, 0);
                assert_eq!(
                    first.used(),
                    bytes.len() + carriers + owners + scratch + 2 * 128 + 512 + 5 + 31
                );
                Ok(())
            },
        )
        .map_err(|_| TokenError::Unavailable)?;
        assert_eq!(
            first.used(),
            prepared.encoded.capacity() + 2 * 128 + 512 + 5 + 31
        );
        assert_eq!(store.stats()?.entries, 1);
        drop(prepared);
        assert_eq!(first.used(), 0);
    }
    {
        let body = serde_json::json!({"input":"synthetic"});
        let carriers = super::request_carriers::CodexRequestCarriers::from_body(&body)?;
        assert!(first.used() > 0);
        drop(carriers);
        let fields = [super::request_plan::PlanField {
            text: "synthetic",
            writable: true,
            matches: &[],
        }];
        let plan =
            super::request_plan::plan_request(&fields, &[]).map_err(|_| TokenError::Unavailable)?;
        assert!(first.used() > 0);
        drop(plan);
        let store = super::tokenization::DlpTokenStore::new();
        let mut lease = store.admit("codex", None, 1)?;
        let prepared =
            super::request_transaction::prepare_fields(&mut lease, &fields, &[], |_, _| Ok(()))
                .map_err(|_| TokenError::Unavailable)?;
        assert_eq!(
            first.used(),
            4 * std::mem::size_of::<super::payload_text::PayloadText>() + 9
        );
        drop(prepared);
    }
    {
        let runtime = super::runtime::SecurityRuntimeConfig::default();
        let found = super::request_detection::detect_spans("13800138000", false, &runtime)?;
        assert_eq!(
            first.used(),
            4 * std::mem::size_of::<super::request_detection::DetectedSpan<'_>>()
        );
        drop(found);
    }
    assert_eq!(PayloadBudget::default().used(), 0);
    Ok(())
}

#[test]
fn reservation_accepts_the_exact_ceiling_and_rejects_one_byte_over() -> Result<(), TokenError> {
    let budget = PayloadBudget::with_limit(7);
    let first = budget.reserve(3)?;
    let second = budget.reserve(4)?;
    assert_eq!(first.bytes(), 3);
    assert_eq!(second.bytes(), 4);
    assert_eq!(budget.used(), 7);
    assert!(matches!(
        budget.reserve(1),
        Err(TokenError::CapacityExceeded)
    ));
    assert!(matches!(
        budget.reserve(8),
        Err(TokenError::CapacityExceeded)
    ));
    assert_eq!(budget.used(), 7);
    drop(first);
    assert_eq!(budget.used(), 4);
    drop(second);
    assert_eq!(budget.used(), 0);
    Ok(())
}

#[test]
fn zero_limit_allows_only_zero_and_zero_growth_or_shrink() -> Result<(), TokenError> {
    let budget = PayloadBudget::with_limit(0);
    let mut empty = budget.reserve(0)?;
    assert_eq!(empty.bytes(), 0);
    empty.grow(0)?;
    empty.shrink(0)?;
    assert!(matches!(
        budget.reserve(1),
        Err(TokenError::CapacityExceeded)
    ));
    assert!(matches!(empty.grow(1), Err(TokenError::CapacityExceeded)));
    assert_eq!(empty.shrink(1), Err(TokenError::Unavailable));
    assert_eq!(empty.bytes(), 0);
    drop(empty);
    assert_eq!(budget.used(), 0);
    Ok(())
}

#[test]
fn zero_reservation_is_allowed_even_when_the_budget_is_full() -> Result<(), TokenError> {
    let budget = PayloadBudget::with_limit(1);
    let full = budget.reserve(1)?;
    let mut empty = budget.reserve(0)?;
    empty.grow(0)?;
    empty.shrink(0)?;
    drop(empty);
    assert_eq!(budget.used(), 1);
    drop(full);
    assert_eq!(budget.used(), 0);
    Ok(())
}

#[test]
fn reservation_sum_overflow_is_rejected_without_changing_existing_charges() -> Result<(), TokenError>
{
    let budget = PayloadBudget::with_limit(usize::MAX);
    let first = budget.reserve(usize::MAX - 1)?;
    let last = budget.reserve(1)?;
    assert_eq!(budget.used(), usize::MAX);
    assert!(matches!(
        budget.reserve(1),
        Err(TokenError::CapacityExceeded)
    ));
    assert!(matches!(
        budget.reserve(usize::MAX),
        Err(TokenError::CapacityExceeded)
    ));
    assert_eq!(budget.used(), usize::MAX);
    drop(first);
    assert_eq!(budget.used(), 1);
    drop(last);
    assert_eq!(budget.used(), 0);
    Ok(())
}

#[test]
fn growth_and_shrink_adjust_only_the_requested_bytes() -> Result<(), TokenError> {
    let budget = PayloadBudget::with_limit(10);
    let mut reservation = budget.reserve(3)?;
    reservation.grow(7)?;
    assert_eq!(reservation.bytes(), 10);
    assert_eq!(budget.used(), 10);
    reservation.shrink(4)?;
    assert_eq!(reservation.bytes(), 6);
    assert_eq!(budget.used(), 6);
    reservation.grow(0)?;
    reservation.shrink(0)?;
    assert_eq!(budget.used(), 6);
    reservation.shrink(6)?;
    assert_eq!(reservation.bytes(), 0);
    assert_eq!(budget.used(), 0);
    reservation.grow(10)?;
    drop(reservation);
    assert_eq!(budget.used(), 0);
    Ok(())
}

#[test]
fn failed_growth_preserves_the_original_charge_and_can_be_retried() -> Result<(), TokenError> {
    let budget = PayloadBudget::with_limit(8);
    let mut first = budget.reserve(3)?;
    let second = budget.reserve(4)?;
    assert_eq!(first.grow(2), Err(TokenError::CapacityExceeded));
    assert_eq!(first.bytes(), 3);
    assert_eq!(budget.used(), 7);
    drop(second);
    first.grow(5)?;
    assert_eq!(first.bytes(), 8);
    assert_eq!(budget.used(), 8);
    drop(first);
    assert_eq!(budget.used(), 0);
    Ok(())
}

#[test]
fn growth_checks_both_owner_and_aggregate_overflow() -> Result<(), TokenError> {
    let budget = PayloadBudget::with_limit(usize::MAX);
    let mut owner = budget.reserve(usize::MAX)?;
    assert_eq!(owner.grow(1), Err(TokenError::CapacityExceeded));
    assert_eq!(owner.bytes(), usize::MAX);
    assert_eq!(budget.used(), usize::MAX);
    owner.shrink(1)?;
    let other = budget.reserve(1)?;
    assert_eq!(owner.grow(1), Err(TokenError::CapacityExceeded));
    assert_eq!(owner.bytes(), usize::MAX - 1);
    assert_eq!(budget.used(), usize::MAX);
    drop(other);
    owner.grow(1)?;
    drop(owner);
    assert_eq!(budget.used(), 0);
    Ok(())
}

#[test]
fn excessive_shrink_never_underflows_or_releases_another_owners_charge() -> Result<(), TokenError> {
    let budget = PayloadBudget::with_limit(8);
    let mut first = budget.reserve(3)?;
    let second = budget.reserve(5)?;
    for invalid in [4, 8, usize::MAX] {
        assert_eq!(first.shrink(invalid), Err(TokenError::Unavailable));
        assert_eq!(first.bytes(), 3);
        assert_eq!(budget.used(), 8);
    }
    first.shrink(3)?;
    assert_eq!(budget.used(), 5);
    drop(first);
    assert_eq!(budget.used(), 5);
    drop(second);
    assert_eq!(budget.used(), 0);
    Ok(())
}

#[test]
fn moving_a_reservation_transfers_its_single_charge() -> Result<(), TokenError> {
    fn transfer(reservation: Reservation) -> Reservation {
        reservation
    }
    let budget = PayloadBudget::with_limit(8);
    let reservation = budget.reserve(8)?;
    let moved = transfer(reservation);
    assert_eq!(moved.bytes(), 8);
    assert_eq!(budget.used(), 8);
    let mut slot = Some(moved);
    let transferred = slot.take().ok_or(TokenError::Unavailable)?;
    drop(slot);
    assert_eq!(budget.used(), 8);
    drop(transferred);
    assert_eq!(budget.used(), 0);
    Ok(())
}

#[test]
fn dropping_a_scope_or_returning_an_error_releases_its_reservations() -> Result<(), TokenError> {
    fn fail_after_reserving(budget: &PayloadBudget) -> Result<(), TokenError> {
        let _reservation = budget.reserve(5)?;
        Err(TokenError::Unavailable)
    }
    let budget = PayloadBudget::with_limit(8);
    {
        let _first = budget.reserve(3)?;
        let _second = budget.reserve(5)?;
        assert_eq!(budget.used(), 8);
    }
    assert_eq!(budget.used(), 0);
    assert_eq!(fail_after_reserving(&budget), Err(TokenError::Unavailable));
    assert_eq!(budget.used(), 0);
    Ok(())
}

#[test]
fn unwind_releases_the_owned_reservation() -> Result<(), TokenError> {
    let budget = PayloadBudget::with_limit(8);
    let reservation = budget.reserve(8)?;
    let result = catch_unwind(AssertUnwindSafe(move || {
        let _reservation = reservation;
        panic!("synthetic payload unwind");
    }));
    assert!(result.is_err());
    assert_eq!(budget.used(), 0);
    let recovered = budget.reserve(8)?;
    drop(recovered);
    assert_eq!(budget.used(), 0);
    Ok(())
}

struct NoopWake;
impl Wake for NoopWake {
    fn wake(self: Arc<Self>) {}
}

#[test]
fn cancelling_a_pending_future_releases_its_reservation_without_a_runtime() -> Result<(), TokenError>
{
    let budget = PayloadBudget::with_limit(8);
    let reservation = budget.reserve(8)?;
    let mut operation = Box::pin(async move {
        let _reservation = reservation;
        pending::<()>().await;
    });
    let waker = Waker::from(Arc::new(NoopWake));
    assert!(matches!(
        operation.as_mut().poll(&mut Context::from_waker(&waker)),
        Poll::Pending
    ));
    assert_eq!(budget.used(), 8);
    drop(operation);
    assert_eq!(budget.used(), 0);
    Ok(())
}

#[test]
fn cancelling_an_unpolled_future_also_releases_its_reservation() -> Result<(), TokenError> {
    let budget = PayloadBudget::with_limit(8);
    let reservation = budget.reserve(8)?;
    let operation = async move {
        let _reservation = reservation;
        pending::<()>().await;
    };
    assert_eq!(budget.used(), 8);
    drop(operation);
    assert_eq!(budget.used(), 0);
    Ok(())
}

#[test]
fn independent_budgets_are_isolated() -> Result<(), TokenError> {
    let first = PayloadBudget::with_limit(3);
    let second = PayloadBudget::with_limit(5);
    let first_owner = first.reserve(3)?;
    let second_owner = second.reserve(5)?;
    assert_eq!(first.used(), 3);
    assert_eq!(second.used(), 5);
    drop(first_owner);
    assert_eq!(first.used(), 0);
    assert_eq!(second.used(), 5);
    drop(second_owner);
    assert_eq!(second.used(), 0);
    Ok(())
}

#[test]
fn cloning_or_dropping_budget_handles_does_not_duplicate_or_release_charges(
) -> Result<(), TokenError> {
    let first = PayloadBudget::with_limit(8);
    let reservation = first.reserve(8)?;
    let second = first.clone();
    let observer = second.clone();
    assert_eq!(second.used(), 8);
    assert!(matches!(
        second.reserve(1),
        Err(TokenError::CapacityExceeded)
    ));
    drop(first);
    drop(second);
    assert_eq!(observer.used(), 8);
    drop(reservation);
    assert_eq!(observer.used(), 0);
    Ok(())
}

#[test]
fn reservation_can_outlive_all_budget_handles() -> Result<(), TokenError> {
    let mut reservation = {
        let budget = PayloadBudget::with_limit(8);
        budget.reserve(3)?
    };
    reservation.grow(5)?;
    assert_eq!(reservation.bytes(), 8);
    reservation.shrink(8)?;
    assert_eq!(reservation.bytes(), 0);
    drop(reservation);
    Ok(())
}

#[test]
fn immutable_shared_owner_releases_only_after_the_last_arc() -> Result<(), TokenError> {
    struct SharedPayload {
        _reservation: Reservation,
        bytes: [u8; 8],
    }
    let budget = PayloadBudget::with_limit(16);
    let owner = Arc::new(SharedPayload {
        _reservation: budget.reserve(8)?,
        bytes: [0; 8],
    });
    let shared = Arc::clone(&owner);
    assert_eq!(owner.bytes, shared.bytes);
    assert_eq!(budget.used(), 8);
    let distinct_copy = SharedPayload {
        _reservation: budget.reserve(shared.bytes.len())?,
        bytes: shared.bytes,
    };
    assert_eq!(budget.used(), 16);
    drop(owner);
    assert_eq!(budget.used(), 16);
    drop(shared);
    assert_eq!(budget.used(), 8);
    drop(distinct_copy);
    assert_eq!(budget.used(), 0);
    Ok(())
}

#[test]
fn concurrent_reservations_never_exceed_the_aggregate_ceiling() -> Result<(), TokenError> {
    const WORKERS: usize = 16;
    const LIMIT: usize = 11;
    const CHARGE: usize = 3;
    let budget = PayloadBudget::with_limit(LIMIT);
    let barrier = Arc::new(Barrier::new(WORKERS + 1));
    let mut handles = Vec::new();
    for _ in 0..WORKERS {
        let budget = budget.clone();
        let barrier = Arc::clone(&barrier);
        handles.push(thread::spawn(move || {
            barrier.wait();
            let reservation = budget.reserve(CHARGE);
            barrier.wait();
            barrier.wait();
            reservation
        }));
    }
    barrier.wait();
    barrier.wait();
    let peak = budget.used();
    barrier.wait();
    let mut successes = Vec::new();
    for handle in handles {
        match handle.join() {
            Ok(Ok(reservation)) => successes.push(reservation),
            Ok(Err(TokenError::CapacityExceeded)) => {}
            _ => panic!("unexpected synthetic worker failure"),
        }
    }
    assert_eq!(peak, (LIMIT / CHARGE) * CHARGE);
    assert!(peak <= LIMIT);
    assert_eq!(successes.len(), LIMIT / CHARGE);
    assert_eq!(budget.used(), successes.len() * CHARGE);
    drop(successes);
    assert_eq!(budget.used(), 0);
    Ok(())
}

#[test]
fn concurrent_growth_and_shrink_preserve_aggregate_accounting() -> Result<(), TokenError> {
    const WORKERS: usize = 16;
    let budget = PayloadBudget::with_limit(WORKERS);
    let reservations = (0..WORKERS)
        .map(|_| budget.reserve(0))
        .collect::<Result<Vec<_>, _>>()?;
    let barrier = Arc::new(Barrier::new(WORKERS + 1));
    let mut handles = Vec::new();
    for mut reservation in reservations {
        let barrier = Arc::clone(&barrier);
        handles.push(thread::spawn(move || -> Result<(), TokenError> {
            let mut failure = None;
            for _ in 0..64 {
                let growth = reservation.grow(1);
                barrier.wait();
                barrier.wait();
                let shrink = if growth.is_ok() {
                    reservation.shrink(1)
                } else {
                    Ok(())
                };
                failure = failure.or(growth.err()).or(shrink.err());
                barrier.wait();
                barrier.wait();
            }
            failure.map_or(Ok(()), Err)
        }));
    }
    let mut observations = Vec::new();
    for _ in 0..64 {
        barrier.wait();
        let full = budget.used();
        barrier.wait();
        barrier.wait();
        let empty = budget.used();
        barrier.wait();
        observations.push((full, empty));
    }
    for handle in handles {
        match handle.join() {
            Ok(result) => result?,
            Err(_) => panic!("unexpected synthetic worker failure"),
        }
    }
    for (full, empty) in observations {
        assert_eq!(full, WORKERS);
        assert_eq!(empty, 0);
    }
    assert_eq!(budget.used(), 0);
    Ok(())
}
