//! Task-manager sync-window planning helpers.
//!
//! Mirrors `neo-core::network::p2p::task_manager::scheduling`. These are the
//! pure planning primitives the task manager uses each scheduling pass to decide
//! which block indexes, header ranges, and inventory hashes to request, and how
//! many peers may share a single outstanding task. They are kept free of any
//! actor/system/ledger type so they can be unit-tested in isolation and reused
//! by the ported task manager.

use super::payloads::inv_payload::MAX_HASHES_COUNT;
use neo_primitives::UInt256;
use std::collections::HashMap;
use std::hash::Hash;

/// Maximum concurrent sessions that may share the same inventory hash task.
pub const MAX_CONCURRENT_TASKS: u32 = 3;
/// Maximum number of sequential block heights requested in a single round.
pub const MAX_BLOCK_INDEX_BATCH: u32 = MAX_HASHES_COUNT as u32;
/// Maximum heights assigned to one peer in a single request/pipeline round.
pub const BLOCK_INDEX_REQUEST_BATCH: u32 = 100;
/// Number of batch windows to keep in flight globally.
pub const BLOCK_INDEX_WINDOW_MULTIPLIER: u32 = 1;
/// How far ahead of the persisted height block downloads may reach.
pub const BLOCK_INDEX_LOOKAHEAD: u32 = MAX_BLOCK_INDEX_BATCH * BLOCK_INDEX_WINDOW_MULTIPLIER;
/// Sentinel hash representing the header-download task (never a real inventory).
pub const HEADER_TASK_HASH: UInt256 = UInt256::zero();

/// Counts how many peers currently share each outstanding task key.
///
/// Enforces [`MAX_CONCURRENT_TASKS`] per key and removes keys whose count drops
/// to zero so the map stays bounded by the number of live tasks.
#[derive(Debug)]
pub struct TaskCounter<K> {
    counts: HashMap<K, u32>,
}

impl<K> TaskCounter<K>
where
    K: Eq + Hash,
{
    /// Creates a counter with pre-allocated capacity.
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            counts: HashMap::with_capacity(capacity),
        }
    }

    /// Increments the count for `key`, returning `false` when it is already at
    /// [`MAX_CONCURRENT_TASKS`].
    pub fn try_increment(&mut self, key: K) -> bool {
        let entry = self.counts.entry(key).or_insert(0);
        if *entry >= MAX_CONCURRENT_TASKS {
            return false;
        }
        *entry += 1;
        true
    }

    /// Decrements the count for `key`, removing it when it reaches zero.
    pub fn decrement(&mut self, key: &K) {
        if let Some(entry) = self.counts.get_mut(key) {
            if *entry > 1 {
                *entry -= 1;
            } else {
                self.counts.remove(key);
            }
        }
    }

    /// Current count for `key` (`0` when untracked).
    pub fn count(&self, key: &K) -> u32 {
        self.counts.get(key).copied().unwrap_or(0)
    }

    /// Whether `key` has a non-zero count.
    pub fn is_tracked(&self, key: &K) -> bool {
        self.count(key) > 0
    }

    /// Whether `key` can accept another concurrent task.
    pub fn has_capacity_for(&self, key: &K) -> bool {
        self.count(key) < MAX_CONCURRENT_TASKS
    }

    /// Whether no keys are tracked.
    pub fn is_empty(&self) -> bool {
        self.counts.is_empty()
    }

    /// Number of tracked keys.
    pub fn len(&self) -> usize {
        self.counts.len()
    }

    /// Retains only the keys satisfying `keep`.
    pub fn retain_tracked<F>(&mut self, mut keep: F)
    where
        F: FnMut(&K) -> bool,
    {
        self.counts.retain(|key, _| keep(key));
    }
}

/// A planned `GetBlockByIndex` request window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlockIndexRequestPlan {
    /// First height to request.
    pub start_height: u32,
    /// Number of sequential heights to request.
    pub count: u32,
}

/// A planned `GetHeaders` request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HeaderRequestPlan {
    /// First header index to request.
    pub start_index: u32,
}

