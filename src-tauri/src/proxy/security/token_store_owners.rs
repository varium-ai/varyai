//! Independent, memory-only allocation ownership for token-store backing.
//! No owner references the store or acquires its state mutex during destruction.
use super::tokenization::TokenError;
use std::{
    alloc::Layout,
    sync::atomic::{AtomicBool, AtomicUsize, Ordering},
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

// Strong-only sharing keeps allocation fallible on stable Rust. No pointer or weak
// reference escapes this module; the last owner frees backing before returning T.
#[repr(C, align(2))]
struct StrongInner<T> {
    header: ArcHeader,
    value: T,
}

pub(super) struct FallibleShared<T> {
    pointer: std::ptr::NonNull<StrongInner<T>>,
    _owns: std::marker::PhantomData<StrongInner<T>>,
}

// SAFETY: ownership is atomic, and shared access exposes only &T. Moving and
// sharing owners therefore require the same Send + Sync bounds as std Arc.
unsafe impl<T: Send + Sync> Send for FallibleShared<T> {}
unsafe impl<T: Send + Sync> Sync for FallibleShared<T> {}

impl<T> FallibleShared<T> {
    pub(super) fn new(value: T) -> Result<Self, TokenError> {
        let mut backing = Vec::<StrongInner<T>>::new();
        backing
            .try_reserve_exact(1)
            .map_err(|_| TokenError::Unavailable)?;
        if backing.capacity() != 1 {
            return Err(TokenError::CapacityExceeded);
        }
        backing.push(StrongInner {
            header: ArcHeader {
                strong: AtomicUsize::new(1),
                weak: AtomicUsize::new(0),
            },
            value,
        });
        let pointer = std::ptr::NonNull::from(&mut backing[0]);
        std::mem::forget(backing);
        Ok(Self {
            pointer,
            _owns: std::marker::PhantomData,
        })
    }
    pub(super) fn into_inner(self) -> Option<T> {
        let owner = std::mem::ManuallyDrop::new(self);
        owner.release()
    }
    fn release(&self) -> Option<T> {
        // SAFETY: this owner keeps the allocation alive until its decrement. A
        // saturated count deliberately retains backing and its charge forever,
        // rather than wrapping and freeing memory still owned by another clone.
        let inner = unsafe { self.pointer.as_ref() };
        let previous = inner
            .header
            .strong
            .fetch_update(Ordering::Release, Ordering::Relaxed, |count| {
                (count != usize::MAX).then(|| count - 1)
            })
            .ok()?;
        if previous != 1 {
            return None;
        }
        std::sync::atomic::fence(Ordering::Acquire);
        // SAFETY: the release decrement selected the unique final owner. All
        // other accesses happen before the acquire fence; no weak owner exists.
        // Moving T out and reconstructing a zero-length Vec frees only backing,
        // so even a later T::drop panic cannot leak or double-free this block.
        let value = unsafe { std::ptr::read(std::ptr::addr_of!((*self.pointer.as_ptr()).value)) };
        unsafe { drop(Vec::from_raw_parts(self.pointer.as_ptr(), 0, 1)) };
        Some(value)
    }
}
impl<T> std::ops::Deref for FallibleShared<T> {
    type Target = T;
    fn deref(&self) -> &T {
        // SAFETY: a live owner prevents the final release; T is immutable here.
        unsafe { &self.pointer.as_ref().value }
    }
}
impl<T> Clone for FallibleShared<T> {
    fn clone(&self) -> Self {
        // Saturation is a conservative, permanently charged leak, never an abort
        // or a wrapping counter. It cannot be reached by bounded proof metadata.
        let _ = unsafe { self.pointer.as_ref() }.header.strong.fetch_update(
            Ordering::Relaxed,
            Ordering::Relaxed,
            |count| Some(count.saturating_add(1)),
        );
        Self {
            pointer: self.pointer,
            _owns: std::marker::PhantomData,
        }
    }
}
impl<T> Drop for FallibleShared<T> {
    fn drop(&mut self) {
        drop(self.release());
    }
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
    state: FallibleShared<LedgerState>,
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
        let charge = AllocationCharge {
            plaintext: 0,
            metadata: control,
        };
        #[cfg(test)]
        test_support::emit_bootstrap(test_support::Phase::BeforeAllocation, charge)?;
        // Establish the complete bootstrap charge before allocating its backing.
        let ledger = Self {
            state: FallibleShared::new(LedgerState {
                plaintext_limit: plaintext,
                metadata_limit: metadata,
                plaintext: AtomicUsize::new(0),
                metadata: AtomicUsize::new(charge.metadata),
                unavailable: AtomicBool::new(false),
            })?,
        };
        #[cfg(test)]
        test_support::emit_bootstrap(test_support::Phase::AfterAllocation, charge)?;
        Ok(ledger)
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
    value: Option<FallibleShared<T>>,
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
            value: Some(FallibleShared::new(value)?),
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
        if let Some(value) = self.value.take().and_then(FallibleShared::into_inner) {
            // Backing is already freed. The final winner returns the unique charge
            // after payload destruction, including unwinding from its destructor.
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
        emit_observation(Observation {
            kind,
            phase,
            bytes,
            used: ledger.used(),
        })
    }
    pub(super) fn emit_bootstrap(phase: Phase, used: AllocationCharge) -> Result<(), TokenError> {
        emit_observation(Observation {
            kind: Kind::Namespace,
            phase,
            bytes: used.metadata,
            used,
        })
    }
    fn emit_observation(observation: Observation) -> Result<(), TokenError> {
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
                .map(|hook| hook(observation))
                .unwrap_or(Ok(()))
        })
    }
    pub(in super::super) fn observe_drop(kind: Kind, bytes: usize, ledger: &StoreAllocationLedger) {
        let _ = emit(kind, Phase::BeforeDrop, bytes, ledger);
    }
}

