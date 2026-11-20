// Copyright (c) 2024 Neo-RS Project
// This file is part of the neo-rs project
// Licensed under the MIT License
// See LICENSE file for details

//! Bounded recycling of evaluation-stack backing buffers.
//!
//! Every [`EvaluationStack`](crate::evaluation_stack::EvaluationStack) owns a
//! `Vec<StackItem>`. When an execution context is unloaded that vector is
//! dropped, so a script with thousands of calls would allocate and free a
//! backing buffer per context. [`StackAllocator`] removes that churn by taking
//! the backing allocation of a *dead* context and handing it to the next
//! context that needs one.
//!
//! ## Ownership model
//!
//! A buffer is only ever reclaimed from a context that is the exclusive owner
//! of its evaluation stack:
//!
//! * `CALL`/`CALL_L`/`CALLA` clone the context's `SharedStates`, which shares
//!   the stack behind an `Arc<Mutex<EvaluationStack>>`.
//! * The reclaim path uses `Arc::get_mut`, which succeeds only when the strong
//!   count is 1 and no weak handle exists. While a caller context still aliases
//!   the stack the call fails and the buffer is left alone, so a live alias can
//!   never observe a recycled allocation.
//!
//! Recycling never touches the [`ReferenceCounter`](crate::reference_counter::ReferenceCounter):
//! the items in the reclaimed buffer are dropped exactly when the owning
//! context would have dropped them, which is the same point at which the
//! previous implementation released them. `EvaluationStack::clear` keeps its
//! own reference-counting behaviour and is not used here.
//!
//! ## Bounds
//!
//! Retained buffers are bounded on three axes so a large script cannot pin
//! unbounded memory:
//!
//! * [`StackAllocatorLimits::max_retained_buffers`] - number of vectors kept.
//! * [`StackAllocatorLimits::max_retained_capacity`] - element capacity of a
//!   single retained vector.
//! * [`StackAllocatorLimits::max_retained_bytes`] - total bytes of element
//!   storage across all retained vectors.
//!
//! A buffer that would break a bound is dropped instead of retained, and the
//! reason is counted in [`StackAllocatorStats`] so the decision is observable.

use crate::stack_item::StackItem;
use parking_lot::Mutex;

/// Size of one element of a retained backing buffer.
const BUFFER_ELEMENT_SIZE: usize = std::mem::size_of::<StackItem>();

/// Bounds applied to retained evaluation-stack backing buffers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StackAllocatorLimits {
    /// Maximum number of backing buffers kept for reuse.
    pub max_retained_buffers: usize,
    /// Maximum element capacity of a single retained backing buffer.
    pub max_retained_capacity: usize,
    /// Maximum total element storage, in bytes, across all retained buffers.
    pub max_retained_bytes: usize,
}

impl StackAllocatorLimits {
    /// Default number of retained backing buffers.
    pub const DEFAULT_MAX_RETAINED_BUFFERS: usize = 8;
    /// Default element capacity of one retained backing buffer.
    pub const DEFAULT_MAX_RETAINED_CAPACITY: usize = 4096;
    /// Default total element storage, in bytes, across all retained buffers.
    pub const DEFAULT_MAX_RETAINED_BYTES: usize = 256 * 1024;

    /// Creates a new set of bounds.
    #[must_use]
    pub const fn new(
        max_retained_buffers: usize,
        max_retained_capacity: usize,
        max_retained_bytes: usize,
    ) -> Self {
        Self {
            max_retained_buffers,
            max_retained_capacity,
            max_retained_bytes,
        }
    }
}

impl Default for StackAllocatorLimits {
    #[inline]
    fn default() -> Self {
        Self {
            max_retained_buffers: Self::DEFAULT_MAX_RETAINED_BUFFERS,
            max_retained_capacity: Self::DEFAULT_MAX_RETAINED_CAPACITY,
            max_retained_bytes: Self::DEFAULT_MAX_RETAINED_BYTES,
        }
    }
}

/// Why a returned backing buffer was not retained.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StackBufferRejection {
    /// The buffer never allocated storage, so there is nothing to keep.
    Empty,
    /// The buffer's capacity is above `max_retained_capacity` or its bytes
    /// alone would exceed `max_retained_bytes`.
    Oversized,
    /// The pool already holds `max_retained_buffers` buffers.
    PoolFull,
    /// Keeping the buffer would push the pooled bytes over `max_retained_bytes`.
    ByteBudget,
}

