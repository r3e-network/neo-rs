//! Sharded hot-key cache for logical storage items.
//!
//! The RocksDB block cache accelerates reads at the byte level, and
//! [`crate::persistence::read_cache::ReadCache`] accelerates them per
//! [`StorageKey`], but the latter is a single `RwLock<LruCache<..>>`: `LruCache::get`
//! needs `&mut self`, so every hit serialises on one write lock and updates an
//! access clock. Contract execution reads a small set of keys (native
//! governance/state entries, token balances) extremely often, so that single
//! lock is a measurable per-read cost.
//!
//! This cache keeps the same logical unit (a `StorageKey` -> `StorageItem`
//! pair) in a fixed number of independently locked shards. A read locks one
//! shard instead of the global cache, and a `StorageItem` clone is an `Arc`
//! bump, so a hit performs no deserialization at all.
//!
//! Correctness contract:
//! - Only values read from committed backing state may be inserted. The
//!   [`crate::persistence::DataCache`] therefore fills this cache exclusively on
//!   the `store_get` path, never from its write dictionary.
//! - Every write to a cached key invalidates that key, so an entry can never
//!   outlive a value the same execution already replaced.
//! - An overlay clone, which owns speculative writes its parent can never
//!   invalidate, runs without a hot-key cache at all.

use crate::types::{StorageItem, StorageKey};
use lru::LruCache;
use parking_lot::Mutex;
use std::num::NonZeroUsize;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

/// Number of independently locked shards.
pub const HOT_KEY_CACHE_SHARDS: usize = 16;

/// Entries retained by each shard in the default configuration.
pub const HOT_KEY_CACHE_SHARD_CAPACITY: usize = 256;

/// Default total entry bound (`HOT_KEY_CACHE_SHARDS * HOT_KEY_CACHE_SHARD_CAPACITY`).
pub const DEFAULT_HOT_KEY_CACHE_CAPACITY: usize =
    HOT_KEY_CACHE_SHARDS * HOT_KEY_CACHE_SHARD_CAPACITY;

/// Observation-only counters for the hot-key cache.
#[derive(Debug, Default)]
pub struct HotKeyCacheStats {
    hits: AtomicU64,
    misses: AtomicU64,
    evictions: AtomicU64,
    invalidations: AtomicU64,
}

impl HotKeyCacheStats {
    #[inline]
    fn record_hit(&self) {
        self.hits.fetch_add(1, Ordering::Relaxed);
    }

    #[inline]
    fn record_miss(&self) {
        self.misses.fetch_add(1, Ordering::Relaxed);
    }

    #[inline]
    fn record_eviction(&self) {
        self.evictions.fetch_add(1, Ordering::Relaxed);
    }

    #[inline]
    fn record_invalidation(&self) {
        self.invalidations.fetch_add(1, Ordering::Relaxed);
    }
}

/// Point-in-time view of the hot-key cache.
///
/// `Default` is the zero snapshot a caller uses when no cache is running, so a
/// missing cache and an idle cache stay distinguishable by the caller's own
/// presence flag rather than by guessed values.
#[derive(Debug, Clone, Copy, Default)]
pub struct HotKeyCacheStatsSnapshot {
    /// Lookups served from a shard.
    pub hits: u64,
    /// Lookups that had to fall through to the next cache level.
    pub misses: u64,
    /// Entries dropped to honour the per-shard bound.
    pub evictions: u64,
    /// Entries removed because the key was written in this execution.
    pub invalidations: u64,
    /// Resident entries at snapshot time.
    pub retained_entries: usize,
    /// Effective total entry bound.
    pub capacity: usize,
    /// Effective per-shard entry bound.
    pub shard_capacity: usize,
    /// Number of shards in use.
    pub shards: usize,
}

