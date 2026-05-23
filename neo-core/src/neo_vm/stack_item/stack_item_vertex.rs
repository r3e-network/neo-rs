//! Stack item vertex utilities for graph operations.

use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT_STACK_ITEM_ID: AtomicUsize = AtomicUsize::new(1);

/// Generates a unique ID for stack items used in graph traversal.
pub fn next_stack_item_id() -> usize {
    NEXT_STACK_ITEM_ID.fetch_add(1, Ordering::SeqCst)
}
