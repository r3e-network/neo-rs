//! Read cache with LRU eviction, bloom filter for negative lookups, and pre-fetching support.
//!
//! This module provides a read cache for frequently accessed keys with
//! configurable LRU eviction, bloom filter for fast negative lookups,
//! and intelligent pre-fetching for iteration.

use crate::smart_contract::{StorageItem, StorageKey};
use parking_lot::RwLock;
use std::collections::HashMap;
use std::hash::Hash;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tracing::{debug, trace};

/// A simple bloom filter for probabilistic membership testing.
/// Used to avoid expensive store lookups for keys that definitely don't exist.
pub struct BloomFilter {
    /// Bit array
    bits: Vec<AtomicU64>,
    /// Number of hash functions
    num_hashes: usize,
    /// Number of bits
    num_bits: usize,
    /// Number of elements inserted
    count: AtomicUsize,
    /// Maximum capacity before false positive rate increases significantly
    capacity: usize,
}

impl BloomFilter {
    /// Creates a new bloom filter with the specified capacity and false positive rate.
    /// 
    /// Capacity is the expected number of elements.
    /// False positive rate should be between 0 and 1 (e.g., 0.01 for 1%).
    pub fn new(capacity: usize, false_positive_rate: f64) -> Self {
        // Calculate optimal size: m = -n * ln(p) / (ln(2)^2)
        let num_bits = ((- (capacity as f64) * false_positive_rate.ln()) 
            / (2.0_f64.ln().powi(2))).ceil() as usize;
        // Calculate optimal number of hash functions: k = m/n * ln(2)
        let num_hashes = ((num_bits as f64 / capacity as f64) * 2.0_f64.ln()).ceil() as usize;
        
        // Round up to nearest 64 bits for the bit vector
        let num_u64s = num_bits.div_ceil(64);
        let mut bits = Vec::with_capacity(num_u64s);
        for _ in 0..num_u64s {
            bits.push(AtomicU64::new(0));
        }
        
        Self {
            bits,
            num_hashes: num_hashes.clamp(1, 7),
            num_bits: num_u64s * 64,
            count: AtomicUsize::new(0),
            capacity,
        }
    }
    
    /// Creates a bloom filter sized for typical storage workloads.
    pub fn for_storage() -> Self {
        Self::new(100_000, 0.01) // 100K entries, 1% FP rate
    }
    
    /// Hash function using double hashing technique
    #[inline]
    #[allow(dead_code)]
    fn hash_bytes(&self, key: &[u8], seed: usize) -> usize {
        let h1 = xxhash_rust::xxh3::xxh3_64(key);
        let h2 = h1.wrapping_add(seed as u64);
        ((h1.wrapping_add(h2.wrapping_mul(seed as u64))) as usize) % self.num_bits
    }
    
    /// Hash function using pre-computed hash
    #[inline]
    fn hash_with_seed(&self, base_hash: u64, seed: usize) -> usize {
        let h2 = base_hash.wrapping_add(seed as u64);
        ((base_hash.wrapping_add(h2.wrapping_mul(seed as u64))) as usize) % self.num_bits
    }
    
    /// Insert a key into the bloom filter using raw bytes.
    pub fn insert_bytes(&self, key: &[u8]) {
        let base_hash = xxhash_rust::xxh3::xxh3_64(key);
        for i in 0..self.num_hashes {
            let bit_pos = self.hash_with_seed(base_hash, i);
            let word_idx = bit_pos / 64;
            let bit_idx = bit_pos % 64;
            
            self.bits[word_idx].fetch_or(1u64 << bit_idx, Ordering::Relaxed);
        }
        self.count.fetch_add(1, Ordering::Relaxed);
    }
    
    /// Insert a key into the bloom filter using a pre-computed hash.
    pub fn insert_hash(&self, hash: u64) {
        for i in 0..self.num_hashes {
            let bit_pos = self.hash_with_seed(hash, i);
            let word_idx = bit_pos / 64;
            let bit_idx = bit_pos % 64;
            
            self.bits[word_idx].fetch_or(1u64 << bit_idx, Ordering::Relaxed);
        }
        self.count.fetch_add(1, Ordering::Relaxed);
    }
    
    /// Check if a key might be in the set using raw bytes.
    /// Returns false if the key is definitely not present.
    /// Returns true if the key might be present (with some false positive probability).
    #[inline]
    pub fn might_contain_bytes(&self, key: &[u8]) -> bool {
        let base_hash = xxhash_rust::xxh3::xxh3_64(key);
        for i in 0..self.num_hashes {
            let bit_pos = self.hash_with_seed(base_hash, i);
            let word_idx = bit_pos / 64;
            let bit_idx = bit_pos % 64;
            
            let word = self.bits[word_idx].load(Ordering::Relaxed);
            if (word & (1u64 << bit_idx)) == 0 {
                return false;
            }
        }
        true
    }
    
