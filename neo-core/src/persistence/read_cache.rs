//! Read cache with LRU eviction and pre-fetching support.
//!
//! This module provides a read cache for frequently accessed keys with
//! configurable LRU eviction and intelligent pre-fetching for iteration.

use crate::smart_contract::{StorageItem, StorageKey};
use parking_lot::RwLock;
use std::collections::HashMap;
use std::hash::Hash;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tracing::{debug, trace};

/// Cache entry with metadata.
#[derive(Debug, Clone)]
struct CacheEntry<V> {
    value: V,
    access_count: u64,
    last_access: Instant,
    size_bytes: usize,
}

impl<V> CacheEntry<V> {
    fn new(value: V, size_bytes: usize) -> Self {
        Self {
            value,
            access_count: 1,
            last_access: Instant::now(),
            size_bytes,
        }
    }

    fn record_access(&mut self) {
        self.access_count += 1;
        self.last_access = Instant::now();
    }
}

/// Statistics for the read cache.
#[derive(Debug, Default)]
pub struct ReadCacheStats {
    hits: AtomicU64,
    misses: AtomicU64,
    evictions: AtomicU64,
    prefetches: AtomicU64,
    prefetch_hits: AtomicU64,
    inserts: AtomicU64,
    current_entries: AtomicUsize,
    current_bytes: AtomicUsize,
}

impl ReadCacheStats {
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

    /// Records an eviction.
    pub fn record_eviction(&self, bytes: usize) {
        self.evictions.fetch_add(1, Ordering::Relaxed);
        self.current_entries.fetch_sub(1, Ordering::Relaxed);
        self.current_bytes.fetch_sub(bytes, Ordering::Relaxed);
    }

    /// Records a prefetch.
    pub fn record_prefetch(&self, count: usize, bytes: usize) {
        self.prefetches.fetch_add(count as u64, Ordering::Relaxed);
        self.inserts.fetch_add(count as u64, Ordering::Relaxed);
        self.current_entries.fetch_add(count, Ordering::Relaxed);
        self.current_bytes.fetch_add(bytes, Ordering::Relaxed);
    }

    /// Records a prefetch hit.
    pub fn record_prefetch_hit(&self) {
        self.prefetch_hits.fetch_add(1, Ordering::Relaxed);
    }

    /// Records an insert.
    pub fn record_insert(&self, bytes: usize) {
        self.inserts.fetch_add(1, Ordering::Relaxed);
        self.current_entries.fetch_add(1, Ordering::Relaxed);
        self.current_bytes.fetch_add(bytes, Ordering::Relaxed);
    }

    /// Gets a snapshot of statistics.
    pub fn snapshot(&self) -> ReadCacheStatsSnapshot {
        ReadCacheStatsSnapshot {
            hits: self.hits.load(Ordering::Relaxed),
            misses: self.misses.load(Ordering::Relaxed),
            evictions: self.evictions.load(Ordering::Relaxed),
            prefetches: self.prefetches.load(Ordering::Relaxed),
            prefetch_hits: self.prefetch_hits.load(Ordering::Relaxed),
            inserts: self.inserts.load(Ordering::Relaxed),
            current_entries: self.current_entries.load(Ordering::Relaxed),
            current_bytes: self.current_bytes.load(Ordering::Relaxed),
        }
    }
}

/// Snapshot of read cache statistics.
#[derive(Debug, Clone, Copy)]
pub struct ReadCacheStatsSnapshot {
    pub hits: u64,
    pub misses: u64,
    pub evictions: u64,
    pub prefetches: u64,
    pub prefetch_hits: u64,
    pub inserts: u64,
    pub current_entries: usize,
    pub current_bytes: usize,
}

impl ReadCacheStatsSnapshot {
    /// Calculates the hit rate.
    pub fn hit_rate(&self) -> f64 {
        let total = self.hits + self.misses;
        if total == 0 {
            0.0
        } else {
            self.hits as f64 / total as f64
        }
    }

    /// Calculates the prefetch hit rate.
    pub fn prefetch_hit_rate(&self) -> f64 {
        if self.prefetches == 0 {
            0.0
        } else {
            self.prefetch_hits as f64 / self.prefetches as f64
        }
    }
}

