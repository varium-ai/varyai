//! Synthetic-only tests for independent token-store allocation ownership.
use super::{
    token_store_owners::{AllocationCharge, StoreAllocationLedger},
    tokenization::TokenError,
};
use std::{
    panic::{catch_unwind, AssertUnwindSafe},
    sync::{Arc, Barrier},
};

fn charge(plaintext: usize, metadata: usize) -> AllocationCharge {
    AllocationCharge {
        plaintext,
        metadata,
    }
}

#[test]
fn ledger_bootstrap_is_precharged_and_exact_metadata_boundary_is_enforced() -> Result<(), TokenError>
{
    let control = StoreAllocationLedger::control_bytes()?;
    assert!(control >= 4 * std::mem::size_of::<usize>());
    assert_eq!(
        StoreAllocationLedger::new(0, control - 1).err(),
        Some(TokenError::CapacityExceeded)
    );
    let ledger = StoreAllocationLedger::new(0, control)?;
    assert_eq!(ledger.used(), charge(0, control));
    let zero = ledger.reserve(charge(0, 0))?;
    assert_eq!(
        ledger.reserve(charge(0, 1)).err(),
        Some(TokenError::CapacityExceeded)
    );
    assert_eq!(
        ledger.reserve(charge(1, 0)).err(),
        Some(TokenError::CapacityExceeded)
    );
    drop(zero);
    assert_eq!(ledger.used(), charge(0, control));
    Ok(())
}

#[test]
fn ledger_distinct_planes_share_limits_across_clones_and_roll_back_partial_denial(
) -> Result<(), TokenError> {
    let control = StoreAllocationLedger::control_bytes()?;
    let ledger = StoreAllocationLedger::new(7, control + 11)?;
    let other = ledger.clone();
    let first = ledger.reserve(charge(3, 5))?;
    assert_eq!(other.used(), charge(3, control + 5));
    assert_eq!(
        other.reserve(charge(4, 7)).err(),
        Some(TokenError::CapacityExceeded)
    );
    assert_eq!(ledger.used(), charge(3, control + 5));
    let second = other.reserve(charge(4, 6))?;
    assert_eq!(ledger.used(), charge(7, control + 11));
    assert_eq!(
        ledger.reserve(charge(1, 0)).err(),
        Some(TokenError::CapacityExceeded)
    );
    assert_eq!(
        ledger.reserve(charge(0, 1)).err(),
        Some(TokenError::CapacityExceeded)
    );
    drop(first);
    assert_eq!(other.used(), charge(4, control + 6));
    drop(second);
    assert_eq!(other.used(), charge(0, control));
    Ok(())
}

#[test]
fn ledger_checked_overflow_cannot_change_existing_owners() -> Result<(), TokenError> {
    let ledger = StoreAllocationLedger::new(usize::MAX, usize::MAX)?;
    let before = ledger.used();
    let owner = ledger.reserve(charge(1, 1))?;
    assert_eq!(
        ledger.reserve(charge(usize::MAX, 0)).err(),
        Some(TokenError::CapacityExceeded)
    );
    assert_eq!(
        ledger.reserve(charge(0, usize::MAX)).err(),
        Some(TokenError::CapacityExceeded)
    );
    assert_eq!(ledger.used(), charge(1, before.metadata + 1));
    drop(owner);
    assert_eq!(ledger.used(), before);
    Ok(())
}

#[test]
fn ledger_guards_outlive_original_handle_and_release_on_unwind() -> Result<(), TokenError> {
    let control = StoreAllocationLedger::control_bytes()?;
    let ledger = StoreAllocationLedger::new(7, control + 11)?;
    let observer = ledger.clone();
    let owner = ledger.reserve(charge(7, 11))?;
    drop(ledger);
    assert_eq!(observer.used(), charge(7, control + 11));
    assert!(catch_unwind(AssertUnwindSafe(|| {
        let _owner = owner;
        panic!("synthetic allocation guard interruption");
    }))
    .is_err());
    assert_eq!(observer.used(), charge(0, control));
    drop(observer.reserve(charge(7, 11))?);
    assert_eq!(observer.used(), charge(0, control));
    Ok(())
}

#[test]
fn ledger_concurrent_denials_never_leak_partial_plane_charges() -> Result<(), TokenError> {
    let control = StoreAllocationLedger::control_bytes()?;
    let ledger = StoreAllocationLedger::new(32, control + 16)?;
    let barrier = Arc::new(Barrier::new(9));
    let mut workers = Vec::new();
    for _ in 0..8 {
        let ledger = ledger.clone();
        let barrier = barrier.clone();
        workers.push(std::thread::spawn(move || -> Result<(), TokenError> {
            barrier.wait();
            for _ in 0..128 {
                match ledger.reserve(charge(2, 1)) {
                    Ok(owner) => {
                        let used = ledger.used();
                        assert!(used.plaintext <= 32 && used.metadata <= control + 16);
                        std::thread::yield_now();
                        drop(owner);
                    }
                    Err(TokenError::CapacityExceeded) => {}
                    Err(error) => return Err(error),
                }
                assert_eq!(
                    ledger.reserve(charge(1, 17)).err(),
                    Some(TokenError::CapacityExceeded)
                );
            }
            Ok(())
        }));
    }
    barrier.wait();
    for worker in workers {
        assert!(matches!(worker.join(), Ok(Ok(()))));
    }
    assert_eq!(ledger.used(), charge(0, control));
    Ok(())
}

use super::token_store_owners::{arc_backing_bytes, SharedAllocation, StoreText, StoreVec};

