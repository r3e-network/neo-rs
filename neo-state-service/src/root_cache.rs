//! LRU cache of recently-validated [`StateRoot`]s.
//!
//! Used by the verification pipeline to avoid re-loading state roots
//! that were already validated in the current session. Mirrors the
//! C# `StateService.Storage.StateRootCache` behaviour.

use crate::state_root::StateRoot;
use lru::LruCache;
use neo_primitives::UInt256;
use parking_lot::Mutex;
use std::num::NonZeroUsize;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

/// Default capacity for the state root cache (matches C# default).
pub const DEFAULT_ROOT_CACHE_CAPACITY: usize = 1000;

/// Atomic counters for the cache's hit / miss / insertion / eviction
/// ratios. Useful for observability and for the
/// [`crate::StateRootIngestStats`] snapshot.
#[derive(Debug, Default)]
pub struct StateRootCacheStats {
    /// Cache hits.
    pub hits: AtomicU64,
    /// Cache misses.
    pub misses: AtomicU64,
    /// Cache insertions.
    pub insertions: AtomicU64,
    /// Cache evictions.
    pub evictions: AtomicU64,
}

impl StateRootCacheStats {
    /// Returns a point-in-time snapshot of the counters.
    pub fn snapshot(&self) -> StateRootCacheStatsSnapshot {
        StateRootCacheStatsSnapshot {
            hits: self.hits.load(Ordering::Relaxed),
            misses: self.misses.load(Ordering::Relaxed),
            insertions: self.insertions.load(Ordering::Relaxed),
            evictions: self.evictions.load(Ordering::Relaxed),
        }
    }
}

/// Point-in-time snapshot of [`StateRootCacheStats`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct StateRootCacheStatsSnapshot {
    /// Cache hits at the snapshot time.
    pub hits: u64,
    /// Cache misses at the snapshot time.
    pub misses: u64,
    /// Cache insertions at the snapshot time.
    pub insertions: u64,
    /// Cache evictions at the snapshot time.
    pub evictions: u64,
}

/// Cached state-root entry.
#[derive(Debug, Clone)]
pub struct StateRootCacheEntry {
    /// The cached state root.
    pub root: StateRoot,
}

impl StateRootCacheEntry {
    /// Constructs a new cache entry.
    pub fn new(root: StateRoot) -> Self {
        Self { root }
    }
}

/// LRU cache of validated [`StateRoot`]s keyed by their trie root
/// hash. All operations are O(1) amortised.
pub struct StateRootCache {
    inner: Mutex<LruCache<UInt256, Arc<StateRootCacheEntry>>>,
    capacity: NonZeroUsize,
    stats: Arc<StateRootCacheStats>,
}

impl StateRootCache {
    /// Constructs a new cache with the default capacity.
    pub fn new() -> Self {
        Self::with_capacity(DEFAULT_ROOT_CACHE_CAPACITY)
    }

    /// Constructs a new cache with the given capacity.
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            inner: Mutex::new(LruCache::new(NonZeroUsize::new(capacity.max(1)).unwrap())),
            capacity: NonZeroUsize::new(capacity.max(1)).unwrap(),
            stats: Arc::new(StateRootCacheStats::default()),
        }
    }

    /// Returns the cache capacity.
    pub fn capacity(&self) -> usize {
        self.capacity.get()
    }

    /// Returns a handle to the cache's atomic statistics counters.
    pub fn stats(&self) -> Arc<StateRootCacheStats> {
        Arc::clone(&self.stats)
    }

    /// Looks up a state root by its trie root hash, updating the
    /// hit / miss counters accordingly.
    pub fn get(&self, root_hash: &UInt256) -> Option<Arc<StateRootCacheEntry>> {
        let mut guard = self.inner.lock();
        if let Some(entry) = guard.get(root_hash) {
            self.stats.hits.fetch_add(1, Ordering::Relaxed);
            Some(Arc::clone(entry))
        } else {
            self.stats.misses.fetch_add(1, Ordering::Relaxed);
            None
        }
    }

    /// Inserts a state root into the cache, returning the evicted
    /// entry (if any).
    pub fn insert(&self, root: StateRoot) -> Option<Arc<StateRootCacheEntry>> {
        let hash = *root.root_hash();
        let entry = Arc::new(StateRootCacheEntry::new(root));
        let mut guard = self.inner.lock();
        self.stats.insertions.fetch_add(1, Ordering::Relaxed);
        if let Some((_evicted_key, evicted_value)) = guard.push(hash, entry) {
            self.stats.evictions.fetch_add(1, Ordering::Relaxed);
            Some(evicted_value)
        } else {
            None
        }
    }

    /// Removes a state root from the cache, returning it if present.
    pub fn invalidate(&self, root_hash: &UInt256) -> Option<Arc<StateRootCacheEntry>> {
        self.inner.lock().pop(root_hash)
    }

    /// Returns the number of entries currently in the cache.
    pub fn len(&self) -> usize {
        self.inner.lock().len()
    }

    /// Returns whether the cache is empty.
    pub fn is_empty(&self) -> bool {
        self.inner.lock().is_empty()
    }

    /// Clears all entries from the cache. The stats counters are
    /// preserved.
    pub fn clear(&self) {
        self.inner.lock().clear();
    }
}

neo_io::impl_default_via_new!(StateRootCache);

#[cfg(test)]
#[path = "tests/root_cache.rs"]
mod tests;
