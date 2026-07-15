//! Stable process-local identities for compound VM values.

use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_STACK_ITEM_ID: AtomicU64 = AtomicU64::new(1);

/// Allocates a stable process-local compound identity.
#[must_use]
pub fn next_stack_item_id() -> u64 {
    NEXT_STACK_ITEM_ID.fetch_add(1, Ordering::Relaxed)
}
