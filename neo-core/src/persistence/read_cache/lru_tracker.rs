use lru::LruCache;
use std::hash::Hash;

/// Tracks cache access order while leaving value storage and eviction accounting to `ReadCache`.
pub(super) struct LruTracker<K> {
    order: LruCache<K, ()>,
}

impl<K: Clone + Eq + Hash> LruTracker<K> {
    pub(super) fn new() -> Self {
        Self {
            order: LruCache::unbounded(),
        }
    }

    /// Records an access and moves the key to the most-recently-used position.
    pub(super) fn record_access(&mut self, key: K) -> bool {
        self.order.put(key, ()).is_some()
    }

    /// Removes a key from tracking.
    pub(super) fn remove(&mut self, key: &K) -> bool {
        self.order.pop(key).is_some()
    }

    /// Returns the least recently used key without updating access order.
    pub(super) fn find_lru(&self) -> Option<K> {
        self.order.peek_lru().map(|(key, _)| key.clone())
    }

    /// Clears all tracking state.
    pub(super) fn clear(&mut self) {
        self.order.clear();
    }

    #[allow(dead_code)]
    pub(super) fn len(&self) -> usize {
        self.order.len()
    }
}
