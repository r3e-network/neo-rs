use hashbrown::HashMap;
use std::hash::Hash;
use std::sync::atomic::{AtomicU64, Ordering};

/// Optimized LRU tracking using a linked list approach with index map.
/// This provides O(1) LRU operations instead of O(n) with a Vec.
pub(super) struct LruTracker<K> {
    /// Map from key to its position/index in the access order
    order: HashMap<K, u64>,
    /// Counter for generating unique sequence numbers
    sequence: AtomicU64,
}

impl<K: Clone + Eq + Hash> LruTracker<K> {
    pub(super) fn new() -> Self {
        Self {
            order: HashMap::new(),
            sequence: AtomicU64::new(0),
        }
    }

    /// Record access and return the old sequence number if any
    pub(super) fn record_access(&mut self, key: K) -> Option<u64> {
        let new_seq = self.sequence.fetch_add(1, Ordering::Relaxed);
        self.order.insert(key, new_seq)
    }

    /// Remove a key from tracking
    pub(super) fn remove(&mut self, key: &K) -> Option<u64> {
        self.order.remove(key)
    }

    /// Find the least recently used key
    pub(super) fn find_lru(&self) -> Option<K> {
        self.order
            .iter()
            .min_by_key(|(_, seq)| *seq)
            .map(|(k, _)| k.clone())
    }

    /// Clear all tracking
    pub(super) fn clear(&mut self) {
        self.order.clear();
        self.sequence.store(0, Ordering::Relaxed);
    }

    #[allow(dead_code)]
    pub(super) fn len(&self) -> usize {
        self.order.len()
    }
}
