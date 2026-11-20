use crate::types::{StorageItem, StorageKey};
use lru::LruCache;
use parking_lot::RwLock;
use std::hash::Hash;
use std::num::NonZeroUsize;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::{Duration, Instant};
use tracing::{debug, trace};

mod bloom_filter;
mod prefetch;
pub use bloom_filter::{BloomFilter, BloomFilterKey, NegativeLookupBloom};
pub use prefetch::PrefetchHint;

/// A single entry may occupy at most this fraction of the byte budget.
///
/// Admitting a larger value would evict the whole working set for a value that
/// cannot stay resident anyway, so such entries are rejected up front instead of
/// being inserted after every other entry was dropped.
const MAX_ENTRY_BUDGET_DIVISOR: usize = 4;

/// Outcome of admitting a speculative batch into the read cache.
///
/// The caller needs this to report what a prefetch actually bought: the cache
/// stays silent about dropped rows otherwise, and a prefetch whose whole batch
/// is stale would look identical to one that admitted everything.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PrefetchAdmission {
    /// Keys that entered the cache.
    pub admitted: usize,
    /// Keys dropped because a fresher entry was already resident.
    pub stale_skips: usize,
    /// Keys the cache refused (oversized, or over budget after eviction).
    pub rejected: usize,
    /// Value bytes of the admitted keys.
    pub bytes: usize,
}

/// Cache entry with metadata.
#[derive(Debug, Clone)]
struct CacheEntry<V> {
    value: V,
    access_count: u64,
    last_access: Instant,
    size_bytes: usize,
    /// Cache generation observed when this entry was written.
    ///
    /// Speculative (pre-fetched) writes carry the generation of the read that
    /// produced them, so a resident entry with a newer generation is known to be
    /// at least as fresh as the batch that wants to replace it.
    generation: u64,
}