/// Result of planning the globally-available inventory tasks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AvailableInventoryPlan {
    /// Hashes that are stale and should be dropped.
    pub stale: Vec<UInt256>,
    /// Hashes that can be scheduled (global capacity available).
    pub scheduled: Vec<UInt256>,
}

/// Exclusive upper bound of the block-index window for a given height.
///
/// `current + 1 + lookahead` keeps exactly `lookahead` indexes in the window.
pub fn block_index_window_limit(current_height: u32) -> u32 {
    current_height
        .saturating_add(1)
        .saturating_add(MAX_BLOCK_INDEX_BATCH.saturating_mul(BLOCK_INDEX_WINDOW_MULTIPLIER))
}

/// Plans a `GetBlockByIndex` request, skipping heights already in flight and
/// stopping before the next gap, the peer height, or the window limit.
pub fn plan_block_index_request(
    current_height: u32,
    peer_height: u32,
    global_index_tasks: &TaskCounter<u32>,
) -> Option<BlockIndexRequestPlan> {
    if current_height >= peer_height {
        return None;
    }

    let mut start_height = current_height.saturating_add(1);
    while global_index_tasks.is_tracked(&start_height) {
        start_height = start_height.saturating_add(1);
        if start_height > peer_height {
            break;
        }
    }

    let limit_height = block_index_window_limit(current_height);
    if start_height > peer_height || start_height >= limit_height {
        return None;
    }

    let mut end_height = start_height;
    while end_height < peer_height
        && end_height + 1 < limit_height
        && !global_index_tasks.is_tracked(&(end_height + 1))
    {
        end_height += 1;
    }

    let count = (end_height - start_height + 1).min(BLOCK_INDEX_REQUEST_BATCH);
    Some(BlockIndexRequestPlan {
        start_height,
        count,
    })
}

/// Combines the in-memory header cache tip with the ledger's highest header
/// index, taking the highest known source.
pub fn effective_header_height(header_cache_last: Option<u32>, ledger_highest: u32) -> u32 {
    header_cache_last.unwrap_or(ledger_highest).max(ledger_highest)
}

/// Plans a `GetHeaders` request when the peer is ahead, the global header task
/// budget remains, and the peer allows a retry.
pub fn plan_header_request(
    header_height: u32,
    peer_height: u32,
    header_task_count: u32,
    peer_allows_retry: bool,
) -> Option<HeaderRequestPlan> {
    if header_height >= peer_height
        || header_task_count >= MAX_CONCURRENT_TASKS
        || !peer_allows_retry
    {
        return None;
    }

    Some(HeaderRequestPlan {
        start_index: header_height.saturating_add(1),
    })
}

