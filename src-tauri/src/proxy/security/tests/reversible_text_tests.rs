use super::{
    payload_budget::PayloadBudget,
    payload_text::{
        test_support::{self, Action, Phase},
        PayloadText,
    },
    tokenization::TokenError,
};
use std::{cell::Cell, rc::Rc};

#[test]
fn text_owner_precharges_before_allocation_and_retains_fixed_capacity() -> Result<(), TokenError> {
    let budget = PayloadBudget::with_limit(12);
    let hits = Rc::new(Cell::new(0));
    let seen = Rc::clone(&hits);
    let _hook = test_support::install(move |event| {
        assert_eq!(event.used, 12);
        if event.phase != Phase::BeforeCopy {
            assert_eq!(event.bytes, 12);
        }
        seen.set(seen.get() + 1);
        Action::Continue
    });
    let mut owner = PayloadText::with_capacity(12, budget.clone(), 12)?;
    assert_eq!(owner.capacity(), 12);
    assert!(owner.as_str().is_empty());
    owner.append("中")?;
    owner.append("🦀")?;
    owner.append("five!")?;
    assert!(owner.as_str() == "中🦀five!");
    assert_eq!(owner.append("x").err(), Some(TokenError::CapacityExceeded));
    assert!(owner.as_str() == "中🦀five!");
    assert_eq!(owner.capacity(), 12);
    assert_eq!(budget.used(), 12);
    assert_eq!(hits.get(), 5);
    drop(owner);
    assert_eq!(budget.used(), 0);
    Ok(())
}

#[test]
fn text_owner_shared_local_and_isize_rejections_precede_allocation() {
    for (capacity, shared, local) in [(7, 6, 7), (7, 7, 6), (usize::MAX, usize::MAX, usize::MAX)] {
        let budget = PayloadBudget::with_limit(shared);
        let hits = Rc::new(Cell::new(0));
        let seen = Rc::clone(&hits);
        let _hook = test_support::install(move |_| {
            seen.set(seen.get() + 1);
            Action::Continue
        });
        assert_eq!(
            PayloadText::with_capacity(capacity, budget.clone(), local).err(),
            Some(TokenError::CapacityExceeded)
        );
        assert_eq!(hits.get(), 0);
        assert_eq!(budget.used(), 0);
    }
}

#[test]
fn text_owner_distinct_copies_charge_logical_utf8_not_source_capacity() -> Result<(), TokenError> {
    let mut source = String::with_capacity(4096);
    source.push_str("中🦀");
    let budget = PayloadBudget::with_limit(14);
    let first = PayloadText::try_from_str(&source, budget.clone(), 7)?;
    let second = PayloadText::try_from_str(first.as_str(), budget.clone(), 7)?;
    assert_eq!(first.capacity(), 7);
    assert_eq!(second.capacity(), 7);
    assert_eq!(budget.used(), 14);
    drop(first);
    assert_eq!(budget.used(), 7);
    assert!(second.as_str() == source);
    drop(second);
    assert_eq!(budget.used(), 0);
    Ok(())
}

#[test]
fn text_owner_empty_is_free_and_cannot_grow() -> Result<(), TokenError> {
    let budget = PayloadBudget::with_limit(0);
    let mut owner = PayloadText::try_from_str("", budget.clone(), 0)?;
    owner.append("")?;
    assert_eq!(owner.capacity(), 0);
    assert_eq!(owner.append("x").err(), Some(TokenError::CapacityExceeded));
    assert!(owner.as_str().is_empty());
    drop(owner);
    assert_eq!(budget.used(), 0);
    Ok(())
}

#[test]
fn text_owner_all_injected_errors_release_charge_and_copy_is_atomic() -> Result<(), TokenError> {
    for phase in [
        Phase::BeforeAllocation,
        Phase::AfterAllocation,
        Phase::BeforeCopy,
    ] {
        let budget = PayloadBudget::with_limit(9);
        let hits = Rc::new(Cell::new(0));
        let seen = Rc::clone(&hits);
        let hook = test_support::install(move |event| {
            if event.phase == phase {
                seen.set(seen.get() + 1);
                Action::Fail
            } else {
                Action::Continue
            }
        });
        assert_eq!(
            PayloadText::try_from_str("synthetic", budget.clone(), 9).err(),
            Some(TokenError::Unavailable)
        );
        assert_eq!(hits.get(), 1);
        assert_eq!(budget.used(), 0);
        drop(hook);
    }
    let budget = PayloadBudget::with_limit(9);
    let mut owner = PayloadText::with_capacity(9, budget.clone(), 9)?;
    owner.append("syn")?;
    let _hook = test_support::install(|event| {
        if event.phase == Phase::BeforeCopy {
            Action::Fail
        } else {
            Action::Continue
        }
    });
    assert_eq!(owner.append("thetic").err(), Some(TokenError::Unavailable));
    assert!(owner.as_str() == "syn");
    assert_eq!(budget.used(), 9);
    drop(owner);
    assert_eq!(budget.used(), 0);
    Ok(())
}

#[test]
fn text_owner_unwind_in_each_phase_releases_all_reservations() {
    for phase in [
        Phase::BeforeAllocation,
        Phase::AfterAllocation,
        Phase::BeforeCopy,
    ] {
        let budget = PayloadBudget::with_limit(9);
        let reached = Rc::new(Cell::new(false));
        let seen = Rc::clone(&reached);
        let _hook = test_support::install(move |event| {
            if event.phase == phase {
                seen.set(true);
                panic!("synthetic text owner unwind");
            }
            Action::Continue
        });
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            PayloadText::try_from_str("synthetic", budget.clone(), 9)
        }));
        assert!(reached.get());
        assert!(result.is_err());
        assert_eq!(budget.used(), 0);
    }
}