impl<V> CacheEntry<V> {
    fn new(value: V, size_bytes: usize, generation: u64) -> Self {
        Self {
            value,
            access_count: 1,
            last_access: Instant::now(),
            size_bytes,
            generation,
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
    rejected_oversized: AtomicU64,
    rejected_budget: AtomicU64,
    prefetch_stale_skips: AtomicU64,
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
        self.record_cache_eviction(bytes, true);
    }

    /// Records a prefetch.
    #[inline]
    pub fn record_prefetch(&self, count: usize, bytes: usize) {
        self.record_prefetch_batch(count, count, 0, bytes, true);
    }

    /// Records a prefetch hit.
    #[inline]
    pub fn record_prefetch_hit(&self) {
        self.prefetch_hits.fetch_add(1, Ordering::Relaxed);
    }

    /// Records an insert.
    #[inline]
    pub fn record_insert(&self, bytes: usize) {
        self.record_cache_write(None, bytes, true);
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

    /// Records an insert rejected because the entry alone is larger than the
    /// per-entry share of the byte budget.
    #[inline]
    pub fn record_rejected_oversized(&self) {
        self.rejected_oversized.fetch_add(1, Ordering::Relaxed);
    }

    /// Records an insert rejected because the byte budget was still exhausted
    /// after evicting candidates.
    #[inline]
    pub fn record_rejected_budget(&self) {
        self.rejected_budget.fetch_add(1, Ordering::Relaxed);
    }

    /// Records a pre-fetched value dropped because a fresher entry is resident.
    #[inline]
    pub fn record_prefetch_stale_skip(&self) {
        self.prefetch_stale_skips.fetch_add(1, Ordering::Relaxed);
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
            rejected_oversized: self.rejected_oversized.load(Ordering::Relaxed),
            rejected_budget: self.rejected_budget.load(Ordering::Relaxed),
            prefetch_stale_skips: self.prefetch_stale_skips.load(Ordering::Relaxed),
        }
    }

    #[inline]
    fn record_cache_write(&self, old_bytes: Option<usize>, new_bytes: usize, enable_stats: bool) {
        if enable_stats {
            self.inserts.fetch_add(1, Ordering::Relaxed);
        }

        match old_bytes {
            Some(old_bytes) => self.replace_current_bytes(old_bytes, new_bytes),
            None => {
                self.current_entries.fetch_add(1, Ordering::Relaxed);
                self.current_bytes.fetch_add(new_bytes, Ordering::Relaxed);
            }
        }
    }

    #[inline]
    fn record_prefetch_batch(
        &self,
        count: usize,
        new_entries: usize,
        old_bytes: usize,
        new_bytes: usize,
        enable_stats: bool,
    ) {
        if enable_stats {
            self.prefetches.fetch_add(count as u64, Ordering::Relaxed);
            self.inserts.fetch_add(count as u64, Ordering::Relaxed);
        }

        if new_entries > 0 {
            self.current_entries
                .fetch_add(new_entries, Ordering::Relaxed);
        }
        self.replace_current_bytes(old_bytes, new_bytes);
    }

    #[inline]
    fn record_prefetch_events(&self, count: usize, enable_stats: bool) {
        if enable_stats {
            self.prefetches.fetch_add(count as u64, Ordering::Relaxed);
        }
    }

    #[inline]
    fn record_cache_eviction(&self, bytes: usize, enable_stats: bool) {
        if enable_stats {
            self.evictions.fetch_add(1, Ordering::Relaxed);
        }
        self.record_cache_removal(bytes);
    }

    #[inline]
    fn record_cache_removal(&self, bytes: usize) {
        self.current_entries.fetch_sub(1, Ordering::Relaxed);
        self.current_bytes.fetch_sub(bytes, Ordering::Relaxed);
    }

    #[inline]
    fn replace_current_bytes(&self, old_bytes: usize, new_bytes: usize) {
        if new_bytes >= old_bytes {
            self.current_bytes
                .fetch_add(new_bytes - old_bytes, Ordering::Relaxed);
        } else {
            self.current_bytes
                .fetch_sub(old_bytes - new_bytes, Ordering::Relaxed);
        }
    }
}

/// Snapshot of read cache statistics.
///
/// `Default` is the zero snapshot a caller uses when no cache is running, so a
/// missing cache and an idle cache stay distinguishable by the caller's own
/// presence flag rather than by guessed values.
#[derive(Debug, Clone, Copy, Default)]
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
    /// Inserts rejected because the entry alone exceeded the per-entry budget.
    pub rejected_oversized: u64,
    /// Inserts rejected because the byte budget stayed exhausted after eviction.
    pub rejected_budget: u64,
    /// Pre-fetched values dropped because a fresher entry was already resident.
    pub prefetch_stale_skips: u64,
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

/// LRU Read cache for storage operations with bloom filter support.
pub struct ReadCache<K, V>
where
    K: Clone + Eq + Hash + BloomFilterKey,
    V: Clone,
{
    config: ReadCacheConfig,
    data: RwLock<LruCache<K, CacheEntry<V>>>,
    stats: Arc<ReadCacheStats>,
    bloom_filter: Option<Arc<NegativeLookupBloom>>,
    /// Monotonic counter advanced by every authoritative write.
    ///
    /// It is the freshness clock that lets speculative batch inserts recognise
    /// entries written after the read that produced their payload.
    generation: AtomicU64,
}

impl<K, V> ReadCache<K, V>
where
    K: Clone + Eq + Hash + BloomFilterKey,
    V: Clone,
{
    /// Creates a new read cache with the specified configuration.
    pub fn new(config: ReadCacheConfig) -> Self {
        let bloom_filter = if config.enable_bloom_filter {
            Some(Arc::new(NegativeLookupBloom::new(
                config.bloom_filter_capacity,
                config.bloom_filter_fpr,
            )))
        } else {
            None
        };

        Self {
            config,
            data: RwLock::new(LruCache::new(
                NonZeroUsize::new(config.max_entries.max(1))
                    .expect("read cache capacity is clamped to at least one entry"),
            )),
            stats: Arc::new(ReadCacheStats::new()),
            bloom_filter,
            generation: AtomicU64::new(0),
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
        self.bloom_filter
            .as_ref()
            .is_none_or(|b| b.might_contain_hash(key.hash_for_bloom()))
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

        if let Some(entry) = data.peek(key) {
            // Check TTL
            if let Some(ttl) = self.config.ttl
                && entry.last_access.elapsed() > ttl
            {
                // Entry expired
                let entry = data.pop(key).expect("peeked cache entry must exist");
                self.stats
                    .record_cache_eviction(entry.size_bytes, self.config.enable_stats);

                if self.config.enable_stats {
                    self.stats.record_miss();
                }
                return None;
            }
        }

        if let Some(entry) = data.get_mut(key) {
            // Update entry
            entry.record_access();
            let value = entry.value.clone();

            if self.config.enable_stats {
                self.stats.record_hit();
            }

            trace!(target: "neo", "cache hit");
            Some(value)
        } else {
            if self.config.enable_stats {
                self.stats.record_miss();
            }

            trace!(target: "neo", "cache miss");
            None
        }
    }

    /// Puts a value into the cache.
    ///
    /// Entries that cannot reasonably fit the byte budget are rejected before
    /// any eviction runs: admitting them would drop every resident entry and
    /// still leave the cache over budget.
    pub fn put(&self, key: K, value: V, size_bytes: usize) {
        if !self.admit_entry_size(size_bytes) {
            return;
        }

        let generation = self.next_generation();
        let mut data = self.data.write();
        if data.peek(&key).is_some() {
            let _ = data.get_mut(&key);
        }

        // Check if adding this would exceed byte limit
        while self.projected_bytes_after_put(&data, &key, size_bytes) > self.config.max_bytes
            && !data.is_empty()
        {
            if !self.evict_lru(&mut data) {
                break;
            }
        }

        // Eviction may be unable to free enough room, so verify the budget again
        // instead of inserting an entry that leaves the cache over budget.
        if self.projected_bytes_after_put(&data, &key, size_bytes) > self.config.max_bytes {
            self.stats.record_rejected_budget();
            debug!(
                target: "neo",
                size_bytes,
                max_bytes = self.config.max_bytes,
                "cache insert rejected: byte budget still exhausted after eviction"
            );
            return;
        }

        // Insert new entry
        let entry = CacheEntry::new(value, size_bytes, generation);
        let key_for_bloom = key.clone();
        let is_new = self.push_cache_entry(&mut data, key, entry);

        // Update bloom filter for new entries
        if is_new && let Some(ref bloom) = self.bloom_filter {
            bloom.insert_hash(key_for_bloom.hash_for_bloom());
        }

        trace!(target: "neo", size_bytes, "cache insert");
    }

    /// Puts multiple values into the cache (for pre-fetching).
    ///
    /// Pre-fetched values are speculative: they may fill misses, but they must
    /// never replace an entry that is already resident. The decision is taken
    /// before any eviction, because an entry evicted to make room for the batch
    /// would otherwise be repopulated with a stale pre-fetched value.
    pub fn put_batch(&self, items: Vec<(K, V, usize)>) -> PrefetchAdmission {
        self.put_batch_with_generation(items, self.generation())
    }

    /// Puts pre-fetched values that were read at `source_generation`.
    ///
    /// Resident entries written after `source_generation` are fresher than the
    /// batch and are left untouched. The returned [`PrefetchAdmission`] reports
    /// how the batch was partitioned so the caller can publish prefetch metrics.
    pub fn put_batch_with_generation(
        &self,
        items: Vec<(K, V, usize)>,
        source_generation: u64,
    ) -> PrefetchAdmission {
        let mut report = PrefetchAdmission::default();
        if items.is_empty() {
            return report;
        }

        let count = items.len();
        let total_bytes: usize = items.iter().map(|(_, _, size)| size).sum();

        let mut data = self.data.write();

        // Partition the batch before touching the cache: keys already present are
        // protected for the whole operation and oversized values are dropped
        // instead of triggering evictions.
        let mut accepted: Vec<(K, V, usize)> = Vec::with_capacity(count);
        let mut accepted_bytes = 0usize;
        for (key, value, size_bytes) in items {
            if let Some(existing) = data.get_mut(&key) {
                if existing.generation > source_generation {
                    report.stale_skips += 1;
                    self.stats.record_prefetch_stale_skip();
                } else {
                    // A resident entry that is not fresher is still resident:
                    // speculative rows never overwrite it either way.
                    report.rejected += 1;
                }
                continue;
            }
            if !self.admit_entry_size(size_bytes) {
                report.rejected += 1;
                continue;
            }
            accepted_bytes = accepted_bytes.saturating_add(size_bytes);
            accepted.push((key, value, size_bytes));
        }

        // Only reclaim what the batch can actually consume; a batch larger than
        // the whole budget must not evict everything and still be rejected.
        let reclaim_target = accepted_bytes.min(self.config.max_bytes);
        while self
            .stats
            .current_bytes
            .load(Ordering::Relaxed)
            .saturating_add(reclaim_target)
            > self.config.max_bytes
            && !data.is_empty()
        {
            if !self.evict_lru(&mut data) {
                break;
            }
        }

        let mut keys_for_bloom = Vec::with_capacity(accepted.len());

        for (key, value, size_bytes) in accepted {
            // Never insert past the budget, even when eviction could not free
            // enough room for this particular value.
            if self
                .stats
                .current_bytes
                .load(Ordering::Relaxed)
                .saturating_add(size_bytes)
                > self.config.max_bytes
            {
                self.stats.record_rejected_budget();
                report.rejected += 1;
                continue;
            }

            let entry = CacheEntry::new(value, size_bytes, source_generation);
            let key_for_bloom = key.clone();
            if self.push_cache_entry(&mut data, key, entry) {
                keys_for_bloom.push(key_for_bloom);
            }
            report.admitted += 1;
            report.bytes = report.bytes.saturating_add(size_bytes);
        }

        // Update bloom filter
        if let Some(ref bloom) = self.bloom_filter {
            for key in keys_for_bloom {
                bloom.insert_hash(key.hash_for_bloom());
            }
        }

        self.stats
            .record_prefetch_events(count, self.config.enable_stats);

        debug!(
            target: "neo",
            count,
            total_bytes,
            admitted = report.admitted,
            stale_skips = report.stale_skips,
            "cache batch insert (prefetch)"
        );

        report
    }

    /// Removes a value from the cache.
    pub fn remove(&self, key: &K) -> Option<V> {
        let mut data = self.data.write();

        if let Some(entry) = data.pop(key) {
            self.stats.record_cache_removal(entry.size_bytes);

            Some(entry.value)
        } else {
            None
        }
    }

    /// Clears the cache.
    pub fn clear(&self) {
        let mut data = self.data.write();

        data.clear();

        if let Some(ref bloom) = self.bloom_filter {
            bloom.clear();
        }

        self.stats.current_entries.store(0, Ordering::Relaxed);
        self.stats.current_bytes.store(0, Ordering::Relaxed);

        debug!(target: "neo", "cache cleared");
    }

    /// Checks if the cache contains a key.
    pub fn contains(&self, key: &K) -> bool {
        // Check bloom filter first
        if !self.fast_bloom_check(key) {
            return false;
        }
        self.data.read().peek(key).is_some()
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
        if let Some(entry) = data.peek(key) {
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

    fn projected_bytes_after_put(
        &self,
        data: &LruCache<K, CacheEntry<V>>,
        key: &K,
        size_bytes: usize,
    ) -> usize {
        let current_bytes = self.stats.current_bytes.load(Ordering::Relaxed);
        let old_bytes = data.peek(key).map_or(0, |entry| entry.size_bytes);
        current_bytes
            .saturating_sub(old_bytes)
            .saturating_add(size_bytes)
    }

    /// Returns the current freshness generation of the cache.
    pub fn generation(&self) -> u64 {
        self.generation.load(Ordering::Acquire)
    }

    /// Advances the freshness generation and returns the new value.
    fn next_generation(&self) -> u64 {
        self.generation.fetch_add(1, Ordering::AcqRel) + 1
    }

    /// Returns the largest single entry the byte budget accepts.
    fn max_entry_bytes(&self) -> usize {
        self.config.max_bytes / MAX_ENTRY_BUDGET_DIVISOR
    }

    /// Returns true when a single entry may enter the cache at all.
    ///
    /// Rejecting oversized values up front keeps a single cold value from
    /// evicting the working set it could never replace.
    fn admit_entry_size(&self, size_bytes: usize) -> bool {
        if size_bytes <= self.max_entry_bytes() {
            return true;
        }

        self.stats.record_rejected_oversized();
        debug!(
            target: "neo",
            size_bytes,
            max_entry_bytes = self.max_entry_bytes(),
            "cache insert rejected: entry exceeds the per-entry byte budget"
        );
        false
    }

    /// Evicts the least recently used entry.
    fn evict_lru(&self, data: &mut LruCache<K, CacheEntry<V>>) -> bool {
        if let Some((_key, entry)) = data.pop_lru() {
            self.stats
                .record_cache_eviction(entry.size_bytes, self.config.enable_stats);

            trace!(target: "neo", "cache eviction");
            true
        } else {
            false
        }
    }

    fn push_cache_entry(
        &self,
        data: &mut LruCache<K, CacheEntry<V>>,
        key: K,
        entry: CacheEntry<V>,
    ) -> bool {
        let existed = data.peek(&key).is_some();
        let new_size_bytes = entry.size_bytes;

        match data.push(key, entry) {
            Some((_key, old_entry)) if existed => {
                self.stats.record_cache_write(
                    Some(old_entry.size_bytes),
                    new_size_bytes,
                    self.config.enable_stats,
                );
                false
            }
            Some((_key, evicted_entry)) => {
                self.stats
                    .record_cache_eviction(evicted_entry.size_bytes, self.config.enable_stats);
                self.stats
                    .record_cache_write(None, new_size_bytes, self.config.enable_stats);
                trace!(target: "neo", "cache eviction");
                true
            }
            None => {
                self.stats
                    .record_cache_write(None, new_size_bytes, self.config.enable_stats);
                true
            }
        }
    }
}

/// Type alias for the storage read cache.
pub type StorageReadCache = ReadCache<StorageKey, StorageItem>;
