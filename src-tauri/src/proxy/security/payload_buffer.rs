//! Private, budget-bound restoration bytes, not response-path integration.
//!
//! Charge requested capacity before allocation or copying. Replacement growth uses
//! exact required capacity and charges both allocations until the old one is dropped.
//! Accounting covers Vec capacity, not allocator slack, process RSS or zeroization.
use super::{
    payload_budget::{PayloadBudget, Reservation},
    tokenization::TokenError,
};

pub(super) struct PayloadBuffer {
    // Rust drops fields in declaration order: release the allocation before its charge.
    bytes: Vec<u8>,
    reservation: Reservation,
    local_limit: usize,
    budget: PayloadBudget,
}

fn checked_required(
    current: usize,
    additional: usize,
    local_limit: usize,
) -> Result<usize, TokenError> {
    current
        .checked_add(additional)
        .filter(|required| *required <= local_limit && *required <= isize::MAX as usize)
        .ok_or(TokenError::CapacityExceeded)
}

impl PayloadBuffer {
    pub(super) fn new(budget: PayloadBudget, local_limit: usize) -> Result<Self, TokenError> {
        Ok(Self {
            bytes: Vec::new(),
            reservation: budget.reserve(0)?,
            local_limit,
            budget,
        })
    }

    /// Reserve an empty allocation once, for bounded serializers with a known size.
    pub(super) fn with_capacity(
        capacity: usize,
        budget: PayloadBudget,
        local_limit: usize,
    ) -> Result<Self, TokenError> {
        checked_required(0, capacity, local_limit)?;
        let mut owner = Self {
            bytes: Vec::new(),
            reservation: budget.reserve(capacity)?,
            local_limit,
            budget,
        };
        if capacity != 0 {
            owner.allocate(capacity)?;
        }
        Ok(owner)
    }

    pub(super) fn try_from_slice(
        bytes: &[u8],
        budget: PayloadBudget,
        local_limit: usize,
    ) -> Result<Self, TokenError> {
        let mut owner = Self::new(budget, local_limit)?;
        owner.append(bytes)?;
        Ok(owner)
    }

    pub(super) fn as_slice(&self) -> &[u8] {
        &self.bytes
    }

    pub(super) fn len(&self) -> usize {
        self.bytes.len()
    }

    pub(super) fn capacity(&self) -> usize {
        self.bytes.capacity()
    }

    pub(super) fn append(&mut self, bytes: &[u8]) -> Result<(), TokenError> {
        let required = checked_required(self.len(), bytes.len(), self.local_limit)?;
        if bytes.is_empty() {
            return Ok(());
        }
        if required <= self.capacity() {
            #[cfg(test)]
            self.observe_copy(bytes.len())?;
            // Enough retained, fully charged capacity exists; this cannot allocate.
            self.bytes.extend_from_slice(bytes);
            return Ok(());
        }

        // Do not grow the old Vec in place: realloc may retain both allocations.
        // A complete owner also guarantees allocation-before-charge drop on errors/unwind.
        let mut replacement = Self {
            bytes: Vec::new(),
            reservation: self.budget.reserve(required)?,
            local_limit: self.local_limit,
            budget: self.budget.clone(),
        };
        replacement.allocate(required)?;
        #[cfg(test)]
        replacement.observe_copy(required)?;
        replacement.bytes.extend_from_slice(&self.bytes);
        replacement.bytes.extend_from_slice(bytes);
        std::mem::swap(self, &mut replacement);
        // Drop the old owner (Vec first, reservation second) only after the swap.
        Ok(())
    }

