//! Synthetic regressions for stable-toolchain compatibility and atomic accounting.
use super::{payload_budget::PayloadBudget, tokenization::TokenError};
use std::sync::{Arc, Barrier};

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[test]
fn accounting_avoids_deprecated_or_newer_atomic_update_apis() {
    for source in [
        include_str!("../payload_budget.rs"),
        include_str!("../token_store_owners.rs"),
        include_str!("../upstream_error.rs"),
    ] {
        assert!(
            !source.contains(&[".fetch_", "update("].concat()),
            "deprecated_atomic_update"
        );
        assert!(
            !source.contains(&[".try_", "update("].concat()),
            "atomic_update_exceeds_declared_msrv"
        );
    }
    assert!(
        !include_str!("../http_restoration.rs")
            .contains("map_or(HistoryLeafPolicy::Strict, &policy)"),
        "needless_history_policy_borrow"
    );
}

#[test]
fn payload_accounting_rejects_overflow_and_invalid_shrink_without_mutation() -> TestResult {
    let budget = PayloadBudget::with_limit(usize::MAX);
    let mut reservation = budget.reserve(usize::MAX)?;
    assert!(matches!(
        reservation.grow(1),
        Err(TokenError::CapacityExceeded)
    ));
    assert_eq!(budget.used(), usize::MAX, "overflow_changed_charge");
    reservation.shrink(usize::MAX)?;
    assert!(matches!(
        reservation.shrink(1),
        Err(TokenError::Unavailable)
    ));
    assert_eq!(budget.used(), 0, "underflow_changed_charge");
    drop(reservation);
    assert_eq!(budget.used(), 0, "drop_changed_released_charge");
    Ok(())
}

#[test]
fn concurrent_payload_admission_obeys_one_ceiling_and_releases_all_owners() -> TestResult {
    const OWNERS: usize = 8;
    const CEILING: usize = 4;
    let budget = PayloadBudget::with_limit(CEILING);
    let admitted = Arc::new(Barrier::new(OWNERS + 1));
    let release = Arc::new(Barrier::new(OWNERS + 1));
    let mut workers = Vec::new();
    for _ in 0..OWNERS {
        let budget = budget.clone();
        let admitted = admitted.clone();
        let release = release.clone();
        workers.push(std::thread::spawn(move || {
            let reservation = budget.reserve(1);
            admitted.wait();
            release.wait();
            reservation.is_ok()
        }));
    }
    admitted.wait();
    let observed = budget.used();
    release.wait();
    let mut successes = 0;
    for worker in workers {
        successes += usize::from(worker.join().map_err(|_| "synthetic_worker_failed")?);
    }
    assert_eq!(observed, CEILING, "concurrent_ceiling_changed");
    assert_eq!(successes, CEILING, "concurrent_admission_changed");
    assert_eq!(budget.used(), 0, "concurrent_owner_charge_retained");
    Ok(())
}