#[cfg(test)]
pub(super) mod allocator_test_support {
    use super::StrongInner;
    use std::{
        alloc::{GlobalAlloc, Layout, System},
        cell::Cell,
        marker::PhantomData,
        rc::Rc,
    };

    #[derive(Clone, Copy)]
    struct Failure {
        size: usize,
        align: usize,
        denied: bool,
    }
    thread_local! {
        static FAILURE: Cell<Option<Failure>> = const { Cell::new(None) };
        static DEALLOCATION: Cell<Option<(usize, usize)>> = const { Cell::new(None) };
    }
    struct TestAllocator;
    #[global_allocator]
    static ALLOCATOR: TestAllocator = TestAllocator;

    fn deny(layout: Layout) -> bool {
        FAILURE
            .try_with(|slot| {
                let Some(mut failure) = slot.get() else {
                    return false;
                };
                if failure.denied
                    || failure.size != layout.size()
                    || failure.align != layout.align()
                {
                    return false;
                }
                failure.denied = true;
                slot.set(Some(failure));
                true
            })
            .unwrap_or(false)
    }
    // SAFETY: every non-injected operation delegates its original pointer and
    // layout to System. The one injected allocation returns null without owning
    // backing; thread-local observation performs no allocation or unwinding.
    unsafe impl GlobalAlloc for TestAllocator {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            if deny(layout) {
                std::ptr::null_mut()
            } else {
                unsafe { System.alloc(layout) }
            }
        }
        unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
            if deny(layout) {
                std::ptr::null_mut()
            } else {
                unsafe { System.alloc_zeroed(layout) }
            }
        }
        unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
            unsafe { System.dealloc(pointer, layout) }
            let _ = DEALLOCATION.try_with(|slot| {
                if let Some((watched, count)) = slot.get() {
                    if watched == pointer as usize && count == 0 {
                        slot.set(Some((watched, 1)));
                    }
                }
            });
        }
        unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
            unsafe { System.realloc(pointer, layout, size) }
        }
    }
    pub(in super::super) struct Guard {
        previous: Option<Failure>,
        _thread_bound: PhantomData<Rc<()>>,
    }
    impl Guard {
        pub(in super::super) fn denied(&self) -> bool {
            FAILURE.with(|slot| slot.get().is_some_and(|failure| failure.denied))
        }
    }
    impl Drop for Guard {
        fn drop(&mut self) {
            FAILURE.with(|slot| slot.set(self.previous));
        }
    }
    pub(in super::super) struct DeallocationGuard {
        previous: Option<(usize, usize)>,
        _thread_bound: PhantomData<Rc<()>>,
    }
    impl Drop for DeallocationGuard {
        fn drop(&mut self) {
            DEALLOCATION.with(|slot| slot.set(self.previous));
        }
    }
    pub(in super::super) fn watch_shared_backing<T>(
        owner: &super::FallibleShared<T>,
    ) -> DeallocationGuard {
        DeallocationGuard {
            previous: DEALLOCATION
                .with(|slot| slot.replace(Some((owner.pointer.as_ptr() as usize, 0)))),
            _thread_bound: PhantomData,
        }
    }
    pub(in super::super) fn observed_deallocations() -> usize {
        DEALLOCATION.with(|slot| slot.get().map_or(0, |(_, count)| count))
    }
    pub(in super::super) fn fail_next_shared<T>() -> Guard {
        let layout = Layout::new::<StrongInner<T>>();
        Guard {
            previous: FAILURE.with(|slot| {
                slot.replace(Some(Failure {
                    size: layout.size(),
                    align: layout.align(),
                    denied: false,
                }))
            }),
            _thread_bound: PhantomData,
        }
    }
}