#[test]
fn store_vector_precharges_complete_old_and_new_growth_peak() -> Result<(), TokenError> {
    let control = StoreAllocationLedger::control_bytes()?;
    let width = std::mem::size_of::<usize>();
    for available in [12 * width - 1, 12 * width] {
        let ledger = StoreAllocationLedger::new(0, control + available)?;
        let mut owner = StoreVec::<usize>::new(ledger.clone(), 8);
        for value in 0..4 {
            owner.push(value)?;
        }
        assert_eq!(owner.capacity(), 4);
        assert_eq!(ledger.used(), charge(0, control + 4 * width));
        let result = owner.push(4);
        if available == 12 * width {
            result?;
            assert_eq!(owner.as_slice(), &[0, 1, 2, 3, 4]);
            assert_eq!(owner.capacity(), 8);
            assert_eq!(ledger.used(), charge(0, control + 8 * width));
        } else {
            assert_eq!(result.err(), Some(TokenError::CapacityExceeded));
            assert_eq!(owner.as_slice(), &[0, 1, 2, 3]);
            assert_eq!(owner.capacity(), 4);
            assert_eq!(ledger.used(), charge(0, control + 4 * width));
        }
        drop(owner);
        assert_eq!(ledger.used(), charge(0, control));
    }
    Ok(())
}

#[test]
fn store_vector_bound_and_checked_capacity_fail_without_touching_elements() -> Result<(), TokenError>
{
    let control = StoreAllocationLedger::control_bytes()?;
    let ledger = StoreAllocationLedger::new(0, control + 1024)?;
    let mut owner = StoreVec::new(ledger.clone(), 1);
    owner.push(7u64)?;
    assert_eq!(owner.push(8).err(), Some(TokenError::CapacityExceeded));
    assert_eq!(owner.as_slice(), &[7]);
    assert!(StoreVec::<u64>::with_capacity(usize::MAX, ledger.clone(), usize::MAX).is_err());
    drop(owner);
    let mut empty = StoreVec::<()>::new(ledger.clone(), 1);
    empty.push(())?;
    assert_eq!(empty.push(()).err(), Some(TokenError::CapacityExceeded));
    assert_eq!(ledger.used(), charge(0, control));
    Ok(())
}

#[test]
fn store_text_copies_only_exact_utf8_length_under_plaintext_plane() -> Result<(), TokenError> {
    let control = StoreAllocationLedger::control_bytes()?;
    let source = "synthetic-é-尾";
    for limit in [source.len() - 1, source.len()] {
        let ledger = StoreAllocationLedger::new(limit, control)?;
        let result = StoreText::try_from_str(source, ledger.clone(), source.len());
        if limit < source.len() {
            assert_eq!(result.err(), Some(TokenError::CapacityExceeded));
        } else {
            let owner = result?;
            assert!(owner.as_str() == source);
            assert!(owner.as_str().as_ptr() != source.as_ptr());
            assert_eq!(owner.capacity(), source.len());
            assert_eq!(ledger.used(), charge(source.len(), control));
            assert_eq!(
                StoreText::try_from_str(source, ledger.clone(), source.len()).err(),
                Some(TokenError::CapacityExceeded)
            );
            drop(owner);
        }
        assert_eq!(ledger.used(), charge(0, control));
    }
    Ok(())
}

#[test]
fn shared_backing_is_charged_once_and_survives_until_last_clone_drops() -> Result<(), TokenError> {
    let control = StoreAllocationLedger::control_bytes()?;
    let bytes = arc_backing_bytes::<StoreText>()?;
    let ledger = StoreAllocationLedger::new(7, control + bytes)?;
    let raw = StoreText::try_from_str("fixture", ledger.clone(), 7)?;
    let first = SharedAllocation::new(raw, ledger.clone())?;
    let second = first.clone();
    let third = second.clone();
    assert!(first.get()?.as_str() == "fixture");
    assert_eq!(ledger.used(), charge(7, control + bytes));
    drop(first);
    drop(second);
    assert_eq!(ledger.used(), charge(7, control + bytes));
    drop(third);
    assert_eq!(ledger.used(), charge(0, control));
    Ok(())
}

struct DropProbe {
    ledger: StoreAllocationLedger,
    expected: AllocationCharge,
    panic: bool,
}
impl Drop for DropProbe {
    fn drop(&mut self) {
        assert_eq!(self.ledger.used(), self.expected);
        if self.panic {
            panic!("synthetic shared payload interruption");
        }
    }
}

#[test]
fn shared_backing_release_follows_payload_drop_even_during_unwind() -> Result<(), TokenError> {
    let control = StoreAllocationLedger::control_bytes()?;
    let bytes = arc_backing_bytes::<DropProbe>()?;
    for panic in [false, true] {
        let ledger = StoreAllocationLedger::new(0, control + bytes)?;
        let owner = SharedAllocation::new(
            DropProbe {
                ledger: ledger.clone(),
                expected: charge(0, control + bytes),
                panic,
            },
            ledger.clone(),
        )?;
        let result = catch_unwind(AssertUnwindSafe(|| drop(owner)));
        assert_eq!(result.is_err(), panic);
        assert_eq!(ledger.used(), charge(0, control));
    }
    Ok(())
}

#[test]
fn shared_backing_denial_drops_unpublished_payload_and_retains_no_charge() -> Result<(), TokenError>
{
    let control = StoreAllocationLedger::control_bytes()?;
    let bytes = arc_backing_bytes::<StoreText>()?;
    let ledger = StoreAllocationLedger::new(7, control + bytes - 1)?;
    let raw = StoreText::try_from_str("fixture", ledger.clone(), 7)?;
    assert_eq!(
        SharedAllocation::new(raw, ledger.clone()).err(),
        Some(TokenError::CapacityExceeded)
    );
    assert_eq!(ledger.used(), charge(0, control));
    Ok(())
}

