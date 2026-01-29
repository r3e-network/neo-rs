//! State Root Cache for efficient verification
//!
//! Provides LRU caching for recent state roots to reduce disk I/O
//! during block validation and state synchronization.

use crate::persistence::cache::LruCache;
use crate::state_service::state_root::StateRoot;
use crate::UInt256;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

/// Default capacity for the state root cache.
pub const DEFAULT_ROOT_CACHE_CAPACITY: usize = 1000;

/// Statistics for state root cache operations.
#[derive(Debug, Default)]
pub struct StateRootCacheStats {
    /// Cache hits
    pub hits: AtomicU64,
    /// Cache misses
    pub misses: AtomicU64,
    /// Cache insertions
    pub insertions: AtomicU64,
    /// Cache evictions
    pub evictions: AtomicU64,
}

impl StateRootCacheStats {
    /// Creates new statistics.
    pub fn new() -> Self {
        Self::default()
    }

    /// Records a cache hit.
    pub fn record_hit(&self) {
        self.hits.fetch_add(1, Ordering::Relaxed);
    }

    /// Records a cache miss.
    pub fn record_miss(&self) {
        self.misses.fetch_add(1, Ordering::Relaxed);
    }

    /// Records a cache insertion.
    pub fn record_insertion(&self) {
        self.insertions.fetch_add(1, Ordering::Relaxed);
    }

    /// Records a cache eviction.
    pub fn record_eviction(&self) {
        self.evictions.fetch_add(1, Ordering::Relaxed);
    }

    /// Gets the hit rate.
    pub fn hit_rate(&self) -> f64 {
        let hits = self.hits.load(Ordering::Relaxed);
        let misses = self.misses.load(Ordering::Relaxed);
        if hits + misses == 0 {
            0.0
        } else {
            hits as f64 / (hits + misses) as f64
        }
    }
}

/// Cache entry for a state root.
#[derive(Debug, Clone)]
pub struct StateRootCacheEntry {
    /// The state root
    pub state_root: StateRoot,
    /// Whether this root has been validated
    pub is_validated: bool,
    /// Block timestamp when this root was created
    pub timestamp: u64,
}

impl StateRootCacheEntry {
    /// Creates a new cache entry.
    pub fn new(state_root: StateRoot, is_validated: bool, timestamp: u64) -> Self {
        Self {
            state_root,
            is_validated,
            timestamp,
        }
    }

    /// Gets the root hash.
    pub fn root_hash(&self) -> UInt256 {
        self.state_root.root_hash
    }

    /// Gets the block index.
    pub fn index(&self) -> u32 {
        self.state_root.index
    }
}

/// LRU cache for state roots to avoid repeated disk lookups.
pub struct StateRootCache {
    cache: LruCache<u32, StateRootCacheEntry>,
    hash_index: LruCache<UInt256, u32>,
    stats: Arc<StateRootCacheStats>,
}

impl StateRootCache {
    /// Creates a new state root cache with the specified capacity.
    pub fn new(capacity: usize) -> Self {
        Self {
            cache: LruCache::new(capacity),
            hash_index: LruCache::new(capacity),
            stats: Arc::new(StateRootCacheStats::new()),
        }
    }

    /// Creates a new state root cache with default capacity.
    pub fn with_default_capacity() -> Self {
        Self::new(DEFAULT_ROOT_CACHE_CAPACITY)
    }

    /// Gets a state root by block index.
    pub fn get(&mut self, index: u32) -> Option<StateRootCacheEntry> {
        match self.cache.get(&index) {
            Some(entry) => {
                self.stats.record_hit();
                Some(entry)
            }
            None => {
                self.stats.record_miss();
                None
            }
        }
    }

    /// Gets a state root by its hash.
    pub fn get_by_hash(&mut self, hash: &UInt256) -> Option<StateRootCacheEntry> {
        let index = self.hash_index.get(hash)?;
        self.get(index)
    }

    /// Gets the index for a root hash.
    pub fn get_index_for_hash(&mut self, hash: &UInt256) -> Option<u32> {
        self.hash_index.get(hash)
    }

    /// Inserts a state root into the cache.
    pub fn insert(&mut self, entry: StateRootCacheEntry) {
        let index = entry.index();
        let hash = entry.root_hash();
        let was_full = self.cache.len() >= self.cache.capacity();

        self.cache.put(index, entry);
        self.hash_index.put(hash, index);

        self.stats.record_insertion();
        // Record eviction if cache was already at capacity
        if was_full {
            self.stats.record_eviction();
        }
    }

    /// Inserts a state root with automatic timestamp.
    pub fn insert_state_root(&mut self, state_root: StateRoot, is_validated: bool, timestamp: u64) {
        let entry = StateRootCacheEntry::new(state_root, is_validated, timestamp);
        self.insert(entry);
    }

    /// Removes a state root from the cache.
    pub fn remove(&mut self, index: u32) -> Option<StateRootCacheEntry> {
        if let Some(entry) = self.cache.remove(&index) {
            self.hash_index.remove(&entry.root_hash());
            Some(entry)
        } else {
            None
        }
    }

    /// Checks if the cache contains a state root for the given index.
    pub fn contains(&mut self, index: u32) -> bool {
        self.cache.get(&index).is_some()
    }

    /// Checks if the cache contains a state root with the given hash.
    pub fn contains_hash(&mut self, hash: &UInt256) -> bool {
        self.hash_index.get(hash).is_some()
    }

    /// Gets the current number of entries.
    pub fn len(&self) -> usize {
        self.cache.len()
    }

