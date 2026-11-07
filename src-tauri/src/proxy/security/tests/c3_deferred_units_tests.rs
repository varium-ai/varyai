//! Ownership characterization for typed, same-barrier deferred transfers.
use super::{DeferredUnits, ToolBarrier};
use crate::proxy::security::{
    payload_budget::{PayloadBudget, Reservation},
    payload_metadata::test_support::{self, Action, Kind, Phase},
    payload_text::PayloadText,
};
use std::{cell::Cell, rc::Rc, sync::Arc};

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[test]
fn c3_deferred_units_identity_is_lazy_shared_and_charged_characterization() -> TestResult {
    let budget = PayloadBudget::with_limit(4096);
    let mut barrier = ToolBarrier::<u8>::new(budget.clone(), 8, 2)?;
    assert!(budget.used() == 0, "new_barrier_charged_deferred_identity");
    barrier.hold_all();
    assert!(budget.used() == 0, "hold_all_charged_deferred_identity");
    let donor = barrier.deferred_owner()?;
    let identity_charge = std::mem::size_of::<Reservation>() + 2 * std::mem::size_of::<usize>();
    assert!(
        budget.used() == identity_charge,
        "deferred_identity_charge_missing"
    );
    let second = barrier.deferred_owner()?;
    assert!(
        Arc::ptr_eq(&donor.owner, &second.owner),
        "mint_changed_deferred_identity"
    );
    assert!(
        budget.used() == identity_charge,
        "mint_duplicated_identity_charge"
    );
    drop(barrier);
    drop(donor);
    assert!(
        budget.used() == identity_charge,
        "live_donor_lost_identity_charge"
    );
    drop(second);
    assert!(budget.used() == 0, "deferred_identity_drop_retained_charge");
    Ok(())
}

#[test]
fn c3_deferred_units_identity_failure_preserves_barrier_and_retries_characterization() -> TestResult
{
    let charge = std::mem::size_of::<Reservation>() + 2 * std::mem::size_of::<usize>();
    let budget = PayloadBudget::with_limit(charge);
    let blocker = budget.reserve(1)?;
    let mut barrier = ToolBarrier::<u8>::new(budget.clone(), 8, 2)?;
    barrier.hold_all();
    assert!(
        barrier.deferred_owner().is_err(),
        "identity_reservation_failure_not_reached"
    );
    assert!(
        barrier.deferred_owner.is_none(),
        "failed_mint_committed_identity"
    );
    assert!(
        barrier.is_active() && barrier.len() == 0 && barrier.retained_bytes() == 0,
        "failed_mint_changed_barrier"
    );
    assert!(budget.used() == 1, "failed_mint_changed_charge");
    drop(blocker);
    let donor = barrier.deferred_owner()?;
    assert!(budget.used() == charge, "identity_reservation_retry_failed");
    drop(donor);
    drop(barrier);
    assert!(budget.used() == 0, "identity_retry_drop_retained_charge");
    Ok(())
}

#[test]
fn c3_deferred_units_same_budget_other_barrier_rejected_characterization() -> TestResult {
    let budget = PayloadBudget::with_limit(4096);
    let mut destination = ToolBarrier::<u8>::new(budget.clone(), 8, 2)?;
    let mut source = ToolBarrier::<u8>::new(budget.clone(), 8, 2)?;
    let own_donor = destination.deferred_owner()?;
    let mut donor = source.deferred_owner()?;
    donor.push(22, 2)?;
    let source_owner = Arc::clone(&donor.owner);
    let destination_owner = Arc::clone(&own_donor.owner);
    let units = donor.units().as_ptr();
    let baseline = budget.used();
    assert!(
        destination.adopt_deferred(&mut donor).is_err(),
        "same_budget_other_barrier_owner_accepted"
    );
    assert!(
        donor.units() == [22] && donor.units().as_ptr() == units && donor.retained_bytes() == 2,
        "rejected_other_barrier_donor_changed"
    );
    assert!(
        Arc::ptr_eq(&donor.owner, &source_owner),
        "rejected_other_barrier_identity_changed"
    );
    let next = destination.deferred_owner()?;
    assert!(
        Arc::ptr_eq(&next.owner, &destination_owner),
        "rejected_destination_identity_changed"
    );
    assert!(
        !destination.is_active() && destination.len() == 0 && destination.retained_bytes() == 0,
        "rejected_other_barrier_destination_changed"
    );
    assert!(
        budget.used() == baseline,
        "rejected_other_barrier_charge_changed"
    );
    drop((
        next,
        own_donor,
        source_owner,
        destination_owner,
        donor,
        source,
        destination,
    ));
    assert!(
        budget.used() == 0,
        "other_barrier_rejection_drop_retained_charge"
    );
    Ok(())
}