#[test]
fn shared_backing_concurrent_final_drops_release_exactly_once() -> Result<(), TokenError> {
    let control = StoreAllocationLedger::control_bytes()?;
    let ledger = StoreAllocationLedger::new(7, control + arc_backing_bytes::<StoreText>()?)?;
    let owner = SharedAllocation::new(
        StoreText::try_from_str("fixture", ledger.clone(), 7)?,
        ledger.clone(),
    )?;
    let barrier = Arc::new(Barrier::new(9));
    let mut workers = Vec::new();
    for _ in 0..8 {
        let owner = owner.clone();
        let barrier = barrier.clone();
        workers.push(std::thread::spawn(move || {
            barrier.wait();
            drop(owner);
        }));
    }
    drop(owner);
    barrier.wait();
    for worker in workers {
        assert!(worker.join().is_ok());
    }
    assert_eq!(ledger.used(), charge(0, control));
    Ok(())
}

use super::tokenization::{DlpTokenStore, TokenCategory, TokenFragment};

fn synthetic_fragment(raw: &str) -> TokenFragment<'_> {
    TokenFragment {
        raw,
        whole_span_categories: &[TokenCategory::Custom],
    }
}

#[test]
fn store_mapping_backing_is_created_after_validation_and_retained_until_expiry(
) -> Result<(), TokenError> {
    let store = DlpTokenStore::new();
    let ledger = store.allocation_ledger_for_test()?;
    let baseline = ledger.used();
    let mut lease = store.admit("codex", None, 1)?;
    let admitted = charge(0, baseline.metadata + store.namespace_slot_bytes_for_test());
    let mut issued = String::new();
    lease.prepare(&[synthetic_fragment("fixture")], &[], |tokens| {
        assert_eq!(ledger.used(), admitted);
        assert_eq!(store.stats()?.entries, 0);
        issued = tokens[0].clone();
        Ok(())
    })?;
    assert_eq!(
        ledger.used(),
        charge(
            7,
            admitted.metadata
                + store.mapping_backing_bytes_for_test()?
                + 2 * std::mem::size_of::<([u8; 10], SharedAllocation<()>)>()
        )
    );
    assert!(lease.resolve(&issued)? == "fixture");
    drop(lease);
    assert_eq!(ledger.used().plaintext, 7);
    store.expire_idle_at(std::time::Instant::now() + std::time::Duration::from_secs(1801))?;
    assert_eq!(ledger.used(), baseline);
    Ok(())
}

#[test]
fn store_mapping_backing_capacity_denial_occurs_after_validation_without_new_authority(
) -> Result<(), TokenError> {
    let store = DlpTokenStore::new();
    let ledger = store.allocation_ledger_for_test()?;
    let mut lease = store.admit("codex", None, 1)?;
    let old = lease.prepare(&[synthetic_fragment("synthetic-pinned")], &[], |tokens| {
        Ok(tokens[0].clone())
    })?;
    let before = ledger.used();
    let logical_before = store.stats()?;
    let ceiling = ledger.limits_for_test();
    let blocker = ledger.reserve(charge(0, ceiling.metadata - before.metadata))?;
    let mut validated = false;
    let result = lease.prepare(&[synthetic_fragment("synthetic-denied")], &[], |_| {
        validated = true;
        assert_eq!(ledger.used().plaintext, before.plaintext);
        Ok(())
    });
    assert!(validated);
    assert_eq!(result.err(), Some(TokenError::CapacityExceeded));
    assert_eq!(ledger.used(), charge(before.plaintext, ceiling.metadata));
    assert_eq!(store.stats()?, logical_before);
    let mut authority_count = 0;
    lease.visit_authorized_tokens(|_| {
        authority_count += 1;
        Ok(())
    })?;
    assert_eq!(authority_count, 1);
    assert!(lease.resolve(&old)? == "synthetic-pinned");
    drop(blocker);
    assert_eq!(ledger.used(), before);
    lease.prepare(&[synthetic_fragment("synthetic-denied")], &[], |_| Ok(()))?;
    assert_eq!(store.stats()?.entries, 2);
    Ok(())
}

#[test]
fn store_mapping_backing_validation_error_unwind_and_commit_failure_release_new_copies(
) -> Result<(), TokenError> {
    for mode in 0..3 {
        let store = DlpTokenStore::new();
        let ledger = store.allocation_ledger_for_test()?;
        let mut lease = store.admit("codex", None, 1)?;
        let old = lease.prepare(&[synthetic_fragment("synthetic-existing")], &[], |tokens| {
            Ok(tokens[0].clone())
        })?;
        let before = ledger.used();
        let logical_before = store.stats()?;
        if mode == 2 {
            store.fail_next_mapping_reserve_for_test();
        }
        let mut called = false;
        let result = catch_unwind(AssertUnwindSafe(|| {
            lease.prepare(
                &[synthetic_fragment("synthetic-new")],
                &[],
                |_| -> Result<(), TokenError> {
                    called = true;
                    assert_eq!(ledger.used(), before);
                    match mode {
                        0 => Err(TokenError::RedactionFailed),
                        1 => panic!("synthetic mapping validation interruption"),
                        _ => Ok(()),
                    }
                },
            )
        }));
        assert!(called);
        match mode {
            0 => assert!(matches!(result, Ok(Err(TokenError::RedactionFailed)))),
            1 => assert!(result.is_err()),
            _ => assert!(matches!(result, Ok(Err(TokenError::CapacityExceeded)))),
        }
        assert_eq!(ledger.used(), before);
        assert_eq!(store.stats()?, logical_before);
        assert_eq!(lease.authorized_tokens()?.len(), 1);
        assert!(lease.resolve(&old)? == "synthetic-existing");
    }
    Ok(())
}