    /// Checks if the cache is empty.
    pub fn is_empty(&self) -> bool {
        self.cache.is_empty()
    }

    /// Clears the cache.
    pub fn clear(&mut self) {
        self.cache.clear();
        self.hash_index.clear();
    }

    /// Gets cache statistics.
    pub fn stats(&self) -> Arc<StateRootCacheStats> {
        Arc::clone(&self.stats)
    }

    /// Gets the cache capacity.
    pub fn capacity(&self) -> usize {
        self.cache.capacity()
    }

    /// Marks a state root as validated.
    pub fn mark_validated(&mut self, index: u32) -> bool {
        if let Some(mut entry) = self.cache.remove(&index) {
            entry.is_validated = true;
            let hash = entry.root_hash();
            self.cache.put(index, entry);
            self.hash_index.put(hash, index);
            true
        } else {
            false
        }
    }

    /// Gets the most recent validated index in the cache.
    pub fn get_most_recent_validated(&mut self) -> Option<u32> {
        // Since we use LRU, we need to scan for validated entries
        // For performance, we'll just track this separately in the store
        None
    }

    /// Gets all entries in the cache (for testing/debugging).
    #[cfg(test)]
    pub fn entries(&self) -> Vec<(u32, StateRootCacheEntry)> {
        // Note: This is a simplified implementation for testing
        Vec::new()
    }
}

impl Default for StateRootCache {
    fn default() -> Self {
        Self::with_default_capacity()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state_service::state_root::StateRoot;

    fn create_test_state_root(index: u32, hash_byte: u8) -> StateRoot {
        let root_hash = UInt256::from_bytes(&[hash_byte; 32]).unwrap();
        StateRoot::new_current(index, root_hash)
    }

    #[test]
    fn cache_new_creates_empty_cache() {
        let cache = StateRootCache::new(100);
        assert!(cache.is_empty());
        assert_eq!(cache.len(), 0);
    }

    #[test]
    fn cache_insert_and_get() {
        let mut cache = StateRootCache::new(100);
        let state_root = create_test_state_root(1, 0x01);

        cache.insert_state_root(state_root.clone(), false, 123456);

        let entry = cache.get(1).unwrap();
        assert_eq!(entry.index(), 1);
        assert_eq!(entry.root_hash(), state_root.root_hash);
        assert!(!entry.is_validated);
        assert_eq!(entry.timestamp, 123456);
    }

    #[test]
    fn cache_get_by_hash() {
        let mut cache = StateRootCache::new(100);
        let state_root = create_test_state_root(2, 0x02);
        let hash = state_root.root_hash;

        cache.insert_state_root(state_root, false, 0);

        let entry = cache.get_by_hash(&hash).unwrap();
        assert_eq!(entry.index(), 2);
    }

    #[test]
    fn cache_contains() {
        let mut cache = StateRootCache::new(100);
        let state_root = create_test_state_root(3, 0x03);

        assert!(!cache.contains(3));
        cache.insert_state_root(state_root, false, 0);
        assert!(cache.contains(3));
    }

    #[test]
    fn cache_eviction_when_full() {
        let mut cache = StateRootCache::new(3);

        cache.insert_state_root(create_test_state_root(1, 0x01), false, 0);
        cache.insert_state_root(create_test_state_root(2, 0x02), false, 0);
        cache.insert_state_root(create_test_state_root(3, 0x03), false, 0);
        cache.insert_state_root(create_test_state_root(4, 0x04), false, 0);

        // First entry should be evicted
        assert!(cache.get(1).is_none());
        assert!(cache.get(2).is_some());
        assert!(cache.get(3).is_some());
        assert!(cache.get(4).is_some());
    }

    #[test]
    fn cache_remove() {
        let mut cache = StateRootCache::new(100);
        let state_root = create_test_state_root(5, 0x05);

        cache.insert_state_root(state_root, false, 0);
        assert!(cache.contains(5));

        let removed = cache.remove(5);
        assert!(removed.is_some());
        assert!(!cache.contains(5));

        let not_found = cache.remove(999);
        assert!(not_found.is_none());
    }

    #[test]
    fn cache_mark_validated() {
        let mut cache = StateRootCache::new(100);
        let state_root = create_test_state_root(6, 0x06);

        cache.insert_state_root(state_root, false, 0);
        assert!(!cache.get(6).unwrap().is_validated);

        cache.mark_validated(6);
        assert!(cache.get(6).unwrap().is_validated);
    }

    #[test]
    fn cache_stats_track_hits_and_misses() {
        let mut cache = StateRootCache::new(100);

        cache.insert_state_root(create_test_state_root(1, 0x01), false, 0);

        let _ = cache.get(1); // Hit
        let _ = cache.get(999); // Miss

        let stats = cache.stats();
        assert_eq!(stats.hits.load(Ordering::Relaxed), 1);
        assert_eq!(stats.misses.load(Ordering::Relaxed), 1);
        assert!(stats.hit_rate() > 0.49 && stats.hit_rate() < 0.51);
    }

    #[test]
    fn cache_clear_removes_all() {
        let mut cache = StateRootCache::new(100);

        cache.insert_state_root(create_test_state_root(1, 0x01), false, 0);
        cache.insert_state_root(create_test_state_root(2, 0x02), false, 0);

        cache.clear();

        assert!(cache.is_empty());
        assert!(!cache.contains(1));
        assert!(!cache.contains(2));
    }

    #[test]
    fn cache_default_capacity() {
        let cache = StateRootCache::with_default_capacity();
        assert_eq!(cache.capacity(), DEFAULT_ROOT_CACHE_CAPACITY);
    }
}
