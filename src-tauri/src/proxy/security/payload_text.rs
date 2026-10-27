//! Private fixed-capacity UTF-8 owner; no formatting or unguarded extraction.
use super::{
    payload_budget::{PayloadBudget, Reservation},
    tokenization::TokenError,
};

pub(super) struct PayloadText {
    text: String,
    reservation: Reservation,
    #[cfg(test)]
    budget: PayloadBudget,
}

impl PayloadText {
    pub(super) fn with_capacity(
        capacity: usize,
        budget: PayloadBudget,
        local_limit: usize,
    ) -> Result<Self, TokenError> {
        if capacity > local_limit || capacity > isize::MAX as usize {
            return Err(TokenError::CapacityExceeded);
        }
        // Allocation drops before its charge, including errors and unwinding.
        let mut owner = Self {
            text: String::new(),
            reservation: budget.reserve(capacity)?,
            #[cfg(test)]
            budget: budget.clone(),
        };
        if capacity != 0 {
            #[cfg(test)]
            test_support::emit(test_support::Observation {
                phase: test_support::Phase::BeforeAllocation,
                bytes: capacity,
                used: budget.used(),
            })?;
            owner
                .text
                .try_reserve_exact(capacity)
                .map_err(|_| TokenError::Unavailable)?;
            if owner.text.capacity() != capacity {
                return Err(TokenError::CapacityExceeded);
            }
            #[cfg(test)]
            test_support::emit(test_support::Observation {
                phase: test_support::Phase::AfterAllocation,
                bytes: capacity,
                used: budget.used(),
            })?;
        }
        Ok(owner)
    }
    pub(super) fn try_from_str(
        text: &str,
        budget: PayloadBudget,
        local_limit: usize,
    ) -> Result<Self, TokenError> {
        let mut owner = Self::with_capacity(text.len(), budget, local_limit)?;
        owner.append(text)?;
        Ok(owner)
    }
    pub(super) fn append(&mut self, text: &str) -> Result<(), TokenError> {
        self.text
            .len()
            .checked_add(text.len())
            .filter(|next| *next <= self.reservation.bytes() && *next <= self.text.capacity())
            .ok_or(TokenError::CapacityExceeded)?;
        if text.is_empty() {
            return Ok(());
        }
        #[cfg(test)]
        test_support::emit(test_support::Observation {
            phase: test_support::Phase::BeforeCopy,
            bytes: text.len(),
            used: self.budget.used(),
        })?;
        // UTF-8 is supplied by &str and the complete capacity is already owned.
        self.text.push_str(text);
        Ok(())
    }
    pub(super) fn as_str(&self) -> &str {
        &self.text
    }
    pub(super) fn capacity(&self) -> usize {
        self.text.capacity()
    }
}

#[cfg(test)]
pub(super) mod test_support {
    use super::TokenError;
    use std::{cell::RefCell, marker::PhantomData, rc::Rc};
    #[derive(Clone, Copy, PartialEq, Eq)]
    pub(in super::super) enum Phase {
        BeforeAllocation,
        AfterAllocation,
        BeforeCopy,
    }
    #[derive(Clone, Copy)]
    pub(in super::super) struct Observation {
        pub phase: Phase,
        pub bytes: usize,
        pub used: usize,
    }
    pub(in super::super) enum Action {
        Continue,
        Fail,
    }
    type Hook = Box<dyn FnMut(Observation) -> Action>;
    thread_local! { static HOOK: RefCell<Option<Hook>> = const { RefCell::new(None) }; }
    pub(in super::super) struct Guard {
        previous: Option<Hook>,
        _thread_bound: PhantomData<Rc<()>>,
    }
    impl Drop for Guard {
        fn drop(&mut self) {
            HOOK.with(|slot| slot.replace(self.previous.take()));
        }
    }
    pub(in super::super) fn install(hook: impl FnMut(Observation) -> Action + 'static) -> Guard {
        Guard {
            previous: HOOK.with(|slot| slot.replace(Some(Box::new(hook)))),
            _thread_bound: PhantomData,
        }
    }
    pub(in super::super) fn emit(observation: Observation) -> Result<(), TokenError> {
        HOOK.with(
            |slot| match slot.borrow_mut().as_mut().map(|hook| hook(observation)) {
                Some(Action::Fail) => Err(TokenError::Unavailable),
                _ => Ok(()),
            },
        )
    }
}
