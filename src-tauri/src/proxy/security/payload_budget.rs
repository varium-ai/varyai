//! Private accounting for restoration payload allocations, separate from token mappings.
//!
//! Reserve before allocating, growing or copying payloads; retain the reservation until
//! that allocation is released. Distinct copies need distinct charges. Immutable sharing
//! uses an `Arc` around the payload and its reservation, not a cloned reservation.
//! This primitive does not allocate payloads, enforce caller integration or cap process RSS.
use super::tokenization::TokenError;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc, LazyLock,
};

const DEFAULT_LIMIT: usize = 64 * 1024 * 1024;
static GLOBAL: LazyLock<Arc<BudgetState>> =
    LazyLock::new(|| Arc::new(BudgetState::new(DEFAULT_LIMIT)));

struct BudgetState {
    limit: usize,
    used: AtomicUsize,
}

impl BudgetState {
    fn new(limit: usize) -> Self {
        Self {
            limit,
            used: AtomicUsize::new(0),
        }
    }

    fn charge(&self, bytes: usize) -> Result<(), TokenError> {
        // These atomics account only for bytes; they do not publish payload memory.
        self.used
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |used| {
                used.checked_add(bytes).filter(|next| *next <= self.limit)
            })
            .map(|_| ())
            .map_err(|_| TokenError::CapacityExceeded)
    }

    fn release(&self, bytes: usize) -> Result<(), TokenError> {
        // Never wrap or reset another owner's charge if an invariant is violated.
        self.used
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |used| {
                used.checked_sub(bytes)
            })
            .map(|_| ())
            .map_err(|_| TokenError::Unavailable)
    }
}

/// Cloning a handle shares the same ceiling and counter, without reserving bytes.
#[derive(Clone)]
pub(super) struct PayloadBudget {
    state: Arc<BudgetState>,
}

impl Default for PayloadBudget {
    fn default() -> Self {
        Self {
            state: Arc::clone(&GLOBAL),
        }
    }
}

impl PayloadBudget {
    #[cfg(test)]
    pub(super) fn with_limit(limit: usize) -> Self {
        Self {
            state: Arc::new(BudgetState::new(limit)),
        }
    }

    pub(super) fn limit(&self) -> usize {
        self.state.limit
    }

    /// An observation only: admission must use `reserve`, never a preflight read.
    pub(super) fn used(&self) -> usize {
        self.state.used.load(Ordering::Relaxed)
    }

    pub(super) fn reserve(&self, bytes: usize) -> Result<Reservation, TokenError> {
        let mut reservation = Reservation {
            budget: self.clone(),
            bytes: 0,
        };
        reservation.grow(bytes)?;
        Ok(reservation)
    }
}

/// A single, movable charge. Share an immutable allocation's owner via `Arc` instead
/// of duplicating this guard. No debug or serialization implementation exposes owners.
#[must_use]
pub(super) struct Reservation {
    budget: PayloadBudget,
    bytes: usize,
}

impl Reservation {
    pub(super) fn bytes(&self) -> usize {
        self.bytes
    }

    /// Charge additional bytes before growth or copying; failure leaves this guard intact.
    pub(super) fn grow(&mut self, additional: usize) -> Result<(), TokenError> {
        let next = self
            .bytes
            .checked_add(additional)
            .ok_or(TokenError::CapacityExceeded)?;
        self.budget.state.charge(additional)?;
        self.bytes = next;
        Ok(())
    }

    /// Release bytes only after the corresponding allocation has actually been reduced.
    pub(super) fn shrink(&mut self, bytes: usize) -> Result<(), TokenError> {
        let next = self
            .bytes
            .checked_sub(bytes)
            .ok_or(TokenError::Unavailable)?;
        self.budget.state.release(bytes)?;
        self.bytes = next;
        Ok(())
    }
}

impl Drop for Reservation {
    fn drop(&mut self) {
        // Drop cannot fail or panic; a broken invariant conservatively retains the charge.
        let _ = self.budget.state.release(self.bytes);
    }
}