#[test]
fn mapping_and_authority_indices_retain_separate_exact_backing() -> Result<(), TokenError> {
    let store = DlpTokenStore::new();
    let ledger = store.allocation_ledger_for_test()?;
    let mut lease = store.admit("codex", Some("synthetic-index"), 1)?;
    let token = lease.prepare(&[synthetic_fragment("fixture")], &[], |tokens| {
        Ok(tokens[0].clone())
    })?;
    let both = ledger.used();
    let slot = std::mem::size_of::<([u8; 10], SharedAllocation<()>)>();
    let mut second = store.admit("codex", Some("synthetic-index"), 1)?;
    second.prepare(&[synthetic_fragment("fixture")], &[], |_| Ok(()))?;
    assert_eq!(ledger.used(), charge(both.plaintext, both.metadata + slot));
    assert!(second.resolve(&token)? == "fixture");
    drop(second);
    assert_eq!(ledger.used(), both);
    drop(lease);
    assert_eq!(ledger.used(), charge(both.plaintext, both.metadata - slot));
    Ok(())
}

#[test]
fn existing_mapping_authority_index_denial_is_after_validation_and_atomic() -> Result<(), TokenError>
{
    let store = DlpTokenStore::new();
    let ledger = store.allocation_ledger_for_test()?;
    let mut first = store.admit("codex", Some("synthetic-index-denial"), 1)?;
    first.prepare(&[synthetic_fragment("fixture")], &[], |_| Ok(()))?;
    let mut second = store.admit("codex", Some("synthetic-index-denial"), 1)?;
    let before = ledger.used();
    let logical = store.stats()?;
    let ceiling = ledger.limits_for_test();
    let blocker = ledger.reserve(charge(0, ceiling.metadata - before.metadata))?;
    let mut called = false;
    let result = second.prepare(&[synthetic_fragment("fixture")], &[], |_| {
        called = true;
        Ok(())
    });
    assert!(called);
    assert_eq!(result.err(), Some(TokenError::CapacityExceeded));
    assert!(second.authorized_tokens()?.is_empty());
    assert_eq!(store.stats()?, logical);
    assert_eq!(ledger.used(), charge(before.plaintext, ceiling.metadata));
    drop(blocker);
    assert_eq!(ledger.used(), before);
    Ok(())
}

#[test]
fn reserved_vector_push_never_grows_or_spends_unreserved_backing() -> Result<(), TokenError> {
    let ledger = StoreAllocationLedger::new(0, 4096)?;
    let mut values = StoreVec::with_capacity(1, ledger.clone(), 4)?;
    let before = ledger.used();
    values.push_without_growth(7u64)?;
    assert_eq!(ledger.used(), before);
    assert_eq!(
        values.push_without_growth(8).err(),
        Some(TokenError::CapacityExceeded)
    );
    assert_eq!(ledger.used(), before);
    assert_eq!(values.as_slice(), &[7]);
    Ok(())
}

#[test]
fn namespace_index_backing_is_precharged_retained_and_reclaimed() -> Result<(), TokenError> {
    let store = DlpTokenStore::new();
    let ledger = store.allocation_ledger_for_test()?;
    let baseline = ledger.used();
    let active = store.admit("codex", Some("synthetic-active"), 1)?;
    let slot = store.namespace_slot_bytes_for_test();
    assert_eq!(ledger.used(), charge(0, baseline.metadata + slot));
    for _ in 0..3 {
        drop(store.admit("codex", None, 1)?);
    }
    assert_eq!(ledger.used(), charge(0, baseline.metadata + 4 * slot));
    let future = std::time::Instant::now() + std::time::Duration::from_secs(3600);
    store.expire_idle_at(future)?;
    assert_eq!(store.stats()?.namespaces, 1);
    assert_eq!(ledger.used(), charge(0, baseline.metadata + 4 * slot));
    drop(active);
    store.expire_idle_at(future)?;
    assert_eq!(ledger.used(), baseline);
    assert_eq!(store.stats()?, Default::default());
    Ok(())
}

#[test]
fn namespace_admission_backing_denial_publishes_no_namespace_or_lease() -> Result<(), TokenError> {
    let store = DlpTokenStore::new();
    let ledger = store.allocation_ledger_for_test()?;
    let before = ledger.used();
    let limits = ledger.limits_for_test();
    let blocker = ledger.reserve(charge(0, limits.metadata - before.metadata))?;
    assert_eq!(
        store.admit("codex", None, 1).err(),
        Some(TokenError::CapacityExceeded)
    );
    assert_eq!(store.stats()?, Default::default());
    assert_eq!(store.admission_counts_for_test(), (0, 0, 0));
    assert_eq!(ledger.used(), charge(0, limits.metadata));
    drop(blocker);
    assert_eq!(ledger.used(), before);
    drop(store.admit("codex", None, 1)?);
    Ok(())
}

#[test]
fn store_control_backing_is_precharged_once_and_released_after_final_store_clone(
) -> Result<(), TokenError> {
    let store = DlpTokenStore::new();
    let ledger = store.allocation_ledger_for_test()?;
    let control = store.control_backing_bytes_for_test()?;
    let baseline = charge(0, StoreAllocationLedger::control_bytes()? + control);
    assert_eq!(ledger.used(), baseline);
    let second = store.clone();
    drop(store);
    assert_eq!(ledger.used(), baseline);
    drop(second);
    assert_eq!(
        ledger.used(),
        charge(0, StoreAllocationLedger::control_bytes()?)
    );
    Ok(())
}

