//! Synthetic private-owner tests; these do not establish response-path integration.
#![cfg(test)]

use super::{
    payload_budget::PayloadBudget,
    payload_buffer::{
        test_support::{self, Action, Observation, Phase},
        PayloadBuffer,
    },
    tokenization::TokenError,
};
use std::{
    cell::RefCell,
    future::{pending, Future},
    panic::{catch_unwind, AssertUnwindSafe},
    rc::Rc,
    sync::Arc,
    task::{Context, Poll, Wake, Waker},
    thread,
};

fn observe() -> (Rc<RefCell<Vec<Observation>>>, test_support::HookGuard) {
    let observations = Rc::new(RefCell::new(Vec::new()));
    let captured = Rc::clone(&observations);
    let guard = test_support::install(move |event| {
        captured.borrow_mut().push(event);
        Action::Continue
    });
    (observations, guard)
}

fn check_bytes(buffer: &PayloadBuffer, expected: &[u8]) {
    // Keep payload bytes out of assertion reports, even for synthetic fixtures.
    assert!(buffer.as_slice() == expected, "synthetic bytes differ");
    assert_eq!(buffer.len(), expected.len());
}

#[test]
fn empty_owner_needs_no_allocation_or_charge_even_at_zero_limit() -> Result<(), TokenError> {
    let budget = PayloadBudget::with_limit(0);
    let (events, _hook) = observe();
    let mut owner = PayloadBuffer::new(budget.clone(), 0)?;
    check_bytes(&owner, b"");
    assert_eq!(owner.capacity(), 0);
    owner.append(b"")?;
    let copy = owner.try_clone()?;
    assert_eq!(copy.capacity(), 0);
    assert!(events.borrow().is_empty());
    assert_eq!(budget.used(), 0);
    drop(owner);
    drop(copy);
    assert_eq!(budget.used(), 0);
    Ok(())
}

#[test]
fn borrowed_copy_is_charged_before_allocation_and_before_copy() -> Result<(), TokenError> {
    let budget = PayloadBudget::with_limit(4);
    let (events, _hook) = observe();
    let mut source = *b"abcd";
    let owner = PayloadBuffer::try_from_slice(&source, budget.clone(), 4)?;
    source.fill(b'x');
    check_bytes(&owner, b"abcd");
    assert_eq!(owner.capacity(), 4);
    assert_eq!(budget.used(), 4);
    assert_eq!(
        *events.borrow(),
        [
            Observation {
                phase: Phase::BeforeAllocation,
                requested: 4,
                capacity: 0,
                bytes_to_copy: 0,
                used: 4,
            },
            Observation {
                phase: Phase::CapacityObserved,
                requested: 4,
                capacity: 4,
                bytes_to_copy: 0,
                used: 4,
            },
            Observation {
                phase: Phase::BeforeCopy,
                requested: 4,
                capacity: 4,
                bytes_to_copy: 4,
                used: 4,
            },
        ]
    );
    drop(owner);
    assert_eq!(budget.used(), 0);
    Ok(())
}

#[test]
fn append_to_empty_charges_the_exact_capacity_before_allocation() -> Result<(), TokenError> {
    let budget = PayloadBudget::with_limit(5);
    let mut owner = PayloadBuffer::new(budget.clone(), 5)?;
    let (events, _hook) = observe();
    owner.append(b"abcde")?;
    check_bytes(&owner, b"abcde");
    assert_eq!(owner.capacity(), 5);
    let events = events.borrow();
    assert_eq!(events.len(), 3);
    assert_eq!(events[0].phase, Phase::BeforeAllocation);
    assert_eq!(events[0].requested, 5);
    assert!(events.iter().all(|event| event.used == 5));
    assert_eq!(events[2].phase, Phase::BeforeCopy);
    assert_eq!(events[2].bytes_to_copy, 5);
    Ok(())
}