    // Only called on an empty replacement with its entire capacity already charged.
    fn allocate(&mut self, requested: usize) -> Result<(), TokenError> {
        #[cfg(test)]
        test_support::emit(self.observation(test_support::Phase::BeforeAllocation, requested, 0))?;
        self.bytes
            .try_reserve_exact(requested)
            .map_err(|_| TokenError::Unavailable)?;
        let actual = self.capacity();
        #[cfg(test)]
        let observed = test_support::emit(self.observation(
            test_support::Phase::CapacityObserved,
            requested,
            0,
        ))?
        .map_or(actual, |capacity| capacity);
        #[cfg(not(test))]
        let observed = actual;
        // Vec may report more than requested. Never copy into uncharged capacity;
        // reject and drop the tentative owner instead of charging after allocation.
        if actual > self.reservation.bytes()
            || observed > self.reservation.bytes()
            || actual < requested
            || observed < requested
        {
            return Err(TokenError::CapacityExceeded);
        }
        Ok(())
    }

    /// Retain the allocation and its full charge, including now-unused bytes.
    pub(super) fn clear(&mut self) {
        self.bytes.clear();
    }

    /// Retain the allocation and its full charge; lengths above the current one are a no-op.
    pub(super) fn truncate(&mut self, len: usize) {
        self.bytes.truncate(len);
    }

    /// Make a distinct charged copy of logical bytes, not of spare retained capacity.
    pub(super) fn try_clone(&self) -> Result<Self, TokenError> {
        Self::try_from_slice(self.as_slice(), self.budget.clone(), self.local_limit)
    }

    #[cfg(test)]
    fn observation(
        &self,
        phase: test_support::Phase,
        requested: usize,
        bytes_to_copy: usize,
    ) -> test_support::Observation {
        test_support::Observation {
            phase,
            requested,
            capacity: self.capacity(),
            bytes_to_copy,
            used: self.budget.used(),
        }
    }

    #[cfg(test)]
    fn observe_copy(&self, bytes_to_copy: usize) -> Result<(), TokenError> {
        test_support::emit(self.observation(
            test_support::Phase::BeforeCopy,
            self.reservation.bytes(),
            bytes_to_copy,
        ))?;
        Ok(())
    }
}

#[cfg(test)]
pub(super) mod test_support {
    use super::TokenError;
    use std::{cell::RefCell, marker::PhantomData, rc::Rc};

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub(in super::super) enum Phase {
        BeforeAllocation,
        CapacityObserved,
        BeforeCopy,
    }

    // Observations expose only sizes and accounting, never payload bytes.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub(in super::super) struct Observation {
        pub phase: Phase,
        pub requested: usize,
        pub capacity: usize,
        pub bytes_to_copy: usize,
        pub used: usize,
    }

    pub(in super::super) enum Action {
        Continue,
        FailAllocation,
        ReportCapacity(usize),
    }

    type Hook = Box<dyn FnMut(Observation) -> Action>;
    thread_local! {
        static HOOK: RefCell<Option<Hook>> = const { RefCell::new(None) };
    }

    pub(in super::super) struct HookGuard {
        previous: Option<Hook>,
        _thread_bound: PhantomData<Rc<()>>,
    }

    impl Drop for HookGuard {
        fn drop(&mut self) {
            HOOK.with(|slot| slot.replace(self.previous.take()));
        }
    }

    pub(in super::super) fn install(
        hook: impl FnMut(Observation) -> Action + 'static,
    ) -> HookGuard {
        HookGuard {
            previous: HOOK.with(|slot| slot.replace(Some(Box::new(hook)))),
            _thread_bound: PhantomData,
        }
    }

    pub(super) fn emit(event: Observation) -> Result<Option<usize>, TokenError> {
        HOOK.with(|slot| match slot.borrow_mut().as_mut() {
            Some(hook) => match hook(event) {
                Action::Continue => Ok(None),
                Action::FailAllocation => Err(TokenError::Unavailable),
                Action::ReportCapacity(capacity) => Ok(Some(capacity)),
            },
            None => Ok(None),
        })
    }

    pub(in super::super) fn checked_required(
        current: usize,
        additional: usize,
        local_limit: usize,
    ) -> Result<usize, TokenError> {
        super::checked_required(current, additional, local_limit)
    }
}