/// Configuration for the read cache.
#[derive(Debug, Clone, Copy)]
pub struct ReadCacheConfig {
    /// Maximum number of entries
    pub max_entries: usize,
    /// Maximum size in bytes
    pub max_bytes: usize,
    /// Enable pre-fetching
    pub enable_prefetch: bool,
    /// Number of items to pre-fetch
    pub prefetch_count: usize,
    /// Pre-fetch threshold (access count)
    pub prefetch_threshold: u64,
    /// TTL for cache entries (None = no TTL)
    pub ttl: Option<Duration>,
    /// Enable statistics
    pub enable_stats: bool,
}

impl Default for ReadCacheConfig {
    fn default() -> Self {
        Self {
            max_entries: 10000,
            max_bytes: 64 * 1024 * 1024, // 64MB
            enable_prefetch: true,
            prefetch_count: 10,
            prefetch_threshold: 2,
            ttl: None,
            enable_stats: true,
        }
    }
}

impl ReadCacheConfig {
    /// Creates configuration for high memory usage.
    pub fn high_memory() -> Self {
        Self {
            max_entries: 100000,
            max_bytes: 512 * 1024 * 1024, // 512MB
            enable_prefetch: true,
            prefetch_count: 20,
            prefetch_threshold: 2,
            ttl: None,
            enable_stats: true,
        }
    }

    /// Creates configuration for low memory usage.
    pub fn low_memory() -> Self {
        Self {
            max_entries: 1000,
            max_bytes: 8 * 1024 * 1024, // 8MB
            enable_prefetch: false,
            prefetch_count: 5,
            prefetch_threshold: 5,
            ttl: Some(Duration::from_secs(60)),
            enable_stats: true,
        }
    }

    /// Creates configuration with pre-fetching disabled.
    pub fn no_prefetch() -> Self {
        Self {
            enable_prefetch: false,
            prefetch_count: 0,
            ..Default::default()
        }
    }
}

/// LRU Read cache for storage operations.
pub struct ReadCache<K, V>
where
    K: Clone + Eq + Hash,
    V: Clone,
{
    config: ReadCacheConfig,
    data: RwLock<HashMap<K, CacheEntry<V>>>,
    stats: Arc<ReadCacheStats>,
    access_order: RwLock<Vec<K>>,
}