#[test]
fn replacement_growth_charges_old_plus_entire_new_capacity_until_swap() -> Result<(), TokenError> {
    let budget = PayloadBudget::with_limit(10);
    let mut owner = PayloadBuffer::try_from_slice(b"abcd", budget.clone(), 6)?;
    let (events, _hook) = observe();
    owner.append(b"ef")?;
    check_bytes(&owner, b"abcdef");
    assert_eq!(owner.capacity(), 6);
    assert_eq!(budget.used(), 6);
    let events = events.borrow();
    assert_eq!(events.len(), 3);
    assert_eq!(events[0].phase, Phase::BeforeAllocation);
    assert_eq!(events[0].requested, 6);
    assert_eq!(events[2].phase, Phase::BeforeCopy);
    assert_eq!(events[2].bytes_to_copy, 6);
    assert!(events.iter().all(|event| event.used == 10));
    drop(owner);
    assert_eq!(budget.used(), 0);
    Ok(())
}

#[test]
fn replacement_peak_one_byte_over_fails_atomically_then_can_retry() -> Result<(), TokenError> {
    let budget = PayloadBudget::with_limit(10);
    let mut owner = PayloadBuffer::try_from_slice(b"abcd", budget.clone(), 6)?;
    let competing = budget.reserve(1)?;
    let pointer = owner.as_slice().as_ptr();
    let (events, _hook) = observe();
    assert_eq!(owner.append(b"ef"), Err(TokenError::CapacityExceeded));
    check_bytes(&owner, b"abcd");
    assert_eq!(owner.as_slice().as_ptr(), pointer);
    assert_eq!(owner.capacity(), 4);
    assert_eq!(budget.used(), 5);
    assert!(events.borrow().is_empty());
    drop(competing);
    owner.append(b"ef")?;
    check_bytes(&owner, b"abcdef");
    assert_eq!(budget.used(), 6);
    Ok(())
}

#[test]
fn local_exact_ceiling_succeeds_and_one_byte_over_never_allocates() -> Result<(), TokenError> {
    let budget = PayloadBudget::with_limit(30);
    let mut owner = PayloadBuffer::try_from_slice(b"abcd", budget.clone(), 4)?;
    let (events, _hook) = observe();
    assert_eq!(owner.append(b"e"), Err(TokenError::CapacityExceeded));
    assert!(matches!(
        PayloadBuffer::try_from_slice(b"abcde", budget.clone(), 4),
        Err(TokenError::CapacityExceeded)
    ));
    check_bytes(&owner, b"abcd");
    assert_eq!(owner.capacity(), 4);
    assert_eq!(budget.used(), 4);
    assert!(events.borrow().is_empty());
    Ok(())
}

#[test]
fn global_exact_ceiling_succeeds_and_initial_overage_never_allocates() -> Result<(), TokenError> {
    let budget = PayloadBudget::with_limit(4);
    let owner = PayloadBuffer::try_from_slice(b"abcd", budget.clone(), 8)?;
    let (events, _hook) = observe();
    assert!(matches!(
        PayloadBuffer::try_from_slice(b"e", budget.clone(), 8),
        Err(TokenError::CapacityExceeded)
    ));
    assert_eq!(budget.used(), 4);
    assert!(events.borrow().is_empty());
    drop(owner);
    assert_eq!(budget.used(), 0);
    assert!(matches!(
        PayloadBuffer::try_from_slice(b"abcde", budget.clone(), 8),
        Err(TokenError::CapacityExceeded)
    ));
    assert!(events.borrow().is_empty());
    assert_eq!(budget.used(), 0);
    Ok(())
}