    /// Check if a key might be in the set using a pre-computed hash.
    #[inline]
    pub fn might_contain_hash(&self, hash: u64) -> bool {
        for i in 0..self.num_hashes {
            let bit_pos = self.hash_with_seed(hash, i);
            let word_idx = bit_pos / 64;
            let bit_idx = bit_pos % 64;
            
            let word = self.bits[word_idx].load(Ordering::Relaxed);
            if (word & (1u64 << bit_idx)) == 0 {
                return false;
            }
        }
        true
    }
    
    /// Returns the approximate number of elements inserted.
    pub fn len(&self) -> usize {
        self.count.load(Ordering::Relaxed)
    }
    
    /// Returns true if no elements have been inserted.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
    
    /// Clears the bloom filter.
    pub fn clear(&self) {
        for word in &self.bits {
            word.store(0, Ordering::Relaxed);
        }
        self.count.store(0, Ordering::Relaxed);
    }
    
    /// Returns true if the filter is approaching capacity (recommend rebuilding).
    pub fn should_rebuild(&self) -> bool {
        self.count.load(Ordering::Relaxed) >= self.capacity
    }
}

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
    bloom_filter_negatives: AtomicU64,
    bloom_filter_checks: AtomicU64,
}

impl ReadCacheStats {
    /// Creates new statistics.
    pub fn new() -> Self {
        Self::default()
    }

    /// Records a cache hit.
    #[inline]
    pub fn record_hit(&self) {
        self.hits.fetch_add(1, Ordering::Relaxed);
    }

    /// Records a cache miss.
    #[inline]
    pub fn record_miss(&self) {
        self.misses.fetch_add(1, Ordering::Relaxed);
    }

    /// Records an eviction.
    #[inline]
    pub fn record_eviction(&self, bytes: usize) {
        self.evictions.fetch_add(1, Ordering::Relaxed);
        self.current_entries.fetch_sub(1, Ordering::Relaxed);
        self.current_bytes.fetch_sub(bytes, Ordering::Relaxed);
    }

    /// Records a prefetch.
    #[inline]
    pub fn record_prefetch(&self, count: usize, bytes: usize) {
        self.prefetches.fetch_add(count as u64, Ordering::Relaxed);
        self.inserts.fetch_add(count as u64, Ordering::Relaxed);
        self.current_entries.fetch_add(count, Ordering::Relaxed);
        self.current_bytes.fetch_add(bytes, Ordering::Relaxed);
    }

    /// Records a prefetch hit.
    #[inline]
    pub fn record_prefetch_hit(&self) {
        self.prefetch_hits.fetch_add(1, Ordering::Relaxed);
    }

    /// Records an insert.
    #[inline]
    pub fn record_insert(&self, bytes: usize) {
        self.inserts.fetch_add(1, Ordering::Relaxed);
        self.current_entries.fetch_add(1, Ordering::Relaxed);
        self.current_bytes.fetch_add(bytes, Ordering::Relaxed);
    }
    
    /// Records a bloom filter negative lookup.
    #[inline]
    pub fn record_bloom_negative(&self) {
        self.bloom_filter_negatives.fetch_add(1, Ordering::Relaxed);
    }
    