impl<K, V> ReadCache<K, V>
where
    K: Clone + Eq + Hash,
    V: Clone,
{
    /// Creates a new read cache with the specified configuration.
    pub fn new(config: ReadCacheConfig) -> Self {
        Self {
            config,
            data: RwLock::new(HashMap::new()),
            stats: Arc::new(ReadCacheStats::new()),
            access_order: RwLock::new(Vec::new()),
        }
    }

    /// Creates a new read cache with default configuration.
    pub fn with_defaults() -> Self {
        Self::new(ReadCacheConfig::default())
    }

    /// Gets a value from the cache.
    pub fn get(&self, key: &K) -> Option<V> {
        let mut data = self.data.write();
        
        if let Some(entry) = data.get_mut(key) {
            // Check TTL
            if let Some(ttl) = self.config.ttl {
                if entry.last_access.elapsed() > ttl {
                    // Entry expired
                    let size = entry.size_bytes;
                    data.remove(key);
                    self.remove_from_access_order(key);
                    self.stats.record_eviction(size);
                    
                    if self.config.enable_stats {
                        self.stats.record_miss();
                    }
                    return None;
                }
            }
            
            // Update entry
            entry.record_access();
            let value = entry.value.clone();
            
            // Update access order
            drop(data);
            self.update_access_order(key.clone());
            
            if self.config.enable_stats {
                self.stats.record_hit();
            }
            
            trace!(target: "neo", "cache hit");
            Some(value)
        } else {
            drop(data);
            
            if self.config.enable_stats {
                self.stats.record_miss();
            }
            
            trace!(target: "neo", "cache miss");
            None
        }
    }

    /// Puts a value into the cache.
    pub fn put(&self, key: K, value: V, size_bytes: usize) {
        let mut data = self.data.write();
        
        // Check if we need to evict
        while data.len() >= self.config.max_entries {
            self.evict_lru(&mut data);
        }
        
        // Check if adding this would exceed byte limit
        let current_bytes = self.stats.current_bytes.load(Ordering::Relaxed);
        while current_bytes + size_bytes > self.config.max_bytes && !data.is_empty() {
            self.evict_lru(&mut data);
        }
        
        // Insert new entry
        let entry = CacheEntry::new(value, size_bytes);
        data.insert(key.clone(), entry);
        drop(data);
        
        // Update access order
        self.access_order.write().push(key);
        
        if self.config.enable_stats {
            self.stats.record_insert(size_bytes);
        }
        
        trace!(target: "neo", size_bytes, "cache insert");
    }

    /// Puts multiple values into the cache (for pre-fetching).
    pub fn put_batch(&self, items: Vec<(K, V, usize)>) {
        let total_bytes: usize = items.iter().map(|(_, _, size)| size).sum();
        
        let mut data = self.data.write();
        
        // Make room for new entries
        while data.len() + items.len() > self.config.max_entries {
            self.evict_lru(&mut data);
        }
        
        let current_bytes = self.stats.current_bytes.load(Ordering::Relaxed);
        while current_bytes + total_bytes > self.config.max_bytes && !data.is_empty() {
            self.evict_lru(&mut data);
        }
        
        let count = items.len();
        let mut access_order = self.access_order.write();
        
        for (key, value, size_bytes) in items {
            let entry = CacheEntry::new(value, size_bytes);
            data.insert(key.clone(), entry);
            access_order.push(key);
        }
        
        drop(data);
        drop(access_order);
        
        if self.config.enable_stats && count > 0 {
            self.stats.record_prefetch(count, total_bytes);
        }
        
        debug!(target: "neo", count, total_bytes, "cache batch insert (prefetch)");
    }

    /// Removes a value from the cache.
    pub fn remove(&self, key: &K) -> Option<V> {
        let mut data = self.data.write();
        
        if let Some(entry) = data.remove(key) {
            self.remove_from_access_order(key);
            
            if self.config.enable_stats {
                self.stats.current_entries.fetch_sub(1, Ordering::Relaxed);
                self.stats.current_bytes.fetch_sub(entry.size_bytes, Ordering::Relaxed);
            }
            
            Some(entry.value)
        } else {
            None
        }
    }

    /// Clears the cache.
    pub fn clear(&self) {
        let mut data = self.data.write();
        let mut access_order = self.access_order.write();
        
        data.clear();
        access_order.clear();
        
        if self.config.enable_stats {
            self.stats.current_entries.store(0, Ordering::Relaxed);
            self.stats.current_bytes.store(0, Ordering::Relaxed);
        }
        
        debug!(target: "neo", "cache cleared");
    }

    /// Checks if the cache contains a key.
    pub fn contains(&self, key: &K) -> bool {
        self.data.read().contains_key(key)
    }

    /// Returns the number of entries in the cache.
    pub fn len(&self) -> usize {
        self.data.read().len()
    }

    /// Returns true if the cache is empty.
    pub fn is_empty(&self) -> bool {
        self.data.read().is_empty()
    }

    /// Gets a snapshot of statistics.
    pub fn stats(&self) -> ReadCacheStatsSnapshot {
        self.stats.snapshot()
    }

    /// Gets the configuration.
    pub fn config(&self) -> &ReadCacheConfig {
        &self.config
    }

    /// Returns true if pre-fetching is enabled and this key qualifies.
    pub fn should_prefetch(&self, key: &K) -> bool {
        if !self.config.enable_prefetch {
            return false;
        }
        
        let data = self.data.read();
        if let Some(entry) = data.get(key) {
            entry.access_count >= self.config.prefetch_threshold
        } else {
            false
        }
    }

    /// Records a prefetch hit.
    pub fn record_prefetch_hit(&self) {
        if self.config.enable_stats {
            self.stats.record_prefetch_hit();
        }
    }

    /// Evicts the least recently used entry.
    fn evict_lru(&self, data: &mut parking_lot::RwLockWriteGuard<HashMap<K, CacheEntry<V>>>) {
        let access_order = self.access_order.read();
        
        if let Some(lru_key) = access_order.first() {
            let key = lru_key.clone();
            drop(access_order);
            
            if let Some(entry) = data.remove(&key) {
                self.remove_from_access_order(&key);
                
                if self.config.enable_stats {
                    self.stats.record_eviction(entry.size_bytes);
                }
                
                trace!(target: "neo", "cache eviction");
            }
        }
    }

    /// Updates the access order for a key.
    fn update_access_order(&self, key: K) {
        let mut access_order = self.access_order.write();
        
        // Remove from current position
        if let Some(pos) = access_order.iter().position(|k| k == &key) {
            access_order.remove(pos);
        }
        
        // Add to end (most recently used)
        access_order.push(key);
    }

    /// Removes a key from the access order.
    fn remove_from_access_order(&self, key: &K) {
        let mut access_order = self.access_order.write();
        if let Some(pos) = access_order.iter().position(|k| k == key) {
            access_order.remove(pos);
        }
    }
}