#[test]
fn mapping_owner_allocation_copy_and_drop_boundaries_are_unlocked_and_release_on_failure(
) -> Result<(), TokenError> {
    use super::token_store_owners::test_support::{self, Phase};
    use std::{cell::Cell, rc::Rc};
    let mut observed = 0;
    for unwind in [false, true] {
        // A successful first pass measures the actual event count, then each
        // allocation/copy boundary is independently denied or interrupted.
        for stop in 0..=16 {
            let store = DlpTokenStore::new();
            let ledger = store.allocation_ledger_for_test()?;
            let mut lease = store.admit("codex", Some("synthetic-owner-sweep"), 1)?;
            let old = lease.prepare(&[synthetic_fragment("synthetic-old")], &[], |tokens| {
                Ok(tokens[0].clone())
            })?;
            let before = ledger.used();
            let logical = store.stats()?;
            let hits = Rc::new(Cell::new(0));
            let counted = hits.clone();
            let reentrant = store.clone();
            let guard = test_support::install(move |event| {
                // Same-thread reentry would deadlock if this boundary were locked.
                reentrant.stats()?;
                if event.phase == Phase::BeforeDrop {
                    return Ok(());
                }
                assert!(event.used.metadata >= event.bytes || event.used.plaintext >= event.bytes);
                let next = counted.get() + 1;
                counted.set(next);
                if next == stop {
                    if unwind {
                        panic!("synthetic persistent owner interruption");
                    }
                    return Err(TokenError::Unavailable);
                }
                Ok(())
            });
            let result = catch_unwind(AssertUnwindSafe(|| {
                lease.prepare(
                    &[
                        synthetic_fragment("synthetic-new-a"),
                        synthetic_fragment("synthetic-new-b"),
                    ],
                    &[],
                    |_| Ok(()),
                )
            }));
            drop(guard);
            if stop == 0 {
                assert!(matches!(result, Ok(Ok(()))));
                observed = hits.get();
                assert_eq!(observed, 16);
                assert_eq!(store.stats()?.entries, 3);
            } else {
                assert_eq!(hits.get(), stop);
                if unwind {
                    assert!(result.is_err());
                } else {
                    assert!(matches!(result, Ok(Err(TokenError::Unavailable))));
                }
                assert_eq!(ledger.used(), before);
                assert_eq!(store.stats()?, logical);
                assert_eq!(lease.authorized_tokens()?.len(), 1);
                assert!(lease.resolve(&old)? == "synthetic-old");
            }
        }
    }
    assert_eq!(observed, 16);
    Ok(())
}

#[test]
fn namespace_expiry_destroys_backing_unlocked_before_releasing_logical_charge(
) -> Result<(), TokenError> {
    use super::token_store_owners::test_support::{self, Kind, Phase};
    use std::{cell::Cell, rc::Rc};
    let store = DlpTokenStore::new();
    let ledger = store.allocation_ledger_for_test()?;
    let baseline = ledger.used();
    let mut lease = store.admit("codex", None, 1)?;
    lease.prepare(&[synthetic_fragment("fixture")], &[], |_| Ok(()))?;
    drop(lease);
    let logical = store.stats()?;
    let hits = Rc::new(Cell::new(0u8));
    let counted = hits.clone();
    let reentrant = store.clone();
    let guard = test_support::install(move |event| {
        let stats = reentrant.stats()?;
        if event.phase == Phase::BeforeDrop {
            assert_eq!(stats.namespaces, 0);
            assert_eq!(stats.entries, logical.entries);
            assert_eq!(stats.plaintext_bytes, logical.plaintext_bytes);
            assert_eq!(stats.metadata_bytes, logical.metadata_bytes);
            if event.kind == Kind::Mapping {
                assert_eq!(event.used.plaintext, 7);
                counted.set(counted.get() | 1);
            }
            if event.kind == Kind::Namespace {
                counted.set(counted.get() | 2);
            }
            if event.kind == Kind::Vector {
                counted.set(counted.get() | 4);
            }
        }
        Ok(())
    });
    store.expire_idle_at(std::time::Instant::now() + std::time::Duration::from_secs(3600))?;
    drop(guard);
    assert_eq!(hits.get(), 7);
    assert_eq!(store.stats()?, Default::default());
    assert_eq!(ledger.used(), baseline);
    Ok(())
}

#[test]
fn namespace_growth_failure_and_unwind_keep_old_index_and_active_lease() -> Result<(), TokenError> {
    use super::token_store_owners::test_support::{self, Phase};
    for unwind in [false, true] {
        for stop in 1..=2 {
            let store = DlpTokenStore::new();
            let ledger = store.allocation_ledger_for_test()?;
            let first = store.admit("codex", Some("synthetic-growth-active"), 1)?;
            let before = ledger.used();
            let logical = store.stats()?;
            let reentrant = store.clone();
            let mut hits = 0;
            let guard = test_support::install(move |event| {
                assert_eq!(reentrant.stats()?, logical);
                if event.phase == Phase::BeforeDrop {
                    return Ok(());
                }
                hits += 1;
                if hits == stop {
                    if unwind {
                        panic!("synthetic namespace growth interruption");
                    }
                    return Err(TokenError::Unavailable);
                }
                Ok(())
            });
            let result = catch_unwind(AssertUnwindSafe(|| store.admit("codex", None, 1)));
            drop(guard);
            if unwind {
                assert!(result.is_err());
            } else {
                assert!(matches!(result, Ok(Err(TokenError::Unavailable))));
            }
            assert_eq!(ledger.used(), before);
            assert_eq!(store.stats()?, logical);
            assert_eq!(store.admission_counts_for_test().1, 1);
            drop(first);
        }
    }
    Ok(())
}

