//! Synthetic metadata owner tests; no implicit sharing or uncharged growth.
use super::{
    payload_budget::PayloadBudget,
    payload_metadata::{
        test_support::{self, Action, Kind, Phase},
        PayloadKey, PayloadVec,
    },
    tokenization::TokenError,
};
use std::{cell::Cell, rc::Rc};

#[test]
fn metadata_vec_precharges_full_new_allocation_and_old_new_growth_peak() -> Result<(), TokenError> {
    let budget = PayloadBudget::with_limit(1024);
    let mut values = PayloadVec::<u64>::new(budget.clone())?;
    let hits = Rc::new(Cell::new(0));
    let observed = Rc::clone(&hits);
    let _hook = test_support::install(move |event| {
        assert!(event.kind == Kind::Vector);
        assert_eq!(event.used, if event.bytes == 32 { 32 } else { 96 });
        observed.set(observed.get() + 1);
        Action::Continue
    });
    for value in 0..5 {
        values.push(value)?;
    }
    assert_eq!(hits.get(), 4);
    assert_eq!(values.as_slice(), &[0, 1, 2, 3, 4]);
    assert_eq!(budget.used(), 64);
    assert_eq!(values.capacity(), 8);
    drop(values);
    assert_eq!(budget.used(), 0);
    Ok(())
}

#[test]
fn metadata_vec_exhaustion_preserves_old_values_and_capacity_before_allocation(
) -> Result<(), TokenError> {
    let budget = PayloadBudget::with_limit(95);
    let mut values = PayloadVec::<u64>::new(budget.clone())?;
    for value in 0..4 {
        values.push(value)?;
    }
    let _hook = test_support::install(|_| panic!("exhausted growth must not allocate"));
    assert_eq!(values.push(4), Err(TokenError::CapacityExceeded));
    assert_eq!(values.as_slice(), &[0, 1, 2, 3]);
    assert_eq!(budget.used(), 32);
    drop(values);
    assert_eq!(budget.used(), 0);
    Ok(())
}

#[test]
fn metadata_vec_error_and_unwind_preserve_old_owner_and_release_new_peak() -> Result<(), TokenError>
{
    for phase in [Phase::BeforeAllocation, Phase::AfterAllocation] {
        for unwind in [false, true] {
            let budget = PayloadBudget::with_limit(1024);
            let mut values = PayloadVec::<u64>::new(budget.clone())?;
            for value in 0..4 {
                values.push(value)?;
            }
            let _hook = test_support::install(move |event| {
                if event.phase == phase {
                    assert_eq!(event.used, 96);
                    if unwind {
                        panic!("synthetic metadata unwind");
                    }
                    return Action::Fail;
                }
                Action::Continue
            });
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| values.push(4)));
            if unwind {
                assert!(result.is_err());
            } else {
                assert_eq!(result.ok(), Some(Err(TokenError::Unavailable)));
            }
            assert_eq!(values.as_slice(), &[0, 1, 2, 3]);
            assert_eq!(budget.used(), 32);
            drop(values);
            assert_eq!(budget.used(), 0);
        }
    }
    Ok(())
}

#[test]
fn metadata_vec_pop_retains_capacity_and_drops_elements_before_releasing_charge(
) -> Result<(), TokenError> {
    struct Witness {
        budget: PayloadBudget,
        hits: Rc<Cell<usize>>,
    }
    impl Drop for Witness {
        fn drop(&mut self) {
            assert!(self.budget.used() > 0);
            self.hits.set(self.hits.get() + 1);
        }
    }
    let budget = PayloadBudget::with_limit(4096);
    let hits = Rc::new(Cell::new(0));
    let mut values = PayloadVec::new(budget.clone())?;
    for _ in 0..3 {
        values.push(Witness {
            budget: budget.clone(),
            hits: Rc::clone(&hits),
        })?;
    }
    let retained = budget.used();
    drop(values.pop());
    assert_eq!(budget.used(), retained);
    assert_eq!(hits.get(), 1);
    drop(values);
    assert_eq!(hits.get(), 3);
    assert_eq!(budget.used(), 0);
    Ok(())
}

#[test]
fn metadata_key_precharges_utf8_and_each_distinct_owner_separately() -> Result<(), TokenError> {
    let text = "synthetic-中🦀";
    let bytes = text.len();
    let budget = PayloadBudget::with_limit(2 * bytes);
    let hits = Rc::new(Cell::new(0));
    let observed = Rc::clone(&hits);
    let _hook = test_support::install(move |event| {
        assert!(event.kind == Kind::Key);
        assert_eq!(event.bytes, bytes);
        assert!(event.used == bytes || event.used == 2 * bytes);
        observed.set(observed.get() + 1);
        Action::Continue
    });
    let first = PayloadKey::try_from_str(text, &budget)?;
    let second = PayloadKey::try_from_str(text, &budget)?;
    assert_eq!(first.as_str(), text);
    assert_eq!(second.as_str(), text);
    assert_eq!(hits.get(), 4);
    assert_eq!(budget.used(), 2 * bytes);
    drop(first);
    assert_eq!(budget.used(), bytes);
    drop(second);
    assert_eq!(budget.used(), 0);
    Ok(())
}

