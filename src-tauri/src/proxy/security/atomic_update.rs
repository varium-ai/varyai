//! Atomic usize updates compatible with the declared minimum Rust version.
use std::sync::atomic::{AtomicUsize, Ordering};

// Match the standard update contract without relying on renamed, newer APIs.
pub(super) fn try_update_usize<F>(
    counter: &AtomicUsize,
    set_order: Ordering,
    fetch_order: Ordering,
    mut update: F,
) -> Result<usize, usize>
where
    F: FnMut(usize) -> Option<usize>,
{
    let mut previous = counter.load(fetch_order);
    while let Some(next) = update(previous) {
        match counter.compare_exchange_weak(previous, next, set_order, fetch_order) {
            Ok(value) => return Ok(value),
            Err(value) => previous = value,
        }
    }
    Err(previous)
}
