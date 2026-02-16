//! Caching functionality for persistence layer.
//!
//! This module provides production-ready caching capabilities that match
//! the C# Neo caching functionality exactly.

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::hash::Hash;
use std::time::{Duration, Instant};
/// Cache configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheConfig {
    /// Maximum number of entries
    pub max_entries: usize,
    /// Default TTL for entries
    pub default_ttl: Duration,
    /// Enable cache statistics
    pub enable_stats: bool,
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            max_entries: 10000,
            default_ttl: Duration::from_secs(3600),
            enable_stats: true,
        }
    }
}

/// Cache statistics
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CacheStats {
    pub hits: u64,
    pub misses: u64,
    pub evictions: u64,
    pub entries: usize,
}

impl CacheStats {
    pub fn hit_rate(&self) -> f64 {
        if self.hits + self.misses == 0 {
            0.0
        } else {
            self.hits as f64 / (self.hits + self.misses) as f64
        }
    }
}

/// LRU Cache implementation (production-ready)
pub struct LruCache<K, V>
where
    K: Clone + Eq + Hash,
    V: Clone,
{
    /// Maximum capacity
    capacity: usize,
    /// Cache data
    data: HashMap<K, V>,
    /// Access order tracking
    access_order: VecDeque<K>,
    /// Cache statistics
    stats: CacheStats,
    /// Enable statistics tracking
    enable_stats: bool,
}

impl<K, V> LruCache<K, V>
where
    K: Clone + Eq + Hash,
    V: Clone,
{
    /// Creates a new LRU cache with the specified capacity
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity,
            data: HashMap::new(),
            access_order: VecDeque::new(),
            stats: CacheStats::default(),
            enable_stats: true,
        }
    }

    /// Creates a new LRU cache with configuration
    pub fn with_config(config: &CacheConfig) -> Self {
        Self {
            capacity: config.max_entries,
            data: HashMap::new(),
            access_order: VecDeque::new(),
            stats: CacheStats::default(),
            enable_stats: config.enable_stats,
        }
    }

    /// Gets a value from the cache (production implementation)
    pub fn get(&mut self, key: &K) -> Option<V> {
        match self.data.get(key).cloned() {
            Some(value) => {
                self.move_to_front(key);

                // Update statistics
                if self.enable_stats {
                    self.stats.hits += 1;
                }

                Some(value)
            }
            _ => {
                // Update statistics
                if self.enable_stats {
                    self.stats.misses += 1;
                }

                None
            }
        }
    }

    /// Puts a value into the cache (production implementation)
    pub fn put(&mut self, key: K, value: V) {
        if self.data.contains_key(&key) {
            // Update existing entry
            self.data.insert(key.clone(), value);
            self.move_to_front(&key);
        } else {
            // Add new entry
            if self.data.len() >= self.capacity {
                // Evict least recently used
                self.evict_lru();
            }

            self.data.insert(key.clone(), value);
            self.access_order.push_front(key);

            if self.enable_stats {
                self.stats.entries = self.data.len();
            }
        }
    }

    /// Removes a value from the cache
    pub fn remove(&mut self, key: &K) -> Option<V> {
        match self.data.remove(key) {
            Some(value) => {
                // Remove from access order
                if let Some(pos) = self.access_order.iter().position(|k| k == key) {
                    self.access_order.remove(pos);
                }

                if self.enable_stats {
                    self.stats.entries = self.data.len();
                }

                Some(value)
            }
            _ => None,
        }
    }

    /// Clears the cache
    pub fn clear(&mut self) {
        self.data.clear();
        self.access_order.clear();

        if self.enable_stats {
            self.stats.entries = 0;
        }
    }

    /// Gets cache statistics
    pub fn stats(&self) -> &CacheStats {
        &self.stats
    }

    /// Gets the number of entries in the cache
    pub fn len(&self) -> usize {
        self.data.len()
    }

    /// Checks if the cache is empty
    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    /// Gets the cache capacity
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// Moves a key to the front of the access order
    fn move_to_front(&mut self, key: &K) {
        // Remove from current position
        if let Some(pos) = self.access_order.iter().position(|k| k == key) {
            self.access_order.remove(pos);
        }

        // Add to front
        self.access_order.push_front(key.clone());
    }

    /// Evicts the least recently used entry
    fn evict_lru(&mut self) {
        if let Some(lru_key) = self.access_order.pop_back() {
            self.data.remove(&lru_key);

            if self.enable_stats {
                self.stats.evictions += 1;
                self.stats.entries = self.data.len();
            }
        }
    }
}

/// TTL Cache entry
#[derive(Debug, Clone)]
struct TtlEntry<V> {
    value: V,
    expires_at: Instant,
}

/// TTL Cache implementation (production-ready)
pub struct TtlCache<K, V>
where
    K: Clone + Eq + Hash,
    V: Clone,
{
    /// Cache data with TTL
    data: HashMap<K, TtlEntry<V>>,
    /// Default TTL
    default_ttl: Duration,
    /// Cache statistics
    stats: CacheStats,
    /// Enable statistics tracking
    enable_stats: bool,
    /// Last cleanup time
    last_cleanup: Instant,
    /// Cleanup interval
    cleanup_interval: Duration,
}