#[test]
fn required_size_checks_usize_overflow_local_limit_and_vec_bound_without_allocating() {
    assert_eq!(test_support::checked_required(2, 2, 4), Ok(4));
    assert_eq!(
        test_support::checked_required(2, 3, 4),
        Err(TokenError::CapacityExceeded)
    );
    for (current, additional) in [(usize::MAX, 1), (usize::MAX - 1, 2)] {
        assert_eq!(
            test_support::checked_required(current, additional, usize::MAX),
            Err(TokenError::CapacityExceeded)
        );
    }
    assert_eq!(
        test_support::checked_required(0, isize::MAX as usize, usize::MAX),
        Ok(isize::MAX as usize)
    );
    assert_eq!(
        test_support::checked_required(0, isize::MAX as usize + 1, usize::MAX),
        Err(TokenError::CapacityExceeded)
    );
}

#[test]
fn global_sum_overflow_rejects_growth_before_any_allocation() -> Result<(), TokenError> {
    let budget = PayloadBudget::with_limit(usize::MAX);
    let mut owner = PayloadBuffer::try_from_slice(b"a", budget.clone(), usize::MAX)?;
    let competing = budget.reserve(usize::MAX - 1)?;
    let (events, _hook) = observe();
    assert_eq!(owner.append(b"b"), Err(TokenError::CapacityExceeded));
    assert!(events.borrow().is_empty());
    check_bytes(&owner, b"a");
    assert_eq!(owner.capacity(), 1);
    assert_eq!(budget.used(), usize::MAX);
    drop(competing);
    assert_eq!(budget.used(), 1);
    drop(owner);
    assert_eq!(budget.used(), 0);
    Ok(())
}

#[test]
fn clear_and_truncate_keep_full_capacity_charged_until_drop() -> Result<(), TokenError> {
    let budget = PayloadBudget::with_limit(6);
    let mut owner = PayloadBuffer::try_from_slice(b"abcdef", budget.clone(), 6)?;
    owner.truncate(2);
    check_bytes(&owner, b"ab");
    assert_eq!(owner.capacity(), 6);
    assert_eq!(budget.used(), 6);
    owner.truncate(usize::MAX);
    check_bytes(&owner, b"ab");
    owner.clear();
    check_bytes(&owner, b"");
    assert_eq!(owner.capacity(), 6);
    assert_eq!(budget.used(), 6);
    assert!(matches!(
        budget.reserve(1),
        Err(TokenError::CapacityExceeded)
    ));
    drop(owner);
    assert_eq!(budget.used(), 0);
    Ok(())
}

#[test]
fn append_within_retained_capacity_copies_without_another_allocation_or_charge(
) -> Result<(), TokenError> {
    let budget = PayloadBudget::with_limit(6);
    let mut owner = PayloadBuffer::try_from_slice(b"abcdef", budget.clone(), 6)?;
    owner.truncate(2);
    let pointer = owner.as_slice().as_ptr();
    let (events, _hook) = observe();
    owner.append(b"XY")?;
    check_bytes(&owner, b"abXY");
    assert_eq!(owner.capacity(), 6);
    assert_eq!(owner.as_slice().as_ptr(), pointer);
    assert_eq!(budget.used(), 6);
    assert_eq!(events.borrow().len(), 1);
    assert_eq!(events.borrow()[0].phase, Phase::BeforeCopy);
    assert_eq!(events.borrow()[0].bytes_to_copy, 2);
    assert_eq!(events.borrow()[0].used, 6);
    owner.clear();
    owner.append(b"123456")?;
    check_bytes(&owner, b"123456");
    assert_eq!(owner.as_slice().as_ptr(), pointer);
    assert_eq!(budget.used(), 6);
    Ok(())
}

#[test]
fn empty_append_is_a_noop_with_a_full_budget() -> Result<(), TokenError> {
    let budget = PayloadBudget::with_limit(4);
    let mut owner = PayloadBuffer::try_from_slice(b"abcd", budget.clone(), 4)?;
    let (events, _hook) = observe();
    owner.append(b"")?;
    check_bytes(&owner, b"abcd");
    assert!(events.borrow().is_empty());
    assert_eq!(budget.used(), 4);
    Ok(())
}