#[cfg(test)]
mod fallible_shared_tests {
    use super::{
        allocator_test_support::fail_next_shared, arc_backing_bytes, AllocationCharge,
        FallibleShared, LedgerState, SharedAllocation, StoreAllocationLedger, TokenError,
    };
    use std::cell::Cell;

    #[test]
    fn shared_control_allocator_null_returns_ledger_error_without_process_oom() {
        let guard = fail_next_shared::<LedgerState>();
        let result = StoreAllocationLedger::new(0, 4096);
        let denied = guard.denied();
        drop(guard);
        assert!(denied);
        assert_eq!(result.err(), Some(TokenError::Unavailable));
        assert!(StoreAllocationLedger::new(0, 4096).is_ok());
    }

    struct DropCount<'a>(&'a Cell<usize>);
    impl Drop for DropCount<'_> {
        fn drop(&mut self) {
            self.0.set(self.0.get() + 1);
        }
    }
    #[test]
    fn shared_control_allocator_null_releases_reservation_and_payload_once(
    ) -> Result<(), TokenError> {
        let ledger = StoreAllocationLedger::new(0, 4096)?;
        let baseline = ledger.used();
        let drops = Cell::new(0);
        let value = DropCount(&drops);
        let guard = fail_next_shared::<DropCount<'_>>();
        let result = SharedAllocation::new(value, ledger.clone());
        let denied = guard.denied();
        drop(guard);
        assert!(denied);
        assert_eq!(result.err(), Some(TokenError::Unavailable));
        assert!(drops.get() == 1);
        assert_eq!(ledger.used(), baseline);
        Ok(())
    }

    #[repr(align(64))]
    struct Aligned(u8);
    #[test]
    fn shared_control_fallible_layout_alignment_and_zero_sized_payload_are_exact(
    ) -> Result<(), TokenError> {
        for (actual, charged) in [
            (
                std::mem::size_of::<super::StrongInner<()>>(),
                arc_backing_bytes::<()>()?,
            ),
            (
                std::mem::size_of::<super::StrongInner<Aligned>>(),
                arc_backing_bytes::<Aligned>()?,
            ),
        ] {
            assert!(actual == charged);
        }
        let ledger = StoreAllocationLedger::new(0, 4096)?;
        let baseline = ledger.used();
        let aligned = SharedAllocation::new(Aligned(7), ledger.clone())?;
        assert!(aligned.get()?.0 == 7);
        assert!((aligned.get()? as *const Aligned as usize).is_multiple_of(64));
        drop(aligned);
        let zero = SharedAllocation::new((), ledger.clone())?;
        let second = zero.clone();
        drop(zero);
        drop(second);
        assert_eq!(ledger.used(), baseline);
        let direct = FallibleShared::new(())?;
        assert!(FallibleShared::into_inner(direct).is_some());
        assert_eq!(
            baseline,
            AllocationCharge {
                plaintext: 0,
                metadata: StoreAllocationLedger::control_bytes()?
            }
        );
        Ok(())
    }

    #[test]
    fn shared_control_backing_is_freed_before_payload_even_during_unwind() -> Result<(), TokenError>
    {
        use super::allocator_test_support::{observed_deallocations, watch_shared_backing};
        struct Probe<'a> {
            drops: &'a Cell<usize>,
            panic: bool,
        }
        impl Drop for Probe<'_> {
            fn drop(&mut self) {
                assert!(observed_deallocations() == 1);
                self.drops.set(self.drops.get() + 1);
                if self.panic {
                    panic!("synthetic shared payload interruption");
                }
            }
        }
        for panic in [false, true] {
            let ledger = StoreAllocationLedger::new(0, 4096)?;
            let baseline = ledger.used();
            let drops = Cell::new(0);
            let owner = SharedAllocation::new(
                Probe {
                    drops: &drops,
                    panic,
                },
                ledger.clone(),
            )?;
            let backing = owner.value.as_ref().ok_or(TokenError::Unavailable)?;
            let watch = watch_shared_backing(backing);
            let second = owner.clone();
            drop(owner);
            assert!(observed_deallocations() == 0);
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(second)));
            assert!(result.is_err() == panic);
            assert!(observed_deallocations() == 1);
            drop(watch);
            assert!(drops.get() == 1);
            assert_eq!(ledger.used(), baseline);
        }
        Ok(())
    }

    #[test]
    fn shared_control_saturation_never_wraps_frees_or_releases_charge() -> Result<(), TokenError> {
        use super::allocator_test_support::{observed_deallocations, watch_shared_backing};
        use std::sync::atomic::Ordering;
        let ledger = StoreAllocationLedger::new(0, 4096)?;
        let baseline = ledger.used();
        let drops = Cell::new(0);
        let owner = SharedAllocation::new(DropCount(&drops), ledger.clone())?;
        let charged = ledger.used();
        let backing = owner.value.as_ref().ok_or(TokenError::Unavailable)?;
        let watch = watch_shared_backing(backing);
        let pointer = backing.pointer;
        // SAFETY: this single-threaded test retains the unique owner throughout
        // the artificial counter transition; no concurrent access is possible.
        unsafe { pointer.as_ref() }
            .header
            .strong
            .store(usize::MAX - 1, Ordering::Relaxed);
        let second = owner.clone();
        let third = second.clone();
        assert!(
            unsafe { pointer.as_ref() }
                .header
                .strong
                .load(Ordering::Relaxed)
                == usize::MAX
        );
        drop(second);
        drop(third);
        assert!(
            unsafe { pointer.as_ref() }
                .header
                .strong
                .load(Ordering::Relaxed)
                == usize::MAX
        );
        assert!(observed_deallocations() == 0);
        assert!(drops.get() == 0);
        assert_eq!(ledger.used(), charged);
        // SAFETY: only the retained owner remains. Reset solely to reclaim this
        // synthetic saturation fixture; production never resets a shared count.
        unsafe { pointer.as_ref() }
            .header
            .strong
            .store(1, Ordering::Relaxed);
        drop(owner);
        assert!(observed_deallocations() == 1);
        drop(watch);
        assert!(drops.get() == 1);
        assert_eq!(ledger.used(), baseline);
        Ok(())
    }
}
