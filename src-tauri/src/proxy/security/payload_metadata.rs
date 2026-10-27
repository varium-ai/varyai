//! Private carrier metadata owners. No Debug, serialization or implicit clone.
use super::{
    payload_budget::{PayloadBudget, Reservation},
    tokenization::TokenError,
};

pub(super) struct PayloadVec<T> {
    values: Vec<T>,
    reservation: Reservation,
    budget: PayloadBudget,
}

impl<T> PayloadVec<T> {
    pub(super) fn new(budget: PayloadBudget) -> Result<Self, TokenError> {
        Ok(Self {
            values: Vec::new(),
            reservation: budget.reserve(0)?,
            budget,
        })
    }
    pub(super) fn push(&mut self, value: T) -> Result<(), TokenError> {
        if self.values.len() == self.values.capacity() {
            let capacity = self
                .values
                .capacity()
                .checked_mul(2)
                .map(|capacity| capacity.max(4))
                .ok_or(TokenError::CapacityExceeded)?;
            let bytes = capacity
                .checked_mul(std::mem::size_of::<T>())
                .filter(|bytes| *bytes <= isize::MAX as usize)
                .ok_or(TokenError::CapacityExceeded)?;
            // The complete old allocation remains charged until it is freed.
            let reservation = self.budget.reserve(bytes)?;
            #[cfg(test)]
            observe(
                &self.budget,
                test_support::Kind::Vector,
                test_support::Phase::BeforeAllocation,
                bytes,
            )?;
            let mut replacement = Vec::new();
            replacement
                .try_reserve_exact(capacity)
                .map_err(|_| TokenError::Unavailable)?;
            if replacement.capacity() > capacity {
                return Err(TokenError::CapacityExceeded);
            }
            #[cfg(test)]
            observe(
                &self.budget,
                test_support::Kind::Vector,
                test_support::Phase::AfterAllocation,
                bytes,
            )?;
            replacement.append(&mut self.values);
            self.values = replacement;
            self.reservation = reservation;
        }
        self.values.push(value);
        Ok(())
    }
    pub(super) fn pop(&mut self) -> Option<T> {
        self.values.pop()
    }
    pub(super) fn as_slice(&self) -> &[T] {
        &self.values
    }
    pub(super) fn last_mut(&mut self) -> Option<&mut T> {
        self.values.last_mut()
    }
    pub(super) fn len(&self) -> usize {
        self.values.len()
    }
    pub(super) fn capacity(&self) -> usize {
        self.values.capacity()
    }
    // These operations cannot change allocation capacity or its reservation.
    pub(super) fn sort_unstable_by(&mut self, compare: impl FnMut(&T, &T) -> std::cmp::Ordering) {
        self.values.sort_unstable_by(compare);
    }
    pub(super) fn dedup_by(&mut self, same: impl FnMut(&mut T, &mut T) -> bool) {
        self.values.dedup_by(same);
    }
    pub(super) fn get_mut(&mut self, index: usize) -> Option<&mut T> {
        self.values.get_mut(index)
    }
}

pub(super) struct PayloadKey {
    text: String,
    reservation: Reservation,
}
impl PayloadKey {
    pub(super) fn try_from_str(text: &str, budget: &PayloadBudget) -> Result<Self, TokenError> {
        let mut owner = Self {
            text: String::new(),
            reservation: budget.reserve(text.len())?,
        };
        #[cfg(test)]
        observe(
            budget,
            test_support::Kind::Key,
            test_support::Phase::BeforeAllocation,
            text.len(),
        )?;
        owner
            .text
            .try_reserve_exact(text.len())
            .map_err(|_| TokenError::Unavailable)?;
        if owner.text.capacity() > text.len() {
            return Err(TokenError::CapacityExceeded);
        }
        #[cfg(test)]
        observe(
            budget,
            test_support::Kind::Key,
            test_support::Phase::AfterAllocation,
            text.len(),
        )?;
        owner.text.push_str(text);
        Ok(owner)
    }
    pub(super) fn as_str(&self) -> &str {
        &self.text
    }
}

#[cfg(test)]
fn observe(
    budget: &PayloadBudget,
    kind: test_support::Kind,
    phase: test_support::Phase,
    bytes: usize,
) -> Result<(), TokenError> {
    test_support::emit(test_support::Observation {
        phase,
        kind,
        bytes,
        used: budget.used(),
    })
}

#[cfg(test)]
pub(super) mod test_support {
    use std::{cell::RefCell, marker::PhantomData, rc::Rc};
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub(in super::super) enum Phase {
        BeforeAllocation,
        AfterAllocation,
    }
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub(in super::super) enum Kind {
        Vector,
        Key,
    }
    #[derive(Clone, Copy)]
    pub(in super::super) struct Observation {
        pub phase: Phase,
        pub kind: Kind,
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
    pub(in super::super) fn emit(observation: Observation) -> Result<(), super::TokenError> {
        HOOK.with(
            |slot| match slot.borrow_mut().as_mut().map(|hook| hook(observation)) {
                Some(Action::Fail) => Err(super::TokenError::Unavailable),
                _ => Ok(()),
            },
        )
    }
}
