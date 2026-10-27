//! Independent, memory-only allocation ownership for token-store backing.
//! No owner references the store or acquires its state mutex during destruction.
use super::tokenization::TokenError;
use std::{
    alloc::Layout,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc,
    },
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct AllocationCharge {
    pub plaintext: usize,
    pub metadata: usize,
}

// Pinned std 1.98.1 alloc/src/sync.rs:387-410, 428-435: ArcInner is
// repr(C, align(2)), with two atomic usize counters followed by its value.
// This accounts requested backing, not allocator bookkeeping or process RSS.
#[repr(C, align(2))]
struct ArcHeader {
    strong: AtomicUsize,
    weak: AtomicUsize,
}

pub(super) fn arc_backing_bytes<T>() -> Result<usize, TokenError> {
    Layout::new::<ArcHeader>()
        .extend(Layout::new::<T>())
        .map(|(layout, _)| layout.pad_to_align().size())
        .map_err(|_| TokenError::CapacityExceeded)
}

struct LedgerState {
    plaintext_limit: usize,
    metadata_limit: usize,
    plaintext: AtomicUsize,
    metadata: AtomicUsize,
    unavailable: AtomicBool,
}

#[derive(Clone)]
pub(super) struct StoreAllocationLedger {
    state: Arc<LedgerState>,
}

#[must_use]
pub(super) struct AllocationReservation {
    ledger: StoreAllocationLedger,
    charge: AllocationCharge,
}

#[cfg(test)]
impl StoreAllocationLedger {
    pub(super) fn limits_for_test(&self) -> AllocationCharge {
        AllocationCharge {
            plaintext: self.state.plaintext_limit,
            metadata: self.state.metadata_limit,
        }
    }
}

impl StoreAllocationLedger {
    pub(super) fn control_bytes() -> Result<usize, TokenError> {
        arc_backing_bytes::<LedgerState>()
    }
    pub(super) fn new(plaintext: usize, metadata: usize) -> Result<Self, TokenError> {
        let control = Self::control_bytes()?;
        if control > metadata {
            return Err(TokenError::CapacityExceeded);
        }
        // Establish the complete bootstrap charge before allocating its backing.
        Ok(Self {
            state: Arc::new(LedgerState {
                plaintext_limit: plaintext,
                metadata_limit: metadata,
                plaintext: AtomicUsize::new(0),
                metadata: AtomicUsize::new(control),
                unavailable: AtomicBool::new(false),
            }),
        })
    }
    pub(super) fn used(&self) -> AllocationCharge {
        AllocationCharge {
            plaintext: self.state.plaintext.load(Ordering::Relaxed),
            metadata: self.state.metadata.load(Ordering::Relaxed),
        }
    }
    pub(super) fn reserve(
        &self,
        charge: AllocationCharge,
    ) -> Result<AllocationReservation, TokenError> {
        if self.state.unavailable.load(Ordering::Relaxed) {
            return Err(TokenError::Unavailable);
        }
        Self::add(
            &self.state.plaintext,
            charge.plaintext,
            self.state.plaintext_limit,
        )?;
        if let Err(error) = Self::add(
            &self.state.metadata,
            charge.metadata,
            self.state.metadata_limit,
        ) {
            self.subtract(&self.state.plaintext, charge.plaintext);
            return Err(error);
        }
        Ok(AllocationReservation {
            ledger: self.clone(),
            charge,
        })
    }
    fn add(counter: &AtomicUsize, bytes: usize, limit: usize) -> Result<(), TokenError> {
        counter
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |used| {
                used.checked_add(bytes).filter(|next| *next <= limit)
            })
            .map(|_| ())
            .map_err(|_| TokenError::CapacityExceeded)
    }
    fn subtract(&self, counter: &AtomicUsize, bytes: usize) {
        if counter
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |used| {
                used.checked_sub(bytes)
            })
            .is_err()
        {
            // Never wrap or reset another owner's charge after an invariant failure.
            self.state.unavailable.store(true, Ordering::Relaxed);
        }
    }
    fn release(&self, charge: AllocationCharge) {
        self.subtract(&self.state.metadata, charge.metadata);
        self.subtract(&self.state.plaintext, charge.plaintext);
    }
}

impl Drop for AllocationReservation {
    fn drop(&mut self) {
        self.ledger.release(self.charge);
    }
}