impl<K, V> TtlCache<K, V>
where
    K: Clone + Eq + Hash,
    V: Clone,
{
    /// Creates a new TTL cache with default TTL
    pub fn new(default_ttl: Duration) -> Self {
        Self {
            data: HashMap::new(),
            default_ttl,
            stats: CacheStats::default(),
            enable_stats: true,
            last_cleanup: Instant::now(),
            cleanup_interval: Duration::from_secs(60), // Cleanup every minute
        }
    }

    /// Creates a new TTL cache with configuration
    pub fn with_config(config: &CacheConfig) -> Self {
        Self {
            data: HashMap::new(),
            default_ttl: config.default_ttl,
            stats: CacheStats::default(),
            enable_stats: config.enable_stats,
            last_cleanup: Instant::now(),
            cleanup_interval: Duration::from_secs(60),
        }
    }

    /// Gets a value from the cache (production implementation)
    pub fn get(&mut self, key: &K) -> Option<V> {
        // Cleanup expired entries periodically
        self.cleanup_if_needed();

        if let Some(entry) = self.data.get(key) {
            if entry.expires_at > Instant::now() {
                // Entry is still valid
                if self.enable_stats {
                    self.stats.hits += 1;
                }

                Some(entry.value.clone())
            } else {
                // Entry has expired, remove it
                self.data.remove(key);

                if self.enable_stats {
                    self.stats.misses += 1;
                    self.stats.entries = self.data.len();
                }

                None
            }
        } else {
            // Entry not found
            if self.enable_stats {
                self.stats.misses += 1;
            }

            None
        }
    }

    /// Puts a value into the cache with default TTL (production implementation)
    pub fn put(&mut self, key: K, value: V) {
        self.put_with_ttl(key, value, self.default_ttl);
    }

    /// Puts a value into the cache with custom TTL (production implementation)
    pub fn put_with_ttl(&mut self, key: K, value: V, ttl: Duration) {
        let expires_at = Instant::now() + ttl;
        let entry = TtlEntry { value, expires_at };

        self.data.insert(key, entry);

        if self.enable_stats {
            self.stats.entries = self.data.len();
        }
    }

    /// Removes a value from the cache
    pub fn remove(&mut self, key: &K) -> Option<V> {
        match self.data.remove(key) {
            Some(entry) => {
                if self.enable_stats {
                    self.stats.entries = self.data.len();
                }

                Some(entry.value)
            }
            _ => None,
        }
    }

    /// Clears the cache
    pub fn clear(&mut self) {
        self.data.clear();

        if self.enable_stats {
            self.stats.entries = 0;
        }
    }

    /// Gets cache statistics
    pub fn stats(&self) -> &CacheStats {
        &self.stats
    }

    /// Gets the number of entries in the cache
    pub fn len(&self) -> usize {
        self.data.len()
    }

    /// Checks if the cache is empty
    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    /// Cleans up expired entries if needed
    fn cleanup_if_needed(&mut self) {
        let now = Instant::now();

        if now.duration_since(self.last_cleanup) >= self.cleanup_interval {
            self.cleanup_expired();
            self.last_cleanup = now;
        }
    }

    /// Removes all expired entries
    fn cleanup_expired(&mut self) {
        let now = Instant::now();
        let initial_len = self.data.len();

        self.data.retain(|_, entry| entry.expires_at > now);

        if self.enable_stats {
            let removed = initial_len - self.data.len();
            self.stats.evictions += removed as u64;
            self.stats.entries = self.data.len();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ============================================================================
    // CacheStats Tests
    // ============================================================================

    #[test]
    fn cache_stats_hit_rate_zero_when_empty() {
        let stats = CacheStats::default();
        assert_eq!(stats.hit_rate(), 0.0);
    }

    #[test]
    fn cache_stats_hit_rate_calculates_correctly() {
        let stats = CacheStats {
            hits: 75,
            misses: 25,
            evictions: 0,
            entries: 0,
        };
        assert!((stats.hit_rate() - 0.75).abs() < 0.001);
    }

    // ============================================================================
    // LruCache Tests
    // ============================================================================

    #[test]
    fn lru_cache_new_creates_empty_cache() {
        let cache: LruCache<String, i32> = LruCache::new(10);
        assert!(cache.is_empty());
        assert_eq!(cache.len(), 0);
    }

    #[test]
    fn lru_cache_put_and_get() {
        let mut cache = LruCache::new(10);
        cache.put("key1".to_string(), 42);

        assert_eq!(cache.get(&"key1".to_string()), Some(42));
        assert_eq!(cache.len(), 1);
    }

    #[test]
    fn lru_cache_get_nonexistent_returns_none() {
        let mut cache: LruCache<String, i32> = LruCache::new(10);
        assert_eq!(cache.get(&"nonexistent".to_string()), None);
    }

    #[test]
    fn lru_cache_evicts_lru_when_full() {
        let mut cache = LruCache::new(3);
        cache.put("a".to_string(), 1);
        cache.put("b".to_string(), 2);
        cache.put("c".to_string(), 3);

        // Cache is full, adding new entry should evict "a" (least recently used)
        cache.put("d".to_string(), 4);

        assert_eq!(cache.get(&"a".to_string()), None); // Evicted
        assert_eq!(cache.get(&"b".to_string()), Some(2));
        assert_eq!(cache.get(&"c".to_string()), Some(3));
        assert_eq!(cache.get(&"d".to_string()), Some(4));
    }

    #[test]
    fn lru_cache_access_updates_order() {
        let mut cache = LruCache::new(3);
        cache.put("a".to_string(), 1);
        cache.put("b".to_string(), 2);
        cache.put("c".to_string(), 3);

        // Access "a" to make it most recently used
        let _ = cache.get(&"a".to_string());

        // Adding new entry should evict "b" (now least recently used)
        cache.put("d".to_string(), 4);

        assert_eq!(cache.get(&"a".to_string()), Some(1)); // Still present
        assert_eq!(cache.get(&"b".to_string()), None); // Evicted
    }

    #[test]
    fn lru_cache_remove_works() {
        let mut cache = LruCache::new(10);
        cache.put("key".to_string(), 42);

        let removed = cache.remove(&"key".to_string());
        assert_eq!(removed, Some(42));
        assert!(cache.is_empty());
    }

    #[test]
    fn lru_cache_clear_removes_all() {
        let mut cache = LruCache::new(10);
        cache.put("a".to_string(), 1);
        cache.put("b".to_string(), 2);

        cache.clear();
        assert!(cache.is_empty());
    }

    #[test]
    fn lru_cache_stats_track_hits_and_misses() {
        let mut cache = LruCache::new(10);
        cache.put("key".to_string(), 42);

        let _ = cache.get(&"key".to_string()); // Hit
        let _ = cache.get(&"missing".to_string()); // Miss

        let stats = cache.stats();
        assert_eq!(stats.hits, 1);
        assert_eq!(stats.misses, 1);
    }

    #[test]
    fn lru_cache_stats_track_evictions() {
        let mut cache = LruCache::new(2);
        cache.put("a".to_string(), 1);
        cache.put("b".to_string(), 2);
        cache.put("c".to_string(), 3); // Triggers eviction

        assert_eq!(cache.stats().evictions, 1);
    }

    #[test]
    fn lru_cache_with_config() {
        let config = CacheConfig {
            max_entries: 5,
            default_ttl: Duration::from_secs(60),
            enable_stats: false,
        };
        let cache: LruCache<String, i32> = LruCache::with_config(&config);
        assert_eq!(cache.capacity, 5);
    }

    // ============================================================================
    // TtlCache Tests
    // ============================================================================

    #[test]
    fn ttl_cache_new_creates_empty_cache() {
        let cache: TtlCache<String, i32> = TtlCache::new(Duration::from_secs(60));
        assert!(cache.is_empty());
        assert_eq!(cache.len(), 0);
    }

    #[test]
    fn ttl_cache_put_and_get() {
        let mut cache = TtlCache::new(Duration::from_secs(60));
        cache.put("key".to_string(), 42);

        assert_eq!(cache.get(&"key".to_string()), Some(42));
    }

    #[test]
    fn ttl_cache_get_nonexistent_returns_none() {
        let mut cache: TtlCache<String, i32> = TtlCache::new(Duration::from_secs(60));
        assert_eq!(cache.get(&"nonexistent".to_string()), None);
    }

    #[test]
    fn ttl_cache_remove_works() {
        let mut cache = TtlCache::new(Duration::from_secs(60));
        cache.put("key".to_string(), 42);

        let removed = cache.remove(&"key".to_string());
        assert_eq!(removed, Some(42));
        assert!(cache.is_empty());
    }

    #[test]
    fn ttl_cache_clear_removes_all() {
        let mut cache = TtlCache::new(Duration::from_secs(60));
        cache.put("a".to_string(), 1);
        cache.put("b".to_string(), 2);

        cache.clear();
        assert!(cache.is_empty());
    }

    #[test]
    fn ttl_cache_stats_track_hits_and_misses() {
        let mut cache = TtlCache::new(Duration::from_secs(60));
        cache.put("key".to_string(), 42);

        let _ = cache.get(&"key".to_string()); // Hit
        let _ = cache.get(&"missing".to_string()); // Miss

        let stats = cache.stats();
        assert_eq!(stats.hits, 1);
        assert_eq!(stats.misses, 1);
    }

    #[test]
    fn ttl_cache_with_config() {
        let config = CacheConfig {
            max_entries: 100,
            default_ttl: Duration::from_secs(120),
            enable_stats: true,
        };
        let cache: TtlCache<String, i32> = TtlCache::with_config(&config);
        assert_eq!(cache.default_ttl, Duration::from_secs(120));
    }

    // ============================================================================
    // CacheConfig Tests
    // ============================================================================

    #[test]
    fn cache_config_default_values() {
        let config = CacheConfig::default();
        assert_eq!(config.max_entries, 10000);
        assert_eq!(config.default_ttl, Duration::from_secs(3600));
        assert!(config.enable_stats);
    }
}
