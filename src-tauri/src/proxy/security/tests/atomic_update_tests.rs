//! Synthetic atomic-update contract tests, including deterministic CAS interference.
use super::atomic_update::try_update_usize;
use std::sync::atomic::{AtomicUsize, Ordering};

#[test]
fn atomic_update_returns_previous_value_and_decline_preserves_counter() {
    let counter = AtomicUsize::new(7);
    assert_eq!(
        try_update_usize(&counter, Ordering::Relaxed, Ordering::Relaxed, |value| {
            value.checked_add(2)
        }),
        Ok(7),
        "previous_value_changed"
    );
    assert_eq!(counter.load(Ordering::Relaxed), 9, "update_not_applied");
    assert_eq!(
        try_update_usize(&counter, Ordering::Relaxed, Ordering::Relaxed, |_| None),
        Err(9),
        "declined_value_changed"
    );
    assert_eq!(
        counter.load(Ordering::Relaxed),
        9,
        "decline_mutated_counter"
    );
}

#[test]
fn atomic_update_retries_against_observed_value_after_interference() {
    let counter = AtomicUsize::new(1);
    let mut calls = 0;
    let result = try_update_usize(&counter, Ordering::AcqRel, Ordering::Acquire, |value| {
        calls += 1;
        if calls == 1 {
            counter.store(10, Ordering::Release);
        }
        value.checked_add(1)
    });
    assert_eq!(result, Ok(10), "stale_value_committed");
    assert!(calls >= 2, "interference_not_retried");
    assert_eq!(counter.load(Ordering::Acquire), 11, "retry_value_changed");
}

#[test]
fn atomic_update_can_decline_after_interference_without_overwriting_counter() {
    let counter = AtomicUsize::new(1);
    let mut first = true;
    let result = try_update_usize(&counter, Ordering::Relaxed, Ordering::Relaxed, |value| {
        if first {
            first = false;
            counter.store(4, Ordering::Relaxed);
        }
        value.checked_add(1).filter(|next| *next <= 4)
    });
    assert_eq!(result, Err(4), "retry_decline_value_changed");
    assert_eq!(
        counter.load(Ordering::Relaxed),
        4,
        "retry_decline_mutated_counter"
    );
}

#[test]
fn atomic_update_preserves_release_decrement_and_saturated_owner_behavior() {
    let counter = AtomicUsize::new(1);
    assert_eq!(
        try_update_usize(&counter, Ordering::Release, Ordering::Relaxed, |value| {
            (value != usize::MAX).then(|| value - 1)
        }),
        Ok(1),
        "last_owner_not_selected"
    );
    std::sync::atomic::fence(Ordering::Acquire);
    assert_eq!(counter.load(Ordering::Relaxed), 0, "owner_not_released");
    counter.store(usize::MAX, Ordering::Relaxed);
    assert_eq!(
        try_update_usize(&counter, Ordering::Relaxed, Ordering::Relaxed, |value| {
            Some(value.saturating_add(1))
        }),
        Ok(usize::MAX),
        "saturated_clone_changed"
    );
    assert_eq!(
        try_update_usize(&counter, Ordering::Release, Ordering::Relaxed, |value| {
            (value != usize::MAX).then(|| value - 1)
        }),
        Err(usize::MAX),
        "saturated_owner_released"
    );
    assert_eq!(
        counter.load(Ordering::Relaxed),
        usize::MAX,
        "saturated_count_wrapped"
    );
}
