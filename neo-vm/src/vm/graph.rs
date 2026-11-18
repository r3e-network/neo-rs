//! Identity allocation for compound VM stack items.

use core::sync::atomic::{AtomicUsize, Ordering};

static NEXT_STACK_ITEM_ID: AtomicUsize = AtomicUsize::new(1);

/// Generates a unique identity for compound VM stack items.
pub fn next_stack_item_id() -> usize {
    NEXT_STACK_ITEM_ID.fetch_add(1, Ordering::SeqCst)
}