#[test]
fn expiry_unwind_keeps_logical_charge_until_remaining_backing_drops() -> Result<(), TokenError> {
    use super::token_store_owners::test_support::{self, Kind, Phase};
    use std::{cell::Cell, rc::Rc};
    let store = DlpTokenStore::new();
    let ledger = store.allocation_ledger_for_test()?;
    let baseline = ledger.used();
    let mut lease = store.admit("codex", None, 1)?;
    lease.prepare(&[synthetic_fragment("fixture")], &[], |_| Ok(()))?;
    drop(lease);
    let logical = store.stats()?;
    let logical_while_backing_drops = Rc::new(Cell::new(0));
    let observed = logical_while_backing_drops.clone();
    let reentrant = store.clone();
    let mut interrupted = false;
    let guard = test_support::install(move |event| {
        if event.phase == Phase::BeforeDrop && event.kind == Kind::Mapping && !interrupted {
            interrupted = true;
            panic!("synthetic expiry payload interruption");
        }
        if event.phase == Phase::BeforeDrop && event.kind == Kind::Vector {
            observed.set(reentrant.stats()?.metadata_bytes);
        }
        Ok(())
    });
    let result = catch_unwind(AssertUnwindSafe(|| {
        store.expire_idle_at(std::time::Instant::now() + std::time::Duration::from_secs(3600))
    }));
    drop(guard);
    assert!(result.is_err());
    assert_eq!(logical_while_backing_drops.get(), logical.metadata_bytes);
    assert_eq!(ledger.used(), baseline);
    assert_eq!(store.stats()?, Default::default());
    Ok(())
}

#[test]
fn mapping_index_exact_peak_and_difference_one_preserve_old_authority() -> Result<(), TokenError> {
    use super::token_store_owners::test_support::{self, Phase};
    use std::{cell::Cell, rc::Rc};
    let slot = std::mem::size_of::<([u8; 10], SharedAllocation<()>)>();
    for count in [1usize, 4, 5, 8, 9, 4096] {
        for deficit in [1usize, 0] {
            let store = DlpTokenStore::new();
            let ledger = store.allocation_ledger_for_test()?;
            let mut lease = store.admit("codex", None, 1)?;
            let prior = if count > 1 {
                Some(
                    lease.prepare(&[synthetic_fragment("synthetic-prior")], &[], |tokens| {
                        Ok(tokens[0].clone())
                    })?,
                )
            } else {
                None
            };
            let new_count = count - usize::from(prior.is_some());
            let raw: Vec<_> = (0..new_count)
                .map(|i| format!("synthetic-peak-{i}"))
                .collect();
            let fragments: Vec<_> = raw.iter().map(|raw| synthetic_fragment(raw)).collect();
            let raw_bytes = raw.iter().map(String::len).sum::<usize>();
            let before = ledger.used();
            let logical = store.stats()?;
            // Postvalidation pointers, distinct shared Mapping allocations and
            // both complete replacement indices coexist with every old index.
            let peak = new_count * slot
                + new_count * store.mapping_backing_bytes_for_test()?
                + 2 * count * slot;
            let ceiling = ledger.limits_for_test();
            let blocker = ledger.reserve(charge(
                0,
                ceiling.metadata - before.metadata - peak + deficit,
            ))?;
            let measured = Rc::new(Cell::new(before.metadata));
            let observed = measured.clone();
            let guard = test_support::install(move |event| {
                if event.phase != Phase::BeforeDrop {
                    observed.set(observed.get().max(event.used.metadata));
                }
                Ok(())
            });
            let mut called = false;
            let result = lease.prepare(&fragments, &[], |_| {
                called = true;
                Ok(())
            });
            drop(guard);
            assert!(called);
            if deficit == 0 {
                result?;
                assert_eq!(measured.get(), ceiling.metadata);
                assert_eq!(ledger.used().plaintext, before.plaintext + raw_bytes);
                assert_eq!(store.stats()?.entries, count);
                assert_eq!(lease.authorized_tokens()?.len(), count);
            } else {
                assert_eq!(result.err(), Some(TokenError::CapacityExceeded));
                assert_eq!(store.stats()?, logical);
                assert_eq!(
                    ledger.used(),
                    charge(before.plaintext, ceiling.metadata - peak + deficit)
                );
                assert_eq!(
                    lease.authorized_tokens()?.len(),
                    usize::from(prior.is_some())
                );
            }
            if let Some(prior) = prior {
                assert!(lease.resolve(&prior)? == "synthetic-prior");
            }
            drop(blocker);
            if deficit != 0 {
                assert_eq!(ledger.used(), before);
            }
        }
    }
    Ok(())
}

#[test]
fn namespace_growth_exact_old_new_peak_and_difference_one_are_atomic() -> Result<(), TokenError> {
    use super::token_store_owners::test_support::{self, Phase};
    use std::{cell::Cell, rc::Rc};
    for count in [1usize, 4, 5, 8, 9, 256] {
        for deficit in [1usize, 0] {
            let store = DlpTokenStore::new();
            let ledger = store.allocation_ledger_for_test()?;
            let mut leases = Vec::new();
            for _ in 1..count {
                leases.push(store.admit("codex", None, 1)?);
            }
            let before = ledger.used();
            let logical = store.stats()?;
            let additional = count * store.namespace_slot_bytes_for_test();
            let limits = ledger.limits_for_test();
            let blocker = ledger.reserve(charge(
                0,
                limits.metadata - before.metadata - additional + deficit,
            ))?;
            let measured = Rc::new(Cell::new(before.metadata));
            let observed = measured.clone();
            let reentrant = store.clone();
            let guard = test_support::install(move |event| {
                reentrant.stats()?;
                if event.phase != Phase::BeforeDrop {
                    observed.set(observed.get().max(event.used.metadata));
                }
                Ok(())
            });
            let result = store.admit("codex", None, 1);
            drop(guard);
            if deficit == 0 {
                leases.push(result?);
                assert_eq!(measured.get(), limits.metadata);
                assert_eq!(store.stats()?.namespaces, count);
            } else {
                assert_eq!(result.err(), Some(TokenError::CapacityExceeded));
                assert_eq!(store.stats()?, logical);
                assert_eq!(
                    ledger.used(),
                    charge(0, limits.metadata - additional + deficit)
                );
            }
            drop(blocker);
            if deficit != 0 {
                assert_eq!(ledger.used(), before);
            }
        }
    }
    Ok(())
}