pub(super) struct StoreVec<T> {
    // Field order keeps elements and backing alive until before the charge drops.
    values: Vec<T>,
    reservation: Option<AllocationReservation>,
    ledger: StoreAllocationLedger,
    limit: usize,
}
impl<T> StoreVec<T> {
    pub(super) fn new(ledger: StoreAllocationLedger, limit: usize) -> Self {
        Self {
            values: Vec::new(),
            reservation: None,
            ledger,
            limit,
        }
    }
    pub(super) fn with_capacity(
        capacity: usize,
        ledger: StoreAllocationLedger,
        limit: usize,
    ) -> Result<Self, TokenError> {
        if capacity > limit {
            return Err(TokenError::CapacityExceeded);
        }
        let bytes = capacity
            .checked_mul(std::mem::size_of::<T>())
            .filter(|bytes| *bytes <= isize::MAX as usize)
            .ok_or(TokenError::CapacityExceeded)?;
        let mut owner = Self::new(ledger, limit);
        owner.reservation = Some(owner.ledger.reserve(AllocationCharge {
            plaintext: 0,
            metadata: bytes,
        })?);
        #[cfg(test)]
        test_support::emit(
            test_support::Kind::Vector,
            test_support::Phase::BeforeAllocation,
            bytes,
            &owner.ledger,
        )?;
        owner
            .values
            .try_reserve_exact(capacity)
            .map_err(|_| TokenError::Unavailable)?;
        if std::mem::size_of::<T>() != 0 && owner.values.capacity() != capacity {
            return Err(TokenError::CapacityExceeded);
        }
        #[cfg(test)]
        test_support::emit(
            test_support::Kind::Vector,
            test_support::Phase::AfterAllocation,
            bytes,
            &owner.ledger,
        )?;
        Ok(owner)
    }
    pub(super) fn push(&mut self, value: T) -> Result<(), TokenError> {
        if self.values.len() >= self.limit {
            return Err(TokenError::CapacityExceeded);
        }
        if self.values.len() == self.values.capacity() {
            let capacity = self
                .values
                .capacity()
                .checked_mul(2)
                .map(|capacity| capacity.max(4).min(self.limit))
                .ok_or(TokenError::CapacityExceeded)?;
            let mut replacement = Self::with_capacity(capacity, self.ledger.clone(), self.limit)?;
            #[cfg(test)]
            test_support::emit(
                test_support::Kind::Vector,
                test_support::Phase::BeforeMove,
                self.values.len() * std::mem::size_of::<T>(),
                &self.ledger,
            )?;
            replacement.values.append(&mut self.values);
            // The complete old allocation is freed before its charge is released.
            std::mem::swap(self, &mut replacement);
            drop(replacement);
        }
        self.values.push(value);
        Ok(())
    }
    pub(super) fn push_without_growth(&mut self, value: T) -> Result<(), TokenError> {
        if self.values.len() >= self.limit || self.values.len() == self.values.capacity() {
            return Err(TokenError::CapacityExceeded);
        }
        self.values.push(value);
        Ok(())
    }
    pub(super) fn sort_unstable_by(&mut self, compare: impl FnMut(&T, &T) -> std::cmp::Ordering) {
        self.values.sort_unstable_by(compare);
    }
    pub(super) fn capacity(&self) -> usize {
        self.values.capacity()
    }
    #[cfg(test)]
    pub(super) fn ledger_for_test(&self) -> StoreAllocationLedger {
        self.ledger.clone()
    }
    pub(super) fn len(&self) -> usize {
        self.as_slice().len()
    }
    pub(super) fn is_empty(&self) -> bool {
        self.as_slice().is_empty()
    }
    pub(super) fn as_slice(&self) -> &[T] {
        &self.values
    }
    pub(super) fn as_mut_slice(&mut self) -> &mut [T] {
        &mut self.values
    }
}

pub(super) struct StoreText {
    text: String,
    reservation: AllocationReservation,
}
impl StoreText {
    pub(super) fn try_from_str(
        text: &str,
        ledger: StoreAllocationLedger,
        limit: usize,
    ) -> Result<Self, TokenError> {
        if text.len() > limit || text.len() > isize::MAX as usize {
            return Err(TokenError::CapacityExceeded);
        }
        let mut owner = Self {
            text: String::new(),
            reservation: ledger.reserve(AllocationCharge {
                plaintext: text.len(),
                metadata: 0,
            })?,
        };
        #[cfg(test)]
        test_support::emit(
            test_support::Kind::Text,
            test_support::Phase::BeforeAllocation,
            text.len(),
            &owner.reservation.ledger,
        )?;
        owner
            .text
            .try_reserve_exact(text.len())
            .map_err(|_| TokenError::Unavailable)?;
        if owner.text.capacity() != text.len() {
            return Err(TokenError::CapacityExceeded);
        }
        #[cfg(test)]
        test_support::emit(
            test_support::Kind::Text,
            test_support::Phase::AfterAllocation,
            text.len(),
            &owner.reservation.ledger,
        )?;
        #[cfg(test)]
        test_support::emit(
            test_support::Kind::Text,
            test_support::Phase::BeforeCopy,
            text.len(),
            &owner.reservation.ledger,
        )?;
        owner.text.push_str(text);
        Ok(owner)
    }
    #[cfg(test)]
    pub(super) fn ledger_for_test(&self) -> StoreAllocationLedger {
        self.reservation.ledger.clone()
    }
    pub(super) fn as_str(&self) -> &str {
        &self.text
    }
    pub(super) fn capacity(&self) -> usize {
        self.text.capacity()
    }
}