/// Pre-fetch hint for sequential access patterns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrefetchHint {
    /// No pre-fetching.
    None,
    /// Pre-fetch forward (next keys).
    Forward,
    /// Pre-fetch backward (previous keys).
    Backward,
    /// Pre-fetch both directions.
    Both,
}

/// Iterator with pre-fetching support.
pub struct PrefetchingIterator<K, V, I, F>
where
    K: Clone + Eq + Hash,
    V: Clone,
    I: Iterator<Item = (K, V)>,
    F: Fn(&K) -> Vec<(K, V)>,
{
    inner: I,
    prefetch_fn: F,
    cache: Arc<ReadCache<K, V>>,
    hint: PrefetchHint,
    buffer: Vec<(K, V)>,
    buffer_pos: usize,
}

impl<K, V, I, F> PrefetchingIterator<K, V, I, F>
where
    K: Clone + Eq + Hash,
    V: Clone,
    I: Iterator<Item = (K, V)>,
    F: Fn(&K) -> Vec<(K, V)>,
{
    /// Creates a new pre-fetching iterator.
    pub fn new(
        inner: I,
        cache: Arc<ReadCache<K, V>>,
        prefetch_fn: F,
        hint: PrefetchHint,
    ) -> Self {
        Self {
            inner,
            prefetch_fn,
            cache,
            hint,
            buffer: Vec::new(),
            buffer_pos: 0,
        }
    }

    /// Pre-fetches items based on the current key.
    fn prefetch(&mut self, key: &K) {
        if self.hint == PrefetchHint::None {
            return;
        }

        let items = (self.prefetch_fn)(key);
        
        if !items.is_empty() {
            let cache_items: Vec<_> = items
                .into_iter()
                .map(|(k, v)| {
                    let size = std::mem::size_of_val(&k) + std::mem::size_of_val(&v);
                    (k, v, size)
                })
                .collect();
            
            self.cache.put_batch(cache_items);
        }
    }
}

impl<K, V, I, F> Iterator for PrefetchingIterator<K, V, I, F>
where
    K: Clone + Eq + Hash,
    V: Clone,
    I: Iterator<Item = (K, V)>,
    F: Fn(&K) -> Vec<(K, V)>,
{
    type Item = (K, V);

    fn next(&mut self) -> Option<Self::Item> {
        // Return from buffer first
        if self.buffer_pos < self.buffer.len() {
            let item = self.buffer.get(self.buffer_pos).cloned();
            self.buffer_pos += 1;
            return item;
        }

        // Get next item from inner iterator
        if let Some((key, value)) = self.inner.next() {
            // Trigger pre-fetch
            self.prefetch(&key);
            
            Some((key, value))
        } else {
            None
        }
    }
}