#[test]
fn explicit_copy_charges_only_logical_bytes_and_is_distinct() -> Result<(), TokenError> {
    let budget = PayloadBudget::with_limit(8);
    let mut owner = PayloadBuffer::try_from_slice(b"abcdef", budget.clone(), 6)?;
    owner.truncate(2);
    let (events, _hook) = observe();
    let mut copy = owner.try_clone()?;
    check_bytes(&copy, b"ab");
    assert_ne!(copy.as_slice().as_ptr(), owner.as_slice().as_ptr());
    assert_eq!(copy.capacity(), 2);
    assert_eq!(owner.capacity(), 6);
    assert_eq!(budget.used(), 8);
    assert!(events.borrow().iter().all(|event| event.used == 8));
    assert_eq!(events.borrow()[0].requested, 2);
    copy.clear();
    check_bytes(&owner, b"ab");
    assert_eq!(budget.used(), 8);
    drop(owner);
    assert_eq!(budget.used(), 2);
    drop(copy);
    assert_eq!(budget.used(), 0);
    Ok(())
}

#[test]
fn explicit_copy_preserves_the_local_ceiling() -> Result<(), TokenError> {
    let budget = PayloadBudget::with_limit(30);
    let owner = PayloadBuffer::try_from_slice(b"abcd", budget.clone(), 4)?;
    let mut copy = owner.try_clone()?;
    assert_eq!(copy.append(b"e"), Err(TokenError::CapacityExceeded));
    check_bytes(&copy, b"abcd");
    assert_eq!(budget.used(), 8);
    Ok(())
}

#[test]
fn copy_denial_preserves_original_bytes_pointer_and_charge() -> Result<(), TokenError> {
    let budget = PayloadBudget::with_limit(7);
    let owner = PayloadBuffer::try_from_slice(b"abcd", budget.clone(), 4)?;
    let pointer = owner.as_slice().as_ptr();
    let (events, _hook) = observe();
    assert!(matches!(
        owner.try_clone(),
        Err(TokenError::CapacityExceeded)
    ));
    check_bytes(&owner, b"abcd");
    assert_eq!(owner.as_slice().as_ptr(), pointer);
    assert_eq!(owner.capacity(), 4);
    assert_eq!(budget.used(), 4);
    assert!(events.borrow().is_empty());
    Ok(())
}

#[test]
fn empty_copy_does_not_duplicate_the_sources_retained_capacity() -> Result<(), TokenError> {
    let budget = PayloadBudget::with_limit(4);
    let mut owner = PayloadBuffer::try_from_slice(b"abcd", budget.clone(), 4)?;
    owner.clear();
    let (events, _hook) = observe();
    let copy = owner.try_clone()?;
    check_bytes(&copy, b"");
    assert_eq!(copy.capacity(), 0);
    assert_eq!(budget.used(), 4);
    assert!(events.borrow().is_empty());
    drop(owner);
    assert_eq!(budget.used(), 0);
    drop(copy);
    assert_eq!(budget.used(), 0);
    Ok(())
}

#[test]
fn immutable_arc_sharing_releases_the_charge_only_on_last_drop() -> Result<(), TokenError> {
    let budget = PayloadBudget::with_limit(4);
    let owner = Arc::new(PayloadBuffer::try_from_slice(b"abcd", budget.clone(), 4)?);
    let shared = Arc::clone(&owner);
    assert_eq!(budget.used(), 4);
    drop(owner);
    assert_eq!(budget.used(), 4);
    let reader = thread::spawn(move || {
        check_bytes(&shared, b"abcd");
        shared
    });
    let last = reader.join().map_err(|_| TokenError::Unavailable)?;
    assert_eq!(budget.used(), 4);
    drop(last);
    assert_eq!(budget.used(), 0);
    Ok(())
}

struct NoopWake;
impl Wake for NoopWake {
    fn wake(self: Arc<Self>) {}
}