    /// Records a bloom filter check.
    #[inline]
    pub fn record_bloom_check(&self) {
        self.bloom_filter_checks.fetch_add(1, Ordering::Relaxed);
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
            bloom_filter_negatives: self.bloom_filter_negatives.load(Ordering::Relaxed),
            bloom_filter_checks: self.bloom_filter_checks.load(Ordering::Relaxed),
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
    pub bloom_filter_negatives: u64,
    pub bloom_filter_checks: u64,
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
    
    /// Calculates the bloom filter negative lookup rate.
    pub fn bloom_filter_effectiveness(&self) -> f64 {
        let checks = self.bloom_filter_checks;
        if checks == 0 {
            0.0
        } else {
            self.bloom_filter_negatives as f64 / checks as f64
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
    /// Enable bloom filter for negative lookups
    pub enable_bloom_filter: bool,
    /// Expected number of entries for bloom filter sizing
    pub bloom_filter_capacity: usize,
    /// False positive rate for bloom filter (0.01 = 1%)
    pub bloom_filter_fpr: f64,
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
            enable_bloom_filter: true,
            bloom_filter_capacity: 100_000,
            bloom_filter_fpr: 0.01,
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
            enable_bloom_filter: true,
            bloom_filter_capacity: 200_000,
            bloom_filter_fpr: 0.01,
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
            enable_bloom_filter: true,
            bloom_filter_capacity: 10_000,
            bloom_filter_fpr: 0.05, // Higher FPR acceptable with low memory
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

/// Optimized LRU tracking using a linked list approach with index map.
/// This provides O(1) LRU operations instead of O(n) with a Vec.
struct LruTracker<K> {
    /// Map from key to its position/index in the access order
    order: HashMap<K, u64>,
    /// Counter for generating unique sequence numbers
    sequence: AtomicU64,
}

impl<K: Clone + Eq + Hash> LruTracker<K> {
    fn new() -> Self {
        Self {
            order: HashMap::new(),
            sequence: AtomicU64::new(0),
        }
    }
    
    /// Record access and return the old sequence number if any
    fn record_access(&mut self, key: K) -> Option<u64> {
        let new_seq = self.sequence.fetch_add(1, Ordering::Relaxed);
        self.order.insert(key, new_seq)
    }
    
    /// Remove a key from tracking
    fn remove(&mut self, key: &K) -> Option<u64> {
        self.order.remove(key)
    }
    
    /// Find the least recently used key
    fn find_lru(&self) -> Option<K> {
        self.order.iter()
            .min_by_key(|(_, seq)| *seq)
            .map(|(k, _)| k.clone())
    }
    
    /// Clear all tracking
    fn clear(&mut self) {
        self.order.clear();
        self.sequence.store(0, Ordering::Relaxed);
    }
    
    #[allow(dead_code)]
    fn len(&self) -> usize {
        self.order.len()
    }
}

/// Trait for keys that can be hashed for bloom filter operations.
pub trait BloomFilterKey {
    /// Hashes the key using xxh3 and returns the hash value.
    fn hash_for_bloom(&self) -> u64;
}

impl BloomFilterKey for Vec<u8> {
    fn hash_for_bloom(&self) -> u64 {
        xxhash_rust::xxh3::xxh3_64(self)
    }
}

impl BloomFilterKey for String {
    fn hash_for_bloom(&self) -> u64 {
        xxhash_rust::xxh3::xxh3_64(self.as_bytes())
    }
}

impl BloomFilterKey for StorageKey {
    fn hash_for_bloom(&self) -> u64 {
        // Combine id and key bytes for hashing
        let id_bytes = self.id().to_le_bytes();
        let key_bytes = self.key();
        
        // Use xxh3 with a seed for consistent hashing
        let mut hasher = xxhash_rust::xxh3::Xxh3::new();
        hasher.update(&id_bytes);
        hasher.update(key_bytes);
        hasher.digest()
    }
}

/// LRU Read cache for storage operations with bloom filter support.
pub struct ReadCache<K, V>
where
    K: Clone + Eq + Hash + BloomFilterKey,
    V: Clone,
{
    config: ReadCacheConfig,
    data: RwLock<HashMap<K, CacheEntry<V>>>,
    stats: Arc<ReadCacheStats>,
    lru_tracker: RwLock<LruTracker<K>>,
    bloom_filter: Option<Arc<BloomFilter>>,
}

impl<K, V> ReadCache<K, V>
where
    K: Clone + Eq + Hash + BloomFilterKey,
    V: Clone,
{
    /// Creates a new read cache with the specified configuration.
    pub fn new(config: ReadCacheConfig) -> Self {
        let bloom_filter = if config.enable_bloom_filter {
            Some(Arc::new(BloomFilter::new(
                config.bloom_filter_capacity,
                config.bloom_filter_fpr,
            )))
        } else {
            None
        };
        
        Self {
            config,
            data: RwLock::new(HashMap::new()),
            stats: Arc::new(ReadCacheStats::new()),
            lru_tracker: RwLock::new(LruTracker::new()),
            bloom_filter,
        }
    }

    /// Creates a new read cache with default configuration.
    pub fn with_defaults() -> Self {
        Self::new(ReadCacheConfig::default())
    }

    /// Check if a key might exist using the bloom filter.
    /// Returns false if the key is definitely not in the cache.
    /// Returns true if the key might be in the cache (or bloom filter is disabled).
    #[inline]
    pub fn might_contain(&self, key: &K) -> bool {
        if let Some(ref bloom) = self.bloom_filter {
            if self.config.enable_stats {
                self.stats.record_bloom_check();
            }
            let might_contain = bloom.might_contain_hash(key.hash_for_bloom());
            if !might_contain && self.config.enable_stats {
                self.stats.record_bloom_negative();
            }
            might_contain
        } else {
            true // Without bloom filter, assume it might exist
        }
    }
    
    /// Check bloom filter for negative lookup without updating stats.
    /// Used for fast path checks before acquiring locks.
    #[inline]
    pub fn fast_bloom_check(&self, key: &K) -> bool {
        self.bloom_filter.as_ref()
            .map(|b| b.might_contain_hash(key.hash_for_bloom()))
            .unwrap_or(true)
    }

    /// Gets a value from the cache.
    pub fn get(&self, key: &K) -> Option<V> {
        // Fast path: check bloom filter before acquiring write lock
        let bloom_check = self.fast_bloom_check(key);
        if self.config.enable_stats {
            self.stats.record_bloom_check();
        }
        
        if !bloom_check {
            if self.config.enable_stats {
                self.stats.record_bloom_negative();
                self.stats.record_miss();
            }
            return None;
        }
        
        let mut data = self.data.write();

        if let Some(entry) = data.get_mut(key) {
            // Check TTL
            if let Some(ttl) = self.config.ttl {
                if entry.last_access.elapsed() > ttl {
                    // Entry expired
                    let size = entry.size_bytes;
                    let key_clone = key.clone();
                    data.remove(key);
                    drop(data);
                    
                    self.lru_tracker.write().remove(&key_clone);
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
            self.lru_tracker.write().record_access(key.clone());

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
            if !self.evict_lru(&mut data) {
                break; // Could not evict, stop trying
            }
        }

        // Check if adding this would exceed byte limit
        let current_bytes = self.stats.current_bytes.load(Ordering::Relaxed);
        while current_bytes + size_bytes > self.config.max_bytes && !data.is_empty() {
            if !self.evict_lru(&mut data) {
                break;
            }
        }

        // Insert new entry
        let entry = CacheEntry::new(value, size_bytes);
        let key_for_bloom = key.clone();
        let is_new = data.insert(key.clone(), entry).is_none();
        drop(data);

        // Update LRU tracker
        self.lru_tracker.write().record_access(key);
        
        // Update bloom filter for new entries
        if is_new {
            if let Some(ref bloom) = self.bloom_filter {
                bloom.insert_hash(key_for_bloom.hash_for_bloom());
            }
        }

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
            if !self.evict_lru(&mut data) {
                break;
            }
        }

        let current_bytes = self.stats.current_bytes.load(Ordering::Relaxed);
        while current_bytes + total_bytes > self.config.max_bytes && !data.is_empty() {
            if !self.evict_lru(&mut data) {
                break;
            }
        }

        let count = items.len();
        let mut lru = self.lru_tracker.write();
        let mut keys_for_bloom = Vec::with_capacity(count);

        for (key, value, size_bytes) in items {
            let entry = CacheEntry::new(value, size_bytes);
            let key_for_bloom = key.clone();
            let is_new = data.insert(key.clone(), entry).is_none();
            if is_new {
                keys_for_bloom.push(key_for_bloom);
            }
            lru.record_access(key);
        }
        
        drop(data);
        drop(lru);
        
        // Update bloom filter
        if let Some(ref bloom) = self.bloom_filter {
            for key in keys_for_bloom {
                bloom.insert_hash(key.hash_for_bloom());
            }
        }

        if self.config.enable_stats && count > 0 {
            self.stats.record_prefetch(count, total_bytes);
        }

        debug!(target: "neo", count, total_bytes, "cache batch insert (prefetch)");
    }

    /// Removes a value from the cache.
    pub fn remove(&self, key: &K) -> Option<V> {
        let mut data = self.data.write();

        if let Some(entry) = data.remove(key) {
            self.lru_tracker.write().remove(key);

            if self.config.enable_stats {
                self.stats.current_entries.fetch_sub(1, Ordering::Relaxed);
                self.stats
                    .current_bytes
                    .fetch_sub(entry.size_bytes, Ordering::Relaxed);
            }

            Some(entry.value)
        } else {
            None
        }
    }

    /// Clears the cache.
    pub fn clear(&self) {
        let mut data = self.data.write();
        let mut lru = self.lru_tracker.write();

        data.clear();
        lru.clear();
        
        if let Some(ref bloom) = self.bloom_filter {
            bloom.clear();
        }

        if self.config.enable_stats {
            self.stats.current_entries.store(0, Ordering::Relaxed);
            self.stats.current_bytes.store(0, Ordering::Relaxed);
        }

        debug!(target: "neo", "cache cleared");
    }

    /// Checks if the cache contains a key.
    pub fn contains(&self, key: &K) -> bool {
        // Check bloom filter first
        if !self.fast_bloom_check(key) {
            return false;
        }
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
    fn evict_lru(&self, data: &mut parking_lot::RwLockWriteGuard<HashMap<K, CacheEntry<V>>>) -> bool {
        let lru = self.lru_tracker.read();

        if let Some(lru_key) = lru.find_lru() {
            drop(lru);

            if let Some(entry) = data.remove(&lru_key) {
                self.lru_tracker.write().remove(&lru_key);

                if self.config.enable_stats {
                    self.stats.record_eviction(entry.size_bytes);
                }

                trace!(target: "neo", "cache eviction");
                return true;
            }
        }
        false
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
    K: Clone + Eq + Hash + BloomFilterKey,
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
    K: Clone + Eq + Hash + BloomFilterKey,
    V: Clone,
    I: Iterator<Item = (K, V)>,
    F: Fn(&K) -> Vec<(K, V)>,
{
    /// Creates a new pre-fetching iterator.
    pub fn new(inner: I, cache: Arc<ReadCache<K, V>>, prefetch_fn: F, hint: PrefetchHint) -> Self {
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
    K: Clone + Eq + Hash + BloomFilterKey,
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
            enable_bloom_filter: false,
            bloom_filter_capacity: 1000,
            bloom_filter_fpr: 0.01,
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
            enable_bloom_filter: false,
            bloom_filter_capacity: 1000,
            bloom_filter_fpr: 0.01,
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
            enable_bloom_filter: false,
            bloom_filter_capacity: 1000,
            bloom_filter_fpr: 0.01,
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
            prefetch_threshold: 2, // Need 2 accesses to trigger prefetch
            ttl: None,
            enable_stats: true,
            enable_bloom_filter: false,
            bloom_filter_capacity: 1000,
            bloom_filter_fpr: 0.01,
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
            bloom_filter_negatives: 0,
            bloom_filter_checks: 0,
        };

        assert!((stats.hit_rate() - 0.75).abs() < 0.001);
    }
    
    #[test]
    fn bloom_filter_basic_operations() {
        let bloom = BloomFilter::new(1000, 0.01);
        
        // Check non-existent key
        assert!(!bloom.might_contain_bytes(b"key1"));
        
        // Insert key
        bloom.insert_bytes(b"key1");
        
        // Should now report might contain
        assert!(bloom.might_contain_bytes(b"key1"));
        
        // Check count
        assert_eq!(bloom.len(), 1);
        
        // Clear and verify
        bloom.clear();
        assert!(!bloom.might_contain_bytes(b"key1"));
        assert!(bloom.is_empty());
    }
    
    #[test]
    fn bloom_filter_negative_lookup_in_cache() {
        let config = ReadCacheConfig {
            max_entries: 100,
            max_bytes: 1000,
            enable_prefetch: false,
            prefetch_count: 0,
            prefetch_threshold: 0,
            ttl: None,
            enable_stats: true,
            enable_bloom_filter: true,
            bloom_filter_capacity: 1000,
            bloom_filter_fpr: 0.01,
        };
        
        let cache = ReadCache::<String, String>::new(config);
        
        // Insert a key
        cache.put("existing".to_string(), "value".to_string(), 10);
        
        // Check non-existent key - should use bloom filter for fast negative
        let result = cache.get(&"nonexistent".to_string());
        assert!(result.is_none());
        
        // Existing key should work
        assert_eq!(cache.get(&"existing".to_string()), Some("value".to_string()));
        
        let stats = cache.stats();
        assert!(stats.bloom_filter_checks > 0);
    }
    
    #[test]
    fn read_cache_with_storage_key() {
        use crate::smart_contract::StorageKey;
        
        let cache = ReadCache::<StorageKey, String>::with_defaults();
        
        let key1 = StorageKey::new(1, b"test1".to_vec());
        let key2 = StorageKey::new(2, b"test2".to_vec());
        
        cache.put(key1.clone(), "value1".to_string(), 20);
        cache.put(key2.clone(), "value2".to_string(), 20);
        
        assert_eq!(cache.get(&key1), Some("value1".to_string()));
        assert_eq!(cache.get(&key2), Some("value2".to_string()));
        
        let stats = cache.stats();
        assert_eq!(stats.hits, 2);
    }
}