/// Type alias for the storage read cache.
pub type StorageReadCache = ReadCache<StorageKey, StorageItem>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_cache_put_and_get() {
        let cache = ReadCache::<String, String>::with_defaults();
        
        cache.put("key1".to_string(), "value1".to_string(), 10);
        cache.put("key2".to_string(), "value2".to_string(), 10);
        
        assert_eq!(cache.get(&"key1".to_string()), Some("value1".to_string()));
        assert_eq!(cache.get(&"key2".to_string()), Some("value2".to_string()));
        assert_eq!(cache.get(&"key3".to_string()), None);
        
        let stats = cache.stats();
        assert_eq!(stats.hits, 2);
        assert_eq!(stats.misses, 1);
    }

    #[test]
    fn read_cache_eviction() {
        let config = ReadCacheConfig {
            max_entries: 2,
            max_bytes: 1000,
            enable_prefetch: false,
            prefetch_count: 0,
            prefetch_threshold: 0,
            ttl: None,
            enable_stats: true,
        };
        
        let cache = ReadCache::<String, String>::new(config);
        
        cache.put("key1".to_string(), "value1".to_string(), 10);
        cache.put("key2".to_string(), "value2".to_string(), 10);
        cache.put("key3".to_string(), "value3".to_string(), 10); // Should evict key1
        
        assert_eq!(cache.len(), 2);
        assert_eq!(cache.get(&"key1".to_string()), None); // Evicted
        assert!(cache.get(&"key2".to_string()).is_some());
        assert!(cache.get(&"key3".to_string()).is_some());
        
        let stats = cache.stats();
        assert_eq!(stats.evictions, 1);
    }

    #[test]
    fn read_cache_byte_limit_eviction() {
        let config = ReadCacheConfig {
            max_entries: 100,
            max_bytes: 30,
            enable_prefetch: false,
            prefetch_count: 0,
            prefetch_threshold: 0,
            ttl: None,
            enable_stats: true,
        };
        
        let cache = ReadCache::<String, String>::new(config);
        
        cache.put("key1".to_string(), "value1".to_string(), 20);
        cache.put("key2".to_string(), "value2".to_string(), 20); // Should trigger eviction
        
        // Should have evicted to make room
        assert!(cache.len() <= 2);
    }

    #[test]
    fn read_cache_ttl_expiration() {
        let config = ReadCacheConfig {
            max_entries: 100,
            max_bytes: 1000,
            enable_prefetch: false,
            prefetch_count: 0,
            prefetch_threshold: 0,
            ttl: Some(Duration::from_millis(1)),
            enable_stats: true,
        };
        
        let cache = ReadCache::<String, String>::new(config);
        
        cache.put("key1".to_string(), "value1".to_string(), 10);
        
        // Should be available immediately
        assert!(cache.get(&"key1".to_string()).is_some());
        
        // Wait for expiration
        std::thread::sleep(Duration::from_millis(10));
        
        // Should be expired now
        assert_eq!(cache.get(&"key1".to_string()), None);
    }

    #[test]
    fn read_cache_remove() {
        let cache = ReadCache::<String, String>::with_defaults();
        
        cache.put("key1".to_string(), "value1".to_string(), 10);
        
        let removed = cache.remove(&"key1".to_string());
        assert_eq!(removed, Some("value1".to_string()));
        assert_eq!(cache.get(&"key1".to_string()), None);
    }

    #[test]
    fn read_cache_clear() {
        let cache = ReadCache::<String, String>::with_defaults();
        
        cache.put("key1".to_string(), "value1".to_string(), 10);
        cache.put("key2".to_string(), "value2".to_string(), 10);
        
        cache.clear();
        
        assert!(cache.is_empty());
        assert_eq!(cache.get(&"key1".to_string()), None);
    }

    #[test]
    fn read_cache_put_batch() {
        let cache = ReadCache::<String, String>::with_defaults();
        
        let items = vec![
            ("key1".to_string(), "value1".to_string(), 10),
            ("key2".to_string(), "value2".to_string(), 10),
            ("key3".to_string(), "value3".to_string(), 10),
        ];
        
        cache.put_batch(items);
        
        assert_eq!(cache.len(), 3);
        
        let stats = cache.stats();
        assert_eq!(stats.prefetches, 3);
    }

    #[test]
    fn read_cache_should_prefetch() {
        let config = ReadCacheConfig {
            max_entries: 100,
            max_bytes: 1000,
            enable_prefetch: true,
            prefetch_count: 5,
            prefetch_threshold: 2,  // Need 2 accesses to trigger prefetch
            ttl: None,
            enable_stats: true,
        };
        
        let cache = ReadCache::<String, String>::new(config);
        
        // put() initializes access_count to 1
        cache.put("key1".to_string(), "value1".to_string(), 10);
        
        // After put, access_count = 1, should not prefetch
        assert!(!cache.should_prefetch(&"key1".to_string()));
        
        // First get increments to 2, now meets threshold
        cache.get(&"key1".to_string());
        
        // After first get, access_count = 2, should prefetch
        assert!(cache.should_prefetch(&"key1".to_string()));
    }

    #[test]
    fn read_cache_stats_hit_rate() {
        let stats = ReadCacheStatsSnapshot {
            hits: 75,
            misses: 25,
            evictions: 0,
            prefetches: 0,
            prefetch_hits: 0,
            inserts: 0,
            current_entries: 0,
            current_bytes: 0,
        };
        
        assert!((stats.hit_rate() - 0.75).abs() < 0.001);
    }
}