/// Snapshot of the allocator's retained memory and reuse counters.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct StackAllocatorStats {
    /// Backing buffers currently retained.
    pub retained_buffers: usize,
    /// Total element capacity across the retained buffers.
    pub retained_capacity: usize,
    /// Total element storage, in bytes, across the retained buffers.
    pub retained_bytes: usize,
    /// Buffers handed out by [`StackAllocator::acquire`].
    pub acquired: u64,
    /// Buffers accepted by [`StackAllocator::recycle`].
    pub recycled: u64,
    /// Returns rejected because the buffer had no allocation.
    pub rejected_empty: u64,
    /// Returns rejected because the buffer was too large to retain.
    pub rejected_oversized: u64,
    /// Returns rejected because the pool already held enough buffers.
    pub rejected_pool_full: u64,
    /// Returns rejected because of the total byte budget.
    pub rejected_byte_budget: u64,
}

impl StackAllocatorStats {
    /// Total number of rejected returns across every reason.
    #[must_use]
    pub const fn rejected(&self) -> u64 {
        self.rejected_empty
            + self.rejected_oversized
            + self.rejected_pool_full
            + self.rejected_byte_budget
    }
}

#[derive(Debug, Default)]
struct StackAllocatorState {
    buffers: Vec<Vec<StackItem>>,
    retained_bytes: usize,
    acquired: u64,
    recycled: u64,
    rejected_empty: u64,
    rejected_oversized: u64,
    rejected_pool_full: u64,
    rejected_byte_budget: u64,
}

/// Bounded pool of cleared `Vec<StackItem>` backing allocations.
///
/// The allocator holds only empty vectors: [`Self::recycle`] clears the
/// incoming buffer before admitting it, so no live [`StackItem`] is ever kept
/// alive by the pool. All methods take `&self`; the state is behind a
/// `parking_lot::Mutex` so an engine can acquire a buffer from `create_context`
/// without needing exclusive access to the engine.
#[derive(Debug)]
pub struct StackAllocator {
    limits: StackAllocatorLimits,
    state: Mutex<StackAllocatorState>,
}

impl StackAllocator {
    /// Creates an allocator with the supplied bounds.
    #[must_use]
    pub fn new(limits: StackAllocatorLimits) -> Self {
        Self {
            limits,
            state: Mutex::new(StackAllocatorState::default()),
        }
    }

    /// Returns the active bounds.
    #[inline]
    #[must_use]
    pub const fn limits(&self) -> StackAllocatorLimits {
        self.limits
    }

    /// Replaces the bounds and drops every retained buffer that no longer fits.
    ///
    /// Must not be called while an evaluation stack still holds a buffer taken
    /// from this allocator; that only ever happens inside one engine's
    /// execution, so call this before loading a script.
    pub fn set_limits(&mut self, limits: StackAllocatorLimits) {
        self.limits = limits;
        let mut state = self.state.lock();
        state.buffers.retain(|buffer| {
            let capacity = buffer.capacity();
            capacity != 0
                && capacity <= limits.max_retained_capacity
                && capacity.saturating_mul(BUFFER_ELEMENT_SIZE) <= limits.max_retained_bytes
        });
        state.buffers.truncate(limits.max_retained_buffers);

        let mut retained_bytes = 0usize;
        for buffer in &state.buffers {
            retained_bytes = retained_bytes
                .saturating_add(buffer.capacity().saturating_mul(BUFFER_ELEMENT_SIZE));
        }
        state.retained_bytes = retained_bytes;
        while state.retained_bytes > limits.max_retained_bytes {
            // Drop from the tail until the byte budget is respected.
            let Some(dropped) = state.buffers.pop() else {
                state.retained_bytes = 0;
                break;
            };
            state.retained_bytes = state
                .retained_bytes
                .saturating_sub(dropped.capacity().saturating_mul(BUFFER_ELEMENT_SIZE));
        }
    }