#[test]
fn c3_deferred_units_transfer_is_noalloc_and_preserves_activation_characterization() -> TestResult {
    for active in [false, true] {
        let budget = PayloadBudget::with_limit(4096);
        let mut barrier = ToolBarrier::new(budget.clone(), 8, 2)?;
        if active {
            barrier.hold_all();
        }
        let mut donor = barrier.deferred_owner()?;
        donor.push(11u8, 3)?;
        donor.push(22u8, 5)?;
        let owner = Arc::clone(&donor.owner);
        let units = donor.units().as_ptr();
        let baseline = budget.used();
        let attempts = Rc::new(Cell::new(0));
        let observed = Rc::clone(&attempts);
        let hook = test_support::install(move |_| {
            observed.set(observed.get() + 1);
            Action::Fail
        });
        let result = barrier.adopt_deferred(&mut donor);
        drop(hook);
        result?;
        assert!(
            attempts.get() == 0,
            "typed_transfer_attempted_metadata_allocation"
        );
        assert!(
            barrier.units() == [11, 22]
                && barrier.units().as_ptr() == units
                && barrier.retained_bytes() == 8,
            "typed_transfer_changed_payload_or_bytes"
        );
        assert!(
            barrier.is_active() == active,
            "typed_transfer_changed_activation"
        );
        assert!(
            donor.len() == 0 && donor.retained_bytes() == 0 && Arc::ptr_eq(&donor.owner, &owner),
            "typed_transfer_changed_donor_identity_or_accounting"
        );
        assert!(budget.used() == baseline, "typed_transfer_changed_charge");
        drop((owner, donor, barrier));
        assert!(budget.used() == 0, "typed_transfer_drop_retained_charge");
    }
    Ok(())
}

#[test]
fn c3_deferred_units_push_bounds_are_atomic_characterization() -> TestResult {
    let budget = PayloadBudget::with_limit(4096);
    let mut barrier = ToolBarrier::new(budget.clone(), 8, 2)?;
    let mut donor = barrier.deferred_owner()?;
    let identity = budget.used();
    assert!(
        donor.push(11u8, 9).is_err(),
        "deferred_push_accepted_excess_bytes"
    );
    assert!(
        donor.len() == 0 && donor.retained_bytes() == 0 && budget.used() == identity,
        "oversize_push_changed_empty_donor"
    );
    donor.push(22, 3)?;
    let baseline = budget.used();
    let units = donor.units().as_ptr();
    assert!(
        donor.push(33, usize::MAX).is_err(),
        "deferred_push_accepted_byte_overflow"
    );
    assert!(
        donor.units() == [22]
            && donor.units().as_ptr() == units
            && donor.retained_bytes() == 3
            && budget.used() == baseline,
        "overflow_push_changed_donor"
    );
    donor.push(44, 5)?;
    assert!(
        donor.push(55, 0).is_err(),
        "deferred_push_accepted_excess_units"
    );
    assert!(
        donor.units() == [22, 44] && donor.retained_bytes() == 8 && budget.used() == baseline,
        "unit_limit_push_changed_donor"
    );
    drop((donor, barrier));
    assert!(budget.used() == 0, "deferred_push_drop_retained_charge");
    Ok(())
}

#[test]
fn c3_deferred_units_metadata_failure_rolls_back_and_retries_characterization() -> TestResult {
    for phase in [Phase::BeforeAllocation, Phase::AfterAllocation] {
        let budget = PayloadBudget::with_limit(4096);
        let mut barrier = ToolBarrier::new(budget.clone(), 100, 4)?;
        let mut donor = barrier.deferred_owner()?;
        let baseline = budget.used();
        let owner = Arc::clone(&donor.owner);
        let reached = Rc::new(Cell::new(false));
        let observed = Rc::clone(&reached);
        let hook = test_support::install(move |event| {
            if event.kind == Kind::Vector && event.phase == phase {
                observed.set(true);
                Action::Fail
            } else {
                Action::Continue
            }
        });
        let unit = PayloadText::try_from_str("synthetic-unit", budget.clone(), 100)?;
        let result = donor.push(unit, 14);
        drop(hook);
        assert!(
            reached.get() && result.is_err(),
            "deferred_metadata_fault_not_reached"
        );
        assert!(
            donor.len() == 0 && donor.retained_bytes() == 0 && Arc::ptr_eq(&donor.owner, &owner),
            "deferred_metadata_failure_changed_donor"
        );
        assert!(
            budget.used() == baseline,
            "deferred_metadata_failure_retained_payload_charge"
        );
        donor.push(
            PayloadText::try_from_str("synthetic-unit", budget.clone(), 100)?,
            14,
        )?;
        assert!(
            donor.len() == 1 && donor.retained_bytes() == 14,
            "deferred_metadata_retry_failed"
        );
        drop((owner, donor, barrier));
        assert!(budget.used() == 0, "deferred_metadata_drop_retained_charge");
    }
    Ok(())
}

