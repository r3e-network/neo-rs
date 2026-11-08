//! Tracks recently announced hashes to avoid duplicate inventory processing.
//!
//! Mirrors the `PendingKnownHashes` from `neo-core::network::p2p::remote_node`.
//! Uses a `VecDeque` for FIFO ordering and a `HashSet` for O(1) lookups,
//! avoiding the `indexmap` dependency.

use neo_primitives::UInt256;
use std::collections::{HashSet, VecDeque};
use std::time::{Duration, Instant};

/// Maximum TTL for pending hashes (60 seconds).
/// Entries older than this will be pruned on next `try_add` operation.
pub const PENDING_KNOWN_HASH_TTL: Duration = Duration::from_secs(60);

/// Entry stored in the FIFO queue.
#[derive(Debug, Clone, Copy)]
struct Entry {
    hash: UInt256,
    timestamp: Instant,
}

/// Tracks hashes announced by peers to avoid re-requesting or duplicating
/// inventories. Implements a bounded FIFO with TTL-based expiration.
#[derive(Debug)]
pub struct PendingKnownHashes {
    queue: VecDeque<Entry>,
    set: HashSet<UInt256>,
    capacity: usize,
}

impl PendingKnownHashes {
    /// Creates a new cache with the given maximum capacity.
    pub fn new(capacity: usize) -> Self {
        Self {
            queue: VecDeque::with_capacity(capacity.max(1)),
            set: HashSet::with_capacity(capacity.max(1)),
            capacity: capacity.max(1),
        }
    }

    /// Returns `true` if the hash is currently tracked.
    pub fn contains(&self, hash: &UInt256) -> bool {
        self.set.contains(hash)
    }

    /// Attempts to add a hash to the pending set.
    ///
    /// Automatically prunes stale entries and enforces capacity limits.
    /// Returns `true` if the hash was newly inserted, `false` if already present.
    pub fn try_add(&mut self, hash: UInt256, timestamp: Instant) -> bool {
        // Auto-prune entries older than TTL.
        if let Some(cutoff) = timestamp.checked_sub(PENDING_KNOWN_HASH_TTL) {
            self.prune_older_than(cutoff);
        }

        // If at capacity after pruning, remove oldest entry to make room.
        if self.queue.len() >= self.capacity {
            self.evict_oldest();
        }

        if self.set.contains(&hash) {
            return false;
        }

        self.queue.push_back(Entry { hash, timestamp });
        self.set.insert(hash);
        true
    }

    /// Removes a specific hash from the pending set.
    pub fn remove(&mut self, hash: &UInt256) -> bool {
        if !self.set.remove(hash) {
            return false;
        }
        // Remove from queue (linear scan; acceptable for moderate capacity).
        if let Some(pos) = self.queue.iter().position(|e| e.hash == *hash) {
            self.queue.remove(pos);
        }
        true
    }

    /// Clears all tracked hashes.
    pub fn clear(&mut self) {
        self.queue.clear();
        self.set.clear();
    }

    /// Prunes entries older than the given cutoff timestamp.
    /// Returns the number of entries removed.
    pub fn prune_older_than(&mut self, cutoff: Instant) -> usize {
        let mut removed = 0;
        while let Some(front) = self.queue.front() {
            if front.timestamp >= cutoff {
                break;
            }
            let entry = self.queue.pop_front().unwrap();
            self.set.remove(&entry.hash);
            removed += 1;
        }
        removed
    }

    /// Returns the number of tracked hashes.
    pub fn len(&self) -> usize {
        self.set.len()
    }

    /// Returns `true` if no hashes are tracked.
    pub fn is_empty(&self) -> bool {
        self.set.is_empty()
    }

    fn evict_oldest(&mut self) {
        if let Some(entry) = self.queue.pop_front() {
            self.set.remove(&entry.hash);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_hash(byte: u8) -> UInt256 {
        let mut data = [0u8; 32];
        data[0] = byte;
        UInt256::from(data)
    }

    #[test]
    fn prune_older_than_removes_stale_front_entries() {
        let now = Instant::now();
        let mut cache = PendingKnownHashes::new(4);

        cache.try_add(make_hash(1), now - Duration::from_secs(55));
        cache.try_add(make_hash(2), now - Duration::from_secs(5));

        let removed = cache.prune_older_than(now - Duration::from_secs(30));
        assert_eq!(removed, 1);
        assert!(!cache.contains(&make_hash(1)));
        assert!(cache.contains(&make_hash(2)));
    }

    #[test]
    fn try_add_evicts_oldest_when_full() {
        let now = Instant::now();
        let mut cache = PendingKnownHashes::new(2);

        assert!(cache.try_add(make_hash(1), now));
        assert!(cache.try_add(make_hash(2), now));
        assert!(cache.try_add(make_hash(3), now));

        assert!(!cache.contains(&make_hash(1)));
        assert!(cache.contains(&make_hash(2)));
        assert!(cache.contains(&make_hash(3)));
    }

    #[test]
    fn duplicate_add_returns_false() {
        let now = Instant::now();
        let mut cache = PendingKnownHashes::new(4);
        let hash = make_hash(1);

        assert!(cache.try_add(hash, now));
        assert!(!cache.try_add(hash, now + Duration::from_secs(1)));
        assert_eq!(cache.len(), 1);
    }

    #[test]
    fn remove_keeps_remaining_entries() {
        let now = Instant::now();
        let mut cache = PendingKnownHashes::new(4);

        cache.try_add(make_hash(1), now);
        cache.try_add(make_hash(2), now);
        cache.try_add(make_hash(3), now);

        assert!(cache.remove(&make_hash(2)));
        assert!(!cache.contains(&make_hash(2)));
        assert!(cache.contains(&make_hash(1)));
        assert!(cache.contains(&make_hash(3)));
        assert_eq!(cache.len(), 2);
    }

    #[test]
    fn remove_nonexistent_returns_false() {
        let mut cache = PendingKnownHashes::new(4);
        assert!(!cache.remove(&make_hash(99)));
    }

    #[test]
    fn clear_removes_all_hashes() {
        let now = Instant::now();
        let mut cache = PendingKnownHashes::new(4);

        cache.try_add(make_hash(1), now);
        cache.try_add(make_hash(2), now);
        cache.clear();

        assert!(!cache.contains(&make_hash(1)));
        assert!(!cache.contains(&make_hash(2)));
        assert!(cache.is_empty());
    }

    #[test]
    fn fifo_order_is_preserved_after_remove() {
        let now = Instant::now();
        let mut cache = PendingKnownHashes::new(4);

        cache.try_add(make_hash(1), now);
        cache.try_add(make_hash(2), now);
        cache.try_add(make_hash(3), now);
        cache.remove(&make_hash(2));

        // After removing hash(2), the queue is [1, 3].
        // Adding hash(4) with capacity 4 should not evict.
        cache.try_add(make_hash(4), now);
        assert!(cache.contains(&make_hash(1)));
        assert!(cache.contains(&make_hash(3)));
        assert!(cache.contains(&make_hash(4)));
        assert_eq!(cache.len(), 3);
    }

    #[test]
    fn capacity_one_behaves_as_single_slot() {
        let now = Instant::now();
        let mut cache = PendingKnownHashes::new(1);

        assert!(cache.try_add(make_hash(1), now));
        assert!(cache.try_add(make_hash(2), now));

        assert!(!cache.contains(&make_hash(1)));
        assert!(cache.contains(&make_hash(2)));
        assert_eq!(cache.len(), 1);
    }
}