#[test]
fn cancelling_a_polled_future_releases_the_buffer_and_charge() -> Result<(), TokenError> {
    let budget = PayloadBudget::with_limit(4);
    let owner = PayloadBuffer::try_from_slice(b"abcd", budget.clone(), 4)?;
    let mut operation = Box::pin(async move {
        let _owner = owner;
        pending::<()>().await;
    });
    let waker = Waker::from(Arc::new(NoopWake));
    assert!(matches!(
        operation.as_mut().poll(&mut Context::from_waker(&waker)),
        Poll::Pending
    ));
    assert_eq!(budget.used(), 4);
    drop(operation);
    assert_eq!(budget.used(), 0);
    Ok(())
}

#[test]
fn cancelling_an_unpolled_future_releases_the_buffer_and_charge() -> Result<(), TokenError> {
    let budget = PayloadBudget::with_limit(4);
    let owner = PayloadBuffer::try_from_slice(b"abcd", budget.clone(), 4)?;
    let operation = async move {
        let _owner = owner;
        pending::<()>().await;
    };
    assert_eq!(budget.used(), 4);
    drop(operation);
    assert_eq!(budget.used(), 0);
    Ok(())
}

#[test]
fn error_return_and_unwind_release_owned_capacity() -> Result<(), TokenError> {
    fn fail(budget: &PayloadBudget) -> Result<(), TokenError> {
        let _owner = PayloadBuffer::try_from_slice(b"abcd", budget.clone(), 4)?;
        Err(TokenError::Unavailable)
    }
    let budget = PayloadBudget::with_limit(4);
    assert_eq!(fail(&budget), Err(TokenError::Unavailable));
    assert_eq!(budget.used(), 0);
    let owner = PayloadBuffer::try_from_slice(b"abcd", budget.clone(), 4)?;
    let result = catch_unwind(AssertUnwindSafe(move || {
        let _owner = owner;
        panic!("synthetic owner unwind");
    }));
    assert!(result.is_err());
    assert_eq!(budget.used(), 0);
    Ok(())
}

#[test]
fn injected_initial_allocation_failure_rolls_back_its_precharge() -> Result<(), TokenError> {
    let budget = PayloadBudget::with_limit(4);
    let calls = Rc::new(RefCell::new(0));
    let captured = Rc::clone(&calls);
    let _hook = test_support::install(move |event| {
        assert_eq!(event.phase, Phase::BeforeAllocation);
        assert_eq!(event.used, 4);
        *captured.borrow_mut() += 1;
        Action::FailAllocation
    });
    assert!(matches!(
        PayloadBuffer::try_from_slice(b"abcd", budget.clone(), 4),
        Err(TokenError::Unavailable)
    ));
    assert_eq!(*calls.borrow(), 1);
    assert_eq!(budget.used(), 0);
    Ok(())
}

#[test]
fn injected_replacement_allocation_failure_preserves_original_and_releases_tentative_charge(
) -> Result<(), TokenError> {
    let budget = PayloadBudget::with_limit(10);
    let mut owner = PayloadBuffer::try_from_slice(b"abcd", budget.clone(), 6)?;
    let pointer = owner.as_slice().as_ptr();
    {
        let _hook = test_support::install(|event| {
            assert_eq!(event.phase, Phase::BeforeAllocation);
            assert_eq!(event.requested, 6);
            assert_eq!(event.used, 10);
            Action::FailAllocation
        });
        assert_eq!(owner.append(b"ef"), Err(TokenError::Unavailable));
    }
    check_bytes(&owner, b"abcd");
    assert_eq!(owner.as_slice().as_ptr(), pointer);
    assert_eq!(owner.capacity(), 4);
    assert_eq!(budget.used(), 4);
    owner.append(b"ef")?;
    check_bytes(&owner, b"abcdef");
    assert_eq!(budget.used(), 6);
    Ok(())
}