    /// Takes a cleared backing buffer, or `None` when the pool is empty.
    ///
    /// The returned vector is always empty and always has a non-zero capacity.
    pub fn acquire(&self) -> Option<Vec<StackItem>> {
        let mut state = self.state.lock();
        let buffer = state.buffers.pop()?;
        state.retained_bytes = state
            .retained_bytes
            .saturating_sub(buffer.capacity().saturating_mul(BUFFER_ELEMENT_SIZE));
        state.acquired = state.acquired.saturating_add(1);
        Some(buffer)
    }

    /// Offers a backing buffer for reuse.
    ///
    /// The buffer is cleared first, which drops every remaining [`StackItem`]
    /// while keeping the allocation. A buffer that cannot be retained within
    /// the configured bounds is dropped and the reason is returned.
    pub fn recycle(&self, mut buffer: Vec<StackItem>) -> Result<(), StackBufferRejection> {
        // Release the items the dead owner was holding. This is the same work
        // the owning context would do when it went out of scope.
        buffer.clear();

        let capacity = buffer.capacity();
        let bytes = capacity.saturating_mul(BUFFER_ELEMENT_SIZE);

        let mut state = self.state.lock();
        if capacity == 0 {
            state.rejected_empty = state.rejected_empty.saturating_add(1);
            return Err(StackBufferRejection::Empty);
        }
        if capacity > self.limits.max_retained_capacity || bytes > self.limits.max_retained_bytes {
            state.rejected_oversized = state.rejected_oversized.saturating_add(1);
            return Err(StackBufferRejection::Oversized);
        }
        if state.buffers.len() >= self.limits.max_retained_buffers {
            state.rejected_pool_full = state.rejected_pool_full.saturating_add(1);
            return Err(StackBufferRejection::PoolFull);
        }
        if state.retained_bytes.saturating_add(bytes) > self.limits.max_retained_bytes {
            state.rejected_byte_budget = state.rejected_byte_budget.saturating_add(1);
            return Err(StackBufferRejection::ByteBudget);
        }

        state.retained_bytes = state.retained_bytes.saturating_add(bytes);
        state.recycled = state.recycled.saturating_add(1);
        state.buffers.push(buffer);
        Ok(())
    }

    /// Returns the current retained memory and reuse counters.
    #[must_use]
    pub fn stats(&self) -> StackAllocatorStats {
        let state = self.state.lock();
        let retained_capacity: usize = state.buffers.iter().map(Vec::capacity).sum();
        StackAllocatorStats {
            retained_buffers: state.buffers.len(),
            retained_capacity,
            retained_bytes: state.retained_bytes,
            acquired: state.acquired,
            recycled: state.recycled,
            rejected_empty: state.rejected_empty,
            rejected_oversized: state.rejected_oversized,
            rejected_pool_full: state.rejected_pool_full,
            rejected_byte_budget: state.rejected_byte_budget,
        }
    }

    /// Drops every retained buffer.
    pub fn clear(&self) {
        let mut state = self.state.lock();
        state.buffers.clear();
        state.retained_bytes = 0;
    }

    /// Number of buffers currently retained.
    #[inline]
    #[must_use]
    pub fn retained_buffers(&self) -> usize {
        self.state.lock().buffers.len()
    }

    /// Total element storage, in bytes, of the retained buffers.
    #[inline]
    #[must_use]
    pub fn retained_bytes(&self) -> usize {
        self.state.lock().retained_bytes
    }
}