/// Splits available inventory hashes into stale (dropped) and schedulable sets.
pub fn plan_available_inventory_tasks<I, F>(
    available_tasks: I,
    mut is_stale: F,
    global_inv_tasks: &TaskCounter<UInt256>,
) -> AvailableInventoryPlan
where
    I: IntoIterator<Item = UInt256>,
    F: FnMut(&UInt256) -> bool,
{
    let mut stale = Vec::new();
    let mut scheduled = Vec::new();

    for hash in available_tasks {
        if is_stale(&hash) {
            stale.push(hash);
            continue;
        }

        if global_inv_tasks.has_capacity_for(&hash) {
            scheduled.push(hash);
        }
    }

    AvailableInventoryPlan { stale, scheduled }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn task_counter_limits_and_removes_zero_entries() {
        let mut tasks = TaskCounter::with_capacity(4);
        let key = 42u32;

        assert!(tasks.try_increment(key));
        assert!(tasks.try_increment(key));
        assert!(tasks.try_increment(key));
        assert_eq!(tasks.count(&key), MAX_CONCURRENT_TASKS);
        assert!(!tasks.try_increment(key));

        tasks.decrement(&key);
        assert_eq!(tasks.count(&key), MAX_CONCURRENT_TASKS - 1);
        assert!(tasks.try_increment(key));
        assert_eq!(tasks.count(&key), MAX_CONCURRENT_TASKS);

        tasks.decrement(&key);
        tasks.decrement(&key);
        tasks.decrement(&key);
        assert!(!tasks.is_tracked(&key));

        tasks.decrement(&key);
        assert!(tasks.is_empty());
    }

    #[test]
    fn block_index_plan_skips_in_flight_heights_and_stops_before_next_gap() {
        let mut in_flight = TaskCounter::with_capacity(4);
        assert!(in_flight.try_increment(6));
        assert!(in_flight.try_increment(7));
        assert!(in_flight.try_increment(10));

        let plan = plan_block_index_request(5, 12, &in_flight).expect("block request plan");

        assert_eq!(plan.start_height, 8);
        assert_eq!(plan.count, 2);
    }

    #[test]
    fn block_index_plan_respects_peer_and_protocol_batch_limits() {
        let in_flight = TaskCounter::with_capacity(0);

        let plan = plan_block_index_request(0, 20_000, &in_flight).expect("block request plan");

        assert_eq!(plan.start_height, 1);
        assert_eq!(plan.count, BLOCK_INDEX_REQUEST_BATCH);
        assert!(plan.count <= MAX_BLOCK_INDEX_BATCH);
        assert!(plan.count <= MAX_HASHES_COUNT as u32);
    }

    #[test]
    fn block_index_plan_leaves_following_range_for_another_peer() {
        let mut in_flight = TaskCounter::with_capacity(BLOCK_INDEX_REQUEST_BATCH as usize);
        let first = plan_block_index_request(0, 20_000, &in_flight).expect("first request plan");
        for index in first.start_height..first.start_height + first.count {
            assert!(in_flight.try_increment(index));
        }

        let second = plan_block_index_request(0, 20_000, &in_flight).expect("second request plan");

        assert_eq!(first.start_height, 1);
        assert_eq!(second.start_height, first.start_height + first.count);
        assert_eq!(second.count, BLOCK_INDEX_REQUEST_BATCH);
    }

    #[test]
    fn block_index_plan_returns_none_when_window_is_already_full() {
        let mut in_flight = TaskCounter::with_capacity(10_000);
        for index in 1..10_000 {
            assert!(in_flight.try_increment(index));
        }

        assert!(plan_block_index_request(0, 20_000, &in_flight).is_none());
    }

    #[test]
    fn effective_header_height_uses_highest_known_source() {
        assert_eq!(effective_header_height(None, 7), 7);
        assert_eq!(effective_header_height(Some(3), 7), 7);
        assert_eq!(effective_header_height(Some(11), 7), 11);
    }

    #[test]
    fn header_request_plan_requires_peer_ahead_retry_and_global_capacity() {
        assert_eq!(
            plan_header_request(7, 9, 0, true).map(|plan| plan.start_index),
            Some(8)
        );
        assert_eq!(
            plan_header_request(7, 9, MAX_CONCURRENT_TASKS - 1, true).map(|p| p.start_index),
            Some(8)
        );
        assert!(plan_header_request(7, 7, 0, true).is_none());
        assert!(plan_header_request(7, 9, MAX_CONCURRENT_TASKS, true).is_none());
        assert!(plan_header_request(7, 9, 0, false).is_none());
    }

    #[test]
    fn available_inventory_plan_removes_stale_and_schedules_under_capacity_hashes() {
        let a = UInt256::from([1u8; 32]);
        let stale = UInt256::from([2u8; 32]);
        let saturated = UInt256::from([3u8; 32]);
        let shared = UInt256::from([4u8; 32]);
        let mut global_inv_tasks = TaskCounter::with_capacity(4);
        for _ in 0..MAX_CONCURRENT_TASKS {
            assert!(global_inv_tasks.try_increment(saturated));
        }
        for _ in 0..MAX_CONCURRENT_TASKS - 1 {
            assert!(global_inv_tasks.try_increment(shared));
        }

        let plan = plan_available_inventory_tasks(
            [a, stale, saturated, shared],
            |hash| *hash == stale,
            &global_inv_tasks,
        );

        assert_eq!(
            plan.stale.into_iter().collect::<HashSet<_>>(),
            HashSet::from([stale])
        );
        assert_eq!(
            plan.scheduled.into_iter().collect::<HashSet<_>>(),
            HashSet::from([a, shared])
        );
    }
}