#[test]
fn owner_layout_counts_arc_alignment_and_vector_drop_keeps_backing_charge() -> Result<(), TokenError>
{
    #[repr(align(64))]
    struct Aligned([u8; 65]);
    let header = std::alloc::Layout::new::<[std::sync::atomic::AtomicUsize; 2]>();
    let exact = header
        .extend(std::alloc::Layout::new::<Aligned>())
        .map_err(|_| TokenError::Unavailable)?
        .0
        .pad_to_align()
        .size();
    assert_eq!(arc_backing_bytes::<Aligned>()?, exact);
    assert_eq!(exact, 192);
    let ledger = StoreAllocationLedger::new(0, 4096)?;
    let before = ledger.used();
    let width = std::mem::size_of::<DropProbe>();
    for panic in [false, true] {
        let mut values = StoreVec::with_capacity(1, ledger.clone(), 1)?;
        values.push_without_growth(DropProbe {
            ledger: ledger.clone(),
            expected: charge(0, before.metadata + width),
            panic,
        })?;
        assert_eq!(
            catch_unwind(AssertUnwindSafe(|| drop(values))).is_err(),
            panic
        );
        assert_eq!(ledger.used(), before);
    }
    Ok(())
}

#[test]
fn persistent_owners_release_every_conflicted_attempt_before_all_32_retries(
) -> Result<(), TokenError> {
    use super::payload_budget::PayloadBudget;
    for existing in [false, true] {
        let store = DlpTokenStore::new();
        let ledger = store.allocation_ledger_for_test()?;
        let mut first = store.admit("codex", Some("synthetic-owner-retries"), 1)?;
        let mut concurrent = store.admit("codex", Some("synthetic-owner-retries"), 1)?;
        let prior = if existing {
            Some(
                first.prepare(&[synthetic_fragment("synthetic-retained")], &[], |tokens| {
                    Ok(tokens[0].clone())
                })?,
            )
        } else {
            None
        };
        let baseline = ledger.used();
        let logical = store.stats()?;
        let budget = PayloadBudget::with_limit(64 * 1024 * 1024);
        let mut validations = 0;
        let result = first.prepare_with_budget(
            &[synthetic_fragment("synthetic-retried")],
            &[],
            budget.clone(),
            |_| {
                // Every preceding attempt's persistent copies and indices must
                // already have disappeared before the next candidate callback.
                assert_eq!(ledger.used(), baseline);
                validations += 1;
                concurrent.prepare(&[], &[], |_| Ok(()))
            },
        );
        assert_eq!(result.err(), Some(TokenError::Unavailable));
        assert_eq!(validations, 32);
        assert_eq!(ledger.used(), baseline);
        assert_eq!(store.stats()?, logical);
        assert_eq!(budget.used(), 0);
        assert_eq!(first.authorized_tokens()?.len(), usize::from(existing));
        if let Some(prior) = prior {
            assert!(first.resolve(&prior)? == "synthetic-retained");
        }
        first.prepare(&[synthetic_fragment("synthetic-retried")], &[], |_| Ok(()))?;
        assert_eq!(store.stats()?.entries, usize::from(existing) + 1);
    }
    Ok(())
}

#[test]
fn poisoned_store_keeps_persistent_owners_until_final_handle_destruction() -> Result<(), TokenError>
{
    let store = DlpTokenStore::new();
    let ledger = store.allocation_ledger_for_test()?;
    let mut lease = store.admit("codex", None, 1)?;
    lease.prepare(
        &[synthetic_fragment("synthetic-poison-retained")],
        &[],
        |_| Ok(()),
    )?;
    let owned = ledger.used();
    store.poison_for_test();
    assert_eq!(store.stats().err(), Some(TokenError::Unavailable));
    assert_eq!(ledger.used(), owned);
    drop(lease);
    assert_eq!(ledger.used().plaintext, owned.plaintext);
    assert!(ledger.used().metadata < owned.metadata);
    drop(store);
    assert_eq!(
        ledger.used(),
        charge(0, StoreAllocationLedger::control_bytes()?)
    );
    Ok(())
}

#[test]
fn concurrent_namespace_and_mapping_publication_release_all_backing_after_expiry(
) -> Result<(), TokenError> {
    use std::time::{Duration, Instant};
    let store = DlpTokenStore::new();
    let ledger = store.allocation_ledger_for_test()?;
    let baseline = ledger.used();
    let barrier = Arc::new(Barrier::new(9));
    let mut workers = Vec::new();
    for worker in 0..8 {
        let store = store.clone();
        let barrier = barrier.clone();
        workers.push(std::thread::spawn(move || -> Result<(), TokenError> {
            let session = format!("synthetic-owner-worker-{worker}");
            barrier.wait();
            let mut private = store.admit("codex", Some(&session), 1)?;
            let mut shared = store.admit("codex", Some("synthetic-owner-shared"), 1)?;
            for index in 0..8 {
                let raw = format!("synthetic-owner-value-{worker}-{index}");
                let token = private.prepare(&[synthetic_fragment(&raw)], &[], |tokens| {
                    Ok(tokens[0].clone())
                })?;
                assert!(private.resolve(&token)? == raw);
                let token = shared.prepare(&[synthetic_fragment(&raw)], &[], |tokens| {
                    Ok(tokens[0].clone())
                })?;
                assert!(shared.resolve(&token)? == raw);
            }
            Ok(())
        }));
    }
    barrier.wait();
    for worker in workers {
        assert!(matches!(worker.join(), Ok(Ok(()))));
    }
    assert_eq!(store.stats()?.entries, 128);
    assert_eq!(store.stats()?.namespaces, 9);
    store.expire_idle_at(Instant::now() + Duration::from_secs(1801))?;
    assert_eq!(store.stats()?, Default::default());
    assert_eq!(ledger.used(), baseline);
    drop(store);
    assert_eq!(
        ledger.used(),
        charge(0, StoreAllocationLedger::control_bytes()?)
    );
    Ok(())
}