#[test]
fn c3_deferred_units_clear_retains_identity_and_charged_backing_characterization() -> TestResult {
    let budget = PayloadBudget::with_limit(4096);
    let mut barrier = ToolBarrier::new(budget.clone(), 100, 4)?;
    let mut donor = barrier.deferred_owner()?;
    let identity = budget.used();
    let owner = Arc::clone(&donor.owner);
    donor.push(
        PayloadText::try_from_str("synthetic-unit", budget.clone(), 100)?,
        14,
    )?;
    let units = donor.units().as_ptr();
    let retained_backing = budget.used() - 14;
    assert!(
        retained_backing > identity,
        "deferred_clear_fixture_missing_backing_charge"
    );
    donor.clear();
    assert!(
        donor.len() == 0 && donor.retained_bytes() == 0 && donor.units().as_ptr() == units,
        "deferred_clear_changed_backing_or_bytes"
    );
    assert!(
        Arc::ptr_eq(&donor.owner, &owner) && budget.used() == retained_backing,
        "deferred_clear_lost_identity_or_backing_charge"
    );
    donor.push(
        PayloadText::try_from_str("synthetic-unit", budget.clone(), 100)?,
        14,
    )?;
    assert!(
        donor.units().as_ptr() == units,
        "deferred_clear_prevented_backing_reuse"
    );
    barrier.adopt_deferred(&mut donor)?;
    assert!(
        barrier.len() == 1 && donor.len() == 0 && donor.retained_bytes() == 0,
        "cleared_donor_identity_could_not_transfer"
    );
    drop((owner, donor, barrier));
    assert!(budget.used() == 0, "deferred_clear_drop_retained_charge");
    Ok(())
}

#[test]
fn c3_deferred_units_transfer_rejects_mismatched_limits_characterization() -> TestResult {
    for change_bytes in [false, true] {
        let budget = PayloadBudget::with_limit(4096);
        let mut barrier = ToolBarrier::new(budget.clone(), 8, 2)?;
        let mut donor = barrier.deferred_owner()?;
        donor.push(22u8, 2)?;
        if change_bytes {
            donor.max_bytes += 1;
        } else {
            donor.max_units += 1;
        }
        let baseline = budget.used();
        let units = donor.units().as_ptr();
        assert!(
            barrier.adopt_deferred(&mut donor).is_err(),
            "typed_transfer_accepted_mismatched_limits"
        );
        assert!(
            donor.units() == [22] && donor.units().as_ptr() == units && donor.retained_bytes() == 2,
            "limit_rejection_changed_donor"
        );
        assert!(
            barrier.len() == 0
                && barrier.retained_bytes() == 0
                && !barrier.is_active()
                && budget.used() == baseline,
            "limit_rejection_changed_destination_or_charge"
        );
        drop((donor, barrier));
        assert!(budget.used() == 0, "limit_rejection_drop_retained_charge");
    }
    Ok(())
}

#[test]
fn c3_deferred_units_transfer_rejects_nonzero_destination_bytes_characterization() -> TestResult {
    let budget = PayloadBudget::with_limit(4096);
    let mut barrier = ToolBarrier::new(budget.clone(), 8, 2)?;
    let mut donor = barrier.deferred_owner()?;
    donor.push(22u8, 2)?;
    // A malformed destination must fail even when its unit collection is empty.
    barrier.bytes = 1;
    let baseline = budget.used();
    assert!(
        barrier.adopt_deferred(&mut donor).is_err(),
        "typed_transfer_accepted_nonzero_destination_bytes"
    );
    assert!(
        donor.units() == [22] && donor.retained_bytes() == 2,
        "destination_byte_rejection_changed_donor"
    );
    assert!(
        barrier.len() == 0
            && barrier.retained_bytes() == 1
            && !barrier.is_active()
            && budget.used() == baseline,
        "destination_byte_rejection_changed_state"
    );
    drop((donor, barrier));
    assert!(
        budget.used() == 0,
        "destination_byte_rejection_drop_retained_charge"
    );
    Ok(())
}

#[test]
fn c3_deferred_units_empty_transfer_retains_owner_characterization() -> TestResult {
    let budget = PayloadBudget::with_limit(4096);
    let mut barrier = ToolBarrier::<u8>::new(budget.clone(), 8, 2)?;
    let mut donor: DeferredUnits<u8> = barrier.deferred_owner()?;
    let owner = Arc::clone(&donor.owner);
    let baseline = budget.used();
    barrier.adopt_deferred(&mut donor)?;
    assert!(
        !barrier.is_active() && barrier.len() == 0 && barrier.retained_bytes() == 0,
        "empty_typed_transfer_changed_barrier"
    );
    assert!(
        donor.len() == 0
            && donor.retained_bytes() == 0
            && Arc::ptr_eq(&donor.owner, &owner)
            && budget.used() == baseline,
        "empty_typed_transfer_changed_owner_or_charge"
    );
    donor.push(22, 2)?;
    barrier.adopt_deferred(&mut donor)?;
    assert!(
        barrier.units() == [22] && barrier.retained_bytes() == 2,
        "empty_transfer_lost_reusable_identity"
    );
    drop((owner, donor, barrier));
    assert!(
        budget.used() == 0,
        "empty_typed_transfer_drop_retained_charge"
    );
    Ok(())
}