impl HotKeyCacheStatsSnapshot {
    /// Fraction of lookups served from the cache.
    pub fn hit_rate(&self) -> f64 {
        let total = self.hits + self.misses;
        if total == 0 {
            0.0
        } else {
            self.hits as f64 / total as f64
        }
    }
}

/// Bounded, sharded LRU of committed storage items.
pub struct HotKeyCache {
    shards: Box<[Mutex<LruCache<StorageKey, StorageItem>>]>,
    stats: HotKeyCacheStats,
    retained_entries: AtomicUsize,
    capacity: usize,
    shard_capacity: usize,
}

impl Default for HotKeyCache {
    fn default() -> Self {
        Self::new(DEFAULT_HOT_KEY_CACHE_CAPACITY, HOT_KEY_CACHE_SHARDS)
    }
}

impl HotKeyCache {
    /// Creates a cache bounded to `capacity` entries spread over at most
    /// `shard_count` shards.
    ///
    /// The shard count is clamped to the entry bound and the per-shard bound is
    /// rounded down, so `effective_capacity <= capacity` holds for every input,
    /// including tiny bounds used by tests.
    pub fn new(capacity: usize, shard_count: usize) -> Self {
        let capacity = capacity.max(1);
        let shard_count = shard_count.clamp(1, capacity);
        let shard_capacity = (capacity / shard_count).max(1);
        let shards = (0..shard_count)
            .map(|_| {
                Mutex::new(LruCache::new(
                    NonZeroUsize::new(shard_capacity).expect("shard capacity is at least one"),
                ))
            })
            .collect::<Vec<_>>();
        Self {
            shards: shards.into_boxed_slice(),
            stats: HotKeyCacheStats::default(),
            retained_entries: AtomicUsize::new(0),
            capacity: shard_capacity * shard_count,
            shard_capacity,
        }
    }

    /// Looks a committed item up, promoting it inside its shard.
    pub fn get(&self, key: &StorageKey) -> Option<StorageItem> {
        let mut shard = self.shards[self.shard_index(key)].lock();
        match shard.get(key) {
            Some(item) => {
                let item = item.clone();
                drop(shard);
                self.stats.record_hit();
                Some(item)
            }
            None => {
                drop(shard);
                self.stats.record_miss();
                None
            }
        }
    }