#[test]
fn lease_authority_drop_unwind_releases_pin_only_after_backing_destruction(
) -> Result<(), TokenError> {
    use super::token_store_owners::test_support::{self, Kind, Phase};
    use std::{
        cell::Cell,
        rc::Rc,
        time::{Duration, Instant},
    };
    let store = DlpTokenStore::new();
    let ledger = store.allocation_ledger_for_test()?;
    let baseline = ledger.used();
    let mut lease = store.admit("codex", None, 1)?;
    lease.prepare(&[synthetic_fragment("synthetic-lease-drop")], &[], |_| {
        Ok(())
    })?;
    let logical = store.stats()?;
    let fired = Rc::new(Cell::new(false));
    let fired_hook = fired.clone();
    let reentrant = store.clone();
    let guard = test_support::install(move |event| {
        if event.kind == Kind::Vector
            && event.phase == Phase::BeforeDrop
            && !fired_hook.replace(true)
        {
            assert_eq!(reentrant.stats()?, logical);
            panic!("synthetic lease authority destruction interruption");
        }
        Ok(())
    });
    assert!(catch_unwind(AssertUnwindSafe(|| drop(lease))).is_err());
    drop(guard);
    assert!(fired.get());
    store.expire_idle_at(Instant::now() + Duration::from_secs(1801))?;
    assert_eq!(store.stats()?, Default::default());
    assert_eq!(ledger.used(), baseline);
    Ok(())
}

#[test]
fn completed_context_ownership_enforces_1024_and_16384_ceilings_and_releases_outside_locks(
) -> Result<(), TokenError> {
    use super::completed_context::{CompletedCallSpec, CompletionWitness};
    use std::time::{Duration, Instant};

    let store = DlpTokenStore::new();
    let ledger = store.allocation_ledger_for_test()?;
    let baseline = ledger.used();
    let witness = CompletionWitness::from_verified_completion(true)?;

    let lease = store.admit("codex", Some("session-ceilings"), 1)?;
    let attempt = lease.fork_attempt()?;
    let binding = attempt.bind_actual_attempt(
        "provider-primary",
        "responses",
        "https://api.example.invalid/v1/responses",
        Some("principal-1"),
    )?;

    // 1. 129 calls in a single completed context must fail CapacityExceeded.
    let too_many_calls: Vec<String> = (0..129).map(|i| format!("call_excess_{i}")).collect();
    let call_specs: Vec<CompletedCallSpec<'_>> = too_many_calls
        .iter()
        .map(|id| CompletedCallSpec {
            call_id: id,
            item_type: "function_call",
            name: "fn_x",
            payload: "{}",
            reasoning: None,
            encrypted_reasoning: None,
        })
        .collect();
    assert_eq!(
        attempt
            .publish_completed_context(&binding, &witness, "resp_129_calls", &[], &call_specs, &[])
            .err(),
        Some(TokenError::CapacityExceeded),
        "exceeding 128 call slots in a completed context must be rejected"
    );

    // 2. Per-namespace 1024 context+opaque ownership entries ceiling.
    // Publish one context with 1023 opaques (1 cursor + 1023 opaques = 1024 entries).
    let opaques_1023: Vec<String> = (0..1023).map(|i| format!("opaque-ns-{i}")).collect();
    let opaque_refs: Vec<&str> = opaques_1023.iter().map(String::as_str).collect();
    attempt.publish_completed_context(
        &binding,
        &witness,
        "resp_ns_1024",
        &[],
        &[],
        &opaque_refs,
    )?;

    // Adding 1 more cursor in the same namespace exceeds 1024 and must fail CapacityExceeded without leaking ledger bytes.
    let used_at_1024 = ledger.used();
    assert_eq!(
        attempt
            .publish_completed_context(&binding, &witness, "resp_ns_1025", &[], &[], &[])
            .err(),
        Some(TokenError::CapacityExceeded),
        "exceeding 1024 per-namespace completed-context/opaque entries must fail"
    );
    assert_eq!(ledger.used(), used_at_1024);

    // 3. Metadata ledger exhaustion difference-one check on resume and publish.
    let lease2 = store.admit("codex", Some("session-ceilings-2"), 1)?;
    let attempt2 = lease2.fork_attempt()?;
    let binding2 = attempt2.bind_actual_attempt(
        "provider-primary",
        "responses",
        "https://api.example.invalid/v1/responses",
        Some("principal-1"),
    )?;
    let before_pub = ledger.used();
    let blocker = ledger.reserve(charge(
        0,
        ledger.limits_for_test().metadata - before_pub.metadata,
    ))?;
    assert_eq!(
        attempt2
            .publish_completed_context(&binding2, &witness, "resp_blocked", &[], &[], &[])
            .err(),
        Some(TokenError::CapacityExceeded)
    );
    drop(blocker);
    assert_eq!(ledger.used(), before_pub);

    // 4. Expiry after dropping active leases releases all completed-context backing outside locks.
    drop(attempt);
    drop(lease);
    drop(attempt2);
    drop(lease2);
    store.expire_idle_at(Instant::now() + Duration::from_secs(1801))?;
    assert_eq!(store.stats()?, Default::default());
    assert_eq!(ledger.used(), baseline);
    Ok(())
}