#[test]
fn injected_copy_allocation_failure_preserves_source_and_charge() -> Result<(), TokenError> {
    let budget = PayloadBudget::with_limit(8);
    let owner = PayloadBuffer::try_from_slice(b"abcd", budget.clone(), 4)?;
    let _hook = test_support::install(|event| {
        assert_eq!(event.phase, Phase::BeforeAllocation);
        assert_eq!(event.used, 8);
        Action::FailAllocation
    });
    assert!(matches!(owner.try_clone(), Err(TokenError::Unavailable)));
    check_bytes(&owner, b"abcd");
    assert_eq!(owner.capacity(), 4);
    assert_eq!(budget.used(), 4);
    Ok(())
}

#[test]
fn unexpected_initial_capacity_is_rejected_before_copy_and_releases_charge(
) -> Result<(), TokenError> {
    let budget = PayloadBudget::with_limit(4);
    let calls = Rc::new(RefCell::new(0));
    let captured = Rc::clone(&calls);
    let _hook = test_support::install(move |event| {
        *captured.borrow_mut() += 1;
        assert_eq!(event.used, 4);
        match event.phase {
            Phase::BeforeAllocation => Action::Continue,
            Phase::CapacityObserved => Action::ReportCapacity(5),
            Phase::BeforeCopy => panic!("copy preceded capacity validation"),
        }
    });
    assert!(matches!(
        PayloadBuffer::try_from_slice(b"abcd", budget.clone(), 4),
        Err(TokenError::CapacityExceeded)
    ));
    assert_eq!(*calls.borrow(), 2);
    assert_eq!(budget.used(), 0);
    Ok(())
}

#[test]
fn unexpected_replacement_capacity_is_rejected_without_copy_or_owner_change(
) -> Result<(), TokenError> {
    let budget = PayloadBudget::with_limit(10);
    let mut owner = PayloadBuffer::try_from_slice(b"abcd", budget.clone(), 6)?;
    let pointer = owner.as_slice().as_ptr();
    let _hook = test_support::install(|event| {
        assert_eq!(event.used, 10);
        match event.phase {
            Phase::BeforeAllocation => Action::Continue,
            Phase::CapacityObserved => Action::ReportCapacity(7),
            Phase::BeforeCopy => panic!("copy preceded capacity validation"),
        }
    });
    assert_eq!(owner.append(b"ef"), Err(TokenError::CapacityExceeded));
    check_bytes(&owner, b"abcd");
    assert_eq!(owner.as_slice().as_ptr(), pointer);
    assert_eq!(owner.capacity(), 4);
    assert_eq!(budget.used(), 4);
    Ok(())
}

#[test]
fn unwind_between_allocation_and_copy_rolls_back_replacement_charge() -> Result<(), TokenError> {
    let budget = PayloadBudget::with_limit(10);
    let mut owner = PayloadBuffer::try_from_slice(b"abcd", budget.clone(), 6)?;
    let pointer = owner.as_slice().as_ptr();
    {
        let _hook = test_support::install(|event| {
            assert_eq!(event.used, 10);
            if event.phase == Phase::BeforeCopy {
                panic!("synthetic tentative allocation unwind");
            }
            Action::Continue
        });
        let result = catch_unwind(AssertUnwindSafe(|| owner.append(b"ef")));
        assert!(result.is_err());
    }
    check_bytes(&owner, b"abcd");
    assert_eq!(owner.as_slice().as_ptr(), pointer);
    assert_eq!(owner.capacity(), 4);
    assert_eq!(budget.used(), 4);
    owner.append(b"ef")?;
    assert_eq!(budget.used(), 6);
    Ok(())
}

#[test]
fn allocation_fault_hooks_are_thread_local() -> Result<(), TokenError> {
    let budget = PayloadBudget::with_limit(8);
    let _hook = test_support::install(|_| Action::FailAllocation);
    let other_budget = budget.clone();
    let thread = thread::spawn(move || PayloadBuffer::try_from_slice(b"abcd", other_budget, 4));
    let owner = thread.join().map_err(|_| TokenError::Unavailable)??;
    assert_eq!(budget.used(), 4);
    assert!(matches!(
        PayloadBuffer::try_from_slice(b"abcd", budget.clone(), 4),
        Err(TokenError::Unavailable)
    ));
    assert_eq!(budget.used(), 4);
    drop(owner);
    assert_eq!(budget.used(), 0);
    Ok(())
}

