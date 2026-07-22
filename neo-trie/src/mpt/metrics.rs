//! Request-local MPT hash-work accounting.

use std::cell::Cell;

thread_local! {
    static HASH_COMPUTATIONS: Cell<u64> = const { Cell::new(0) };
}

pub(super) fn record_hash_computation() {
    HASH_COMPUTATIONS.with(|count| count.set(count.get().saturating_add(1)));
}

pub(super) fn hash_computations() -> u64 {
    HASH_COMPUTATIONS.with(Cell::get)
}