#[test]
fn metadata_key_exhaustion_before_allocation_and_errors_unwind_release_charge() {
    let budget = PayloadBudget::with_limit(8);
    let _hook = test_support::install(|_| panic!("exhausted key must not allocate"));
    assert_eq!(
        PayloadKey::try_from_str("synthetic", &budget).err(),
        Some(TokenError::CapacityExceeded)
    );
    assert_eq!(budget.used(), 0);
    drop(_hook);
    for phase in [Phase::BeforeAllocation, Phase::AfterAllocation] {
        for unwind in [false, true] {
            let budget = PayloadBudget::with_limit(1024);
            let _hook = test_support::install(move |event| {
                if event.phase == phase {
                    assert_eq!(event.used, 9);
                    if unwind {
                        panic!("synthetic metadata key unwind");
                    }
                    return Action::Fail;
                }
                Action::Continue
            });
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                PayloadKey::try_from_str("synthetic", &budget)
            }));
            if unwind {
                assert!(result.is_err());
            } else {
                assert_eq!(
                    result.ok().and_then(Result::err),
                    Some(TokenError::Unavailable)
                );
            }
            assert_eq!(budget.used(), 0);
        }
    }
}

#[test]
fn metadata_zero_sized_elements_and_empty_keys_do_not_require_payload_bytes(
) -> Result<(), TokenError> {
    let budget = PayloadBudget::with_limit(0);
    let mut values = PayloadVec::new(budget.clone())?;
    values.push(())?;
    values.push(())?;
    assert_eq!(values.len(), 2);
    let empty = PayloadKey::try_from_str("", &budget)?;
    assert_eq!(empty.as_str(), "");
    assert_eq!(budget.used(), 0);
    Ok(())
}

#[test]
fn metadata_vec_in_place_sort_changes_order_without_allocation_or_charge() -> Result<(), TokenError>
{
    let budget = PayloadBudget::with_limit(128);
    let mut values = PayloadVec::<u64>::new(budget.clone())?;
    for value in [3, 1, 4, 1, 2] {
        values.push(value)?;
    }
    let charge = budget.used();
    let capacity = values.capacity();
    let _hook = test_support::install(|_| panic!("in-place sorting must not allocate"));
    values.sort_unstable_by(|left, right| left.cmp(right));
    assert_eq!(values.as_slice(), &[1, 1, 2, 3, 4]);
    assert_eq!(budget.used(), charge);
    assert_eq!(values.capacity(), capacity);
    drop(values);
    assert_eq!(budget.used(), 0);
    Ok(())
}

#[test]
fn metadata_vec_in_place_dedup_drops_elements_while_retaining_capacity_charge(
) -> Result<(), TokenError> {
    struct Item {
        key: u64,
        budget: PayloadBudget,
        drops: Rc<Cell<usize>>,
    }
    impl Drop for Item {
        fn drop(&mut self) {
            assert!(self.budget.used() > 0);
            self.drops.set(self.drops.get() + 1);
        }
    }
    let budget = PayloadBudget::with_limit(4096);
    let drops = Rc::new(Cell::new(0));
    let mut values = PayloadVec::new(budget.clone())?;
    for key in [1, 1, 2, 2, 3] {
        values.push(Item {
            key,
            budget: budget.clone(),
            drops: Rc::clone(&drops),
        })?;
    }
    let charge = budget.used();
    let capacity = values.capacity();
    let _hook = test_support::install(|_| panic!("in-place deduplication must not allocate"));
    values.dedup_by(|left, right| left.key == right.key);
    assert_eq!(values.len(), 3);
    assert_eq!(drops.get(), 2);
    assert_eq!(budget.used(), charge);
    assert_eq!(values.capacity(), capacity);
    assert!(values.as_slice().iter().map(|v| v.key).eq([1, 2, 3]));
    drop(values);
    assert_eq!(drops.get(), 5);
    assert_eq!(budget.used(), 0);
    Ok(())
}

#[test]
fn metadata_vec_mutation_is_bounded_to_existing_elements() -> Result<(), TokenError> {
    let budget = PayloadBudget::with_limit(32);
    let mut values = PayloadVec::<u64>::new(budget.clone())?;
    values.push(7)?;
    let _hook = test_support::install(|_| panic!("element access must not allocate"));
    *values.get_mut(0).ok_or(TokenError::Unavailable)? = 9;
    assert!(values.get_mut(1).is_none());
    assert!(values.get_mut(usize::MAX).is_none());
    assert_eq!(values.as_slice(), &[9]);
    assert_eq!(budget.used(), 32);
    drop(values);
    assert_eq!(budget.used(), 0);
    Ok(())
}