#[test]
fn preallocated_empty_owner_charges_before_allocation_without_copying() -> Result<(), TokenError> {
    let budget = PayloadBudget::with_limit(8);
    let (events, _hook) = observe();
    let mut owner = PayloadBuffer::with_capacity(8, budget.clone(), 8)?;
    check_bytes(&owner, b"");
    assert_eq!(owner.capacity(), 8);
    assert_eq!(budget.used(), 8);
    assert_eq!(events.borrow().len(), 2);
    assert_eq!(events.borrow()[0].phase, Phase::BeforeAllocation);
    assert!(events.borrow().iter().all(|event| event.used == 8));
    owner.append(b"abcd")?;
    owner.append(b"efgh")?;
    check_bytes(&owner, b"abcdefgh");
    assert_eq!(events.borrow().len(), 4);
    assert_eq!(budget.used(), 8);
    drop(owner);
    assert_eq!(budget.used(), 0);
    Ok(())
}

#[test]
fn preallocation_rejects_local_and_global_exhaustion_before_allocation() {
    for (capacity, limit, local_limit) in [(9, 9, 8), (9, 8, 9), (usize::MAX, 8, usize::MAX)] {
        let budget = PayloadBudget::with_limit(limit);
        let (events, _hook) = observe();
        assert!(matches!(
            PayloadBuffer::with_capacity(capacity, budget.clone(), local_limit),
            Err(TokenError::CapacityExceeded)
        ));
        assert!(events.borrow().is_empty());
        assert_eq!(budget.used(), 0);
    }
}

#[test]
fn preallocation_failure_and_unexpected_capacity_release_the_charge() {
    for action in [0, 1] {
        let budget = PayloadBudget::with_limit(8);
        let calls = Rc::new(std::cell::Cell::new(0));
        let observed = Rc::clone(&calls);
        let _hook = test_support::install(move |event| {
            observed.set(observed.get() + 1);
            assert_eq!(event.used, 8);
            match (action, event.phase) {
                (0, Phase::BeforeAllocation) => Action::FailAllocation,
                (1, Phase::CapacityObserved) => Action::ReportCapacity(9),
                (_, Phase::BeforeCopy) => panic!("preallocation copied payload"),
                _ => Action::Continue,
            }
        });
        let result = PayloadBuffer::with_capacity(8, budget.clone(), 8);
        assert!(matches!(
            result,
            Err(TokenError::Unavailable | TokenError::CapacityExceeded)
        ));
        assert_eq!(calls.get(), action + 1);
        assert_eq!(budget.used(), 0);
    }
}

#[test]
fn preallocation_unwind_releases_charge() {
    let budget = PayloadBudget::with_limit(8);
    let _hook = test_support::install(|event| {
        if event.phase == Phase::CapacityObserved {
            panic!("synthetic preallocation unwind");
        }
        Action::Continue
    });
    let result = catch_unwind(AssertUnwindSafe(|| {
        PayloadBuffer::with_capacity(8, budget.clone(), 8)
    }));
    assert!(result.is_err());
    assert_eq!(budget.used(), 0);
}

#[test]
fn zero_preallocation_is_empty_and_does_not_allocate() -> Result<(), TokenError> {
    let budget = PayloadBudget::with_limit(0);
    let (events, _hook) = observe();
    let owner = PayloadBuffer::with_capacity(0, budget.clone(), 0)?;
    check_bytes(&owner, b"");
    assert_eq!(owner.capacity(), 0);
    assert!(events.borrow().is_empty());
    assert_eq!(budget.used(), 0);
    Ok(())
}