    /// Retains a committed item read from backing storage.
    pub fn insert(&self, key: StorageKey, item: StorageItem) {
        let mut shard = self.shards[self.shard_index(&key)].lock();
        let displaced = shard.push(key, item);
        drop(shard);
        if displaced.is_some() {
            self.stats.record_eviction();
        } else {
            self.retained_entries.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Drops a key because the current execution wrote it.
    pub fn invalidate(&self, key: &StorageKey) {
        let mut shard = self.shards[self.shard_index(key)].lock();
        let removed = shard.pop(key);
        drop(shard);
        if removed.is_some() {
            self.decrement_retained();
            self.stats.record_invalidation();
        }
    }

    /// Drops every retained entry.
    pub fn clear(&self) {
        for shard in self.shards.iter() {
            shard.lock().clear();
        }
        self.retained_entries.store(0, Ordering::Relaxed);
    }

    /// Number of retained entries.
    pub fn len(&self) -> usize {
        self.retained_entries.load(Ordering::Relaxed)
    }

    /// Returns true when no entry is retained.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Effective total entry bound.
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// Effective per-shard entry bound.
    pub fn shard_capacity(&self) -> usize {
        self.shard_capacity
    }

    /// Returns a snapshot of the cache telemetry.
    pub fn stats_snapshot(&self) -> HotKeyCacheStatsSnapshot {
        HotKeyCacheStatsSnapshot {
            hits: self.stats.hits.load(Ordering::Relaxed),
            misses: self.stats.misses.load(Ordering::Relaxed),
            evictions: self.stats.evictions.load(Ordering::Relaxed),
            invalidations: self.stats.invalidations.load(Ordering::Relaxed),
            retained_entries: self.len(),
            capacity: self.capacity,
            shard_capacity: self.shard_capacity,
            shards: self.shards.len(),
        }
    }

    #[inline]
    fn shard_index(&self, key: &StorageKey) -> usize {
        // `hash_code` is the same cheap xxhash3 the key already computes for
        // hash-map placement, so shard selection costs one extra mask.
        (key.hash_code() as u32 as usize) % self.shards.len()
    }

    #[inline]
    fn decrement_retained(&self) {
        let _ = self
            .retained_entries
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
                Some(current.saturating_sub(1))
            });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(suffix: u16) -> StorageKey {
        StorageKey::new(7, suffix.to_le_bytes().to_vec())
    }

    fn item(value: u8) -> StorageItem {
        StorageItem::from_bytes(vec![value])
    }

    #[test]
    fn repeated_reads_hit_after_the_first_fill() {
        let cache = HotKeyCache::new(64, 4);
        assert!(cache.get(&key(1)).is_none());

        cache.insert(key(1), item(0xaa));

        let first = cache.get(&key(1)).expect("value is retained");
        let second = cache.get(&key(1)).expect("value is retained");
        assert_eq!(first.value_bytes().as_ref(), &[0xaa]);
        assert_eq!(second.value_bytes().as_ref(), &[0xaa]);

        let stats = cache.stats_snapshot();
        assert_eq!(stats.hits, 2);
        assert_eq!(stats.misses, 1);
        assert_eq!(stats.retained_entries, 1);
    }

    #[test]
    fn entry_bound_never_exceeds_capacity() {
        let cache = HotKeyCache::new(64, 8);
        assert_eq!(cache.capacity(), 64);

        for suffix in 0..512u16 {
            cache.insert(key(suffix), item(0x01));
        }

        assert!(
            cache.len() <= 64,
            "retained {} entries for a 64 entry bound",
            cache.len()
        );
        let stats = cache.stats_snapshot();
        assert_eq!(stats.capacity, 64);
        assert_eq!(stats.retained_entries, cache.len());
        assert!(stats.evictions >= 512 - 64);
    }

    #[test]
    fn tiny_capacity_keeps_a_hard_bound() {
        let cache = HotKeyCache::new(3, 16);
        for suffix in 0..32u16 {
            cache.insert(key(suffix), item(0x02));
        }
        assert!(cache.capacity() <= 3);
        assert!(cache.len() <= 3);
    }

    #[test]
    fn invalidate_drops_the_value_and_counts_one_removal() {
        let cache = HotKeyCache::new(16, 4);
        cache.insert(key(9), item(0x11));

        cache.invalidate(&key(9));

        assert!(cache.get(&key(9)).is_none());
        assert!(cache.is_empty());
        assert_eq!(cache.stats_snapshot().invalidations, 1);
    }

    #[test]
    fn invalidate_of_an_absent_key_is_not_counted() {
        let cache = HotKeyCache::new(16, 4);
        cache.invalidate(&key(1234));
        let stats = cache.stats_snapshot();
        assert_eq!(stats.invalidations, 0);
        assert_eq!(stats.retained_entries, 0);
    }

    #[test]
    fn clear_empties_every_shard() {
        let cache = HotKeyCache::new(64, 4);
        for suffix in 0..64u16 {
            cache.insert(key(suffix), item(0x03));
        }
        assert!(!cache.is_empty());

        cache.clear();

        assert!(cache.is_empty());
        assert_eq!(cache.stats_snapshot().retained_entries, 0);
    }

    #[test]
    fn shards_are_addressed_by_the_key_hash() {
        let cache = HotKeyCache::new(64, 8);
        let mut used = std::collections::HashSet::new();
        for suffix in 0..64u16 {
            used.insert(cache.shard_index(&key(suffix)));
        }
        assert!(
            used.len() > 1,
            "all keys landed in {used:?}; sharding would not reduce contention"
        );
    }
}