// All shared references stay inside this wrapper. No raw Arc/Weak or extraction API.
pub(super) struct SharedAllocation<T> {
    value: Option<Arc<T>>,
    ledger: StoreAllocationLedger,
    charge: AllocationCharge,
}
impl<T> SharedAllocation<T> {
    pub(super) fn new(value: T, ledger: StoreAllocationLedger) -> Result<Self, TokenError> {
        let charge = AllocationCharge {
            plaintext: 0,
            metadata: arc_backing_bytes::<T>()?,
        };
        let reservation = ledger.reserve(charge)?;
        #[cfg(test)]
        test_support::emit(
            test_support::Kind::Shared,
            test_support::Phase::BeforeAllocation,
            charge.metadata,
            &ledger,
        )?;
        let owner = Self {
            value: Some(Arc::new(value)),
            ledger,
            charge,
        };
        // Transfer the single charge only after publication cannot fail or unwind.
        let mut reservation = reservation;
        reservation.charge = AllocationCharge {
            plaintext: 0,
            metadata: 0,
        };
        #[cfg(test)]
        test_support::emit(
            test_support::Kind::Shared,
            test_support::Phase::AfterAllocation,
            charge.metadata,
            &owner.ledger,
        )?;
        Ok(owner)
    }
    pub(super) fn get(&self) -> Result<&T, TokenError> {
        self.value.as_deref().ok_or(TokenError::Unavailable)
    }
}
impl<T> Clone for SharedAllocation<T> {
    fn clone(&self) -> Self {
        Self {
            value: self.value.clone(),
            ledger: self.ledger.clone(),
            charge: self.charge,
        }
    }
}
impl<T> Drop for SharedAllocation<T> {
    fn drop(&mut self) {
        if let Some(value) = self.value.take().and_then(Arc::into_inner) {
            // Pinned std sync.rs:1230-1254 deallocates the Arc backing before
            // returning T. The final winner alone returns the unique charge.
            let reservation = AllocationReservation {
                ledger: self.ledger.clone(),
                charge: self.charge,
            };
            drop(value);
            drop(reservation);
        }
    }
}

#[cfg(test)]
impl<T> Drop for StoreVec<T> {
    fn drop(&mut self) {
        test_support::observe_drop(
            test_support::Kind::Vector,
            self.values.capacity() * std::mem::size_of::<T>(),
            &self.ledger,
        );
    }
}

#[cfg(test)]
pub(super) mod test_support {
    use super::{AllocationCharge, StoreAllocationLedger, TokenError};
    use std::{cell::RefCell, marker::PhantomData, rc::Rc};
    #[derive(Clone, Copy, PartialEq, Eq)]
    pub(in super::super) enum Kind {
        Vector,
        Text,
        Shared,
        Mapping,
        Namespace,
    }
    #[derive(Clone, Copy, PartialEq, Eq)]
    pub(in super::super) enum Phase {
        BeforeAllocation,
        AfterAllocation,
        BeforeCopy,
        BeforeMove,
        BeforeDrop,
    }
    #[derive(Clone, Copy)]
    pub(in super::super) struct Observation {
        pub kind: Kind,
        pub phase: Phase,
        pub bytes: usize,
        pub used: AllocationCharge,
    }
    type Hook = Box<dyn FnMut(Observation) -> Result<(), TokenError>>;
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
    pub(in super::super) fn install(
        hook: impl FnMut(Observation) -> Result<(), TokenError> + 'static,
    ) -> Guard {
        Guard {
            previous: HOOK.with(|slot| slot.replace(Some(Box::new(hook)))),
            _thread_bound: PhantomData,
        }
    }
    pub(in super::super) fn emit(
        kind: Kind,
        phase: Phase,
        bytes: usize,
        ledger: &StoreAllocationLedger,
    ) -> Result<(), TokenError> {
        HOOK.with(|slot| {
            struct Restore<'a> {
                slot: &'a RefCell<Option<Hook>>,
                hook: Option<Hook>,
            }
            impl Drop for Restore<'_> {
                fn drop(&mut self) {
                    *self.slot.borrow_mut() = self.hook.take();
                }
            }
            let mut restore = Restore {
                slot,
                hook: slot.borrow_mut().take(),
            };
            restore
                .hook
                .as_mut()
                .map(|hook| {
                    hook(Observation {
                        kind,
                        phase,
                        bytes,
                        used: ledger.used(),
                    })
                })
                .unwrap_or(Ok(()))
        })
    }
    pub(in super::super) fn observe_drop(kind: Kind, bytes: usize, ledger: &StoreAllocationLedger) {
        let _ = emit(kind, Phase::BeforeDrop, bytes, ledger);
    }
}