impl Default for StackAllocator {
    #[inline]
    fn default() -> Self {
        Self::new(StackAllocatorLimits::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn buffer_with_capacity(capacity: usize) -> Vec<StackItem> {
        let mut buffer = Vec::with_capacity(capacity);
        assert_eq!(
            buffer.capacity(),
            capacity,
            "an allocation request of {capacity} elements must not be rounded up"
        );
        buffer.push(StackItem::from_int(7));
        buffer
    }

    #[test]
    fn a_recycled_buffer_is_cleared_and_reused() {
        let allocator = StackAllocator::default();
        assert!(allocator.acquire().is_none());

        let buffer = buffer_with_capacity(32);
        let address = buffer.as_ptr();
        assert_eq!(allocator.recycle(buffer), Ok(()));
        assert_eq!(allocator.stats().retained_buffers, 1);

        let reused = allocator.acquire().expect("buffer was retained");
        assert!(reused.is_empty());
        assert_eq!(reused.capacity(), 32);
        // Same allocation, no growth between recycle and reuse.
        assert_eq!(reused.as_ptr(), address);
        assert_eq!(allocator.stats().acquired, 1);
        assert!(allocator.stats().retained_buffers == 0);
    }

    #[test]
    fn an_empty_allocation_is_not_retained() {
        let allocator = StackAllocator::default();
        assert_eq!(
            allocator.recycle(Vec::new()),
            Err(StackBufferRejection::Empty)
        );
        assert_eq!(allocator.stats().rejected_empty, 1);
        assert_eq!(allocator.stats().retained_buffers, 0);
    }

    #[test]
    fn retained_buffer_count_is_bounded() {
        let limits = StackAllocatorLimits::new(2, 64, 64 * BUFFER_ELEMENT_SIZE * 4);
        let allocator = StackAllocator::new(limits);

        for attempt in 0..5 {
            let result = allocator.recycle(buffer_with_capacity(16));
            if attempt < 2 {
                assert_eq!(result, Ok(()));
            } else {
                assert_eq!(result, Err(StackBufferRejection::PoolFull));
            }
        }
        assert_eq!(allocator.stats().retained_buffers, 2);
        assert_eq!(allocator.stats().recycled, 2);
        assert_eq!(allocator.stats().rejected_pool_full, 3);
    }

    #[test]
    fn retained_buffer_capacity_is_bounded() {
        let limits = StackAllocatorLimits::new(8, 32, 64 * BUFFER_ELEMENT_SIZE);
        let allocator = StackAllocator::new(limits);

        assert_eq!(
            allocator.recycle(buffer_with_capacity(33)),
            Err(StackBufferRejection::Oversized)
        );
        assert_eq!(allocator.recycle(buffer_with_capacity(16)), Ok(()));
        assert_eq!(allocator.stats().rejected_oversized, 1);
        assert_eq!(allocator.stats().retained_buffers, 1);
        assert_eq!(allocator.stats().retained_capacity, 16);
        assert_eq!(allocator.stats().retained_bytes, 16 * BUFFER_ELEMENT_SIZE);
    }

    #[test]
    fn retained_byte_budget_is_bounded() {
        let limits = StackAllocatorLimits::new(8, 64, 24 * BUFFER_ELEMENT_SIZE);
        let allocator = StackAllocator::new(limits);

        assert_eq!(allocator.recycle(buffer_with_capacity(16)), Ok(()));
        // 16 + 16 elements would need 32 slots worth of bytes, over the budget.
        assert_eq!(
            allocator.recycle(buffer_with_capacity(16)),
            Err(StackBufferRejection::ByteBudget)
        );
        assert_eq!(allocator.stats().rejected_byte_budget, 1);
        assert_eq!(allocator.stats().retained_bytes, 16 * BUFFER_ELEMENT_SIZE);
    }

    #[test]
    fn tightening_the_bounds_drops_retained_buffers() {
        let mut allocator = StackAllocator::default();
        for _ in 0..4 {
            assert_eq!(allocator.recycle(buffer_with_capacity(16)), Ok(()));
        }
        assert_eq!(allocator.retained_buffers(), 4);

        allocator.set_limits(StackAllocatorLimits::new(1, 32, 64 * BUFFER_ELEMENT_SIZE));
        assert_eq!(allocator.retained_buffers(), 1);
        assert_eq!(allocator.retained_bytes(), 16 * BUFFER_ELEMENT_SIZE);
    }

    #[test]
    fn clearing_drops_every_retained_buffer() {
        let allocator = StackAllocator::default();
        assert_eq!(allocator.recycle(buffer_with_capacity(16)), Ok(()));
        allocator.clear();
        assert_eq!(allocator.retained_buffers(), 0);
        assert_eq!(allocator.retained_bytes(), 0);
        assert!(allocator.acquire().is_none());
    }

    #[test]
    fn statistics_report_every_rejection_reason() {
        let stats = StackAllocatorStats {
            rejected_empty: 1,
            rejected_oversized: 2,
            rejected_pool_full: 3,
            rejected_byte_budget: 4,
            ..StackAllocatorStats::default()
        };
        assert_eq!(stats.rejected(), 10);
    }
}
