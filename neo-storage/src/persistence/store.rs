use super::{
    read_cache::ReadCacheStatsSnapshot, read_only_store::ReadOnlyStore,
    store_snapshot::StoreSnapshot, write_batch_buffer::WriteBatchStatsSnapshot,
    write_store::WriteStore,
};
use std::any::Any;
use std::sync::Arc;

/// Delegate for OnNewSnapshot event
pub type OnNewSnapshotDelegate = Box<dyn Fn(&dyn Store, Arc<dyn StoreSnapshot>) + Send + Sync>;

/// Observation-only snapshot of the acceleration structures a store owns.
///
/// A backend reports only the components it actually runs. `None` means the
/// backend has no such component, which callers must distinguish from a
/// component that is present but idle.
#[derive(Debug, Clone, Default)]
pub struct StoreAccelerationStats {
    /// Point read cache counters, when this backend runs one.
    pub read_cache: Option<ReadCacheStatsSnapshot>,
    /// Write-batch buffer counters, when this backend buffers writes.
    pub write_buffer: Option<WriteBatchStatsSnapshot>,
}

/// Observation-only counters for one backend cache (block or row).
///
/// Every value is a byte count sampled at call time; nothing here influences
/// caching behaviour.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CacheUsageStats {
    /// Configured capacity in bytes.
    pub capacity: usize,
    /// Bytes currently held by the cache.
    pub usage: usize,
    /// Bytes held by entries that may not be evicted (index/filter pinning).
    pub pinned_usage: usize,
}

/// Observation-only snapshot of the storage-engine caches behind a store.
///
/// Sizes are reported separately from usage so an operator can tell a cold but
/// correctly sized cache from a small one that is thrashing. Optional fields
/// mean the backend could not answer the question, which callers must not read
/// as zero.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StoreCacheStats {
    /// Block cache capacity, usage and pinned usage.
    pub block_cache: CacheUsageStats,
    /// Row cache counters, `None` when the backend runs without a row cache.
    pub row_cache: Option<CacheUsageStats>,
    /// Bytes of SST index blocks, when the backend reports them.
    pub index_size: Option<usize>,
    /// Bytes of SST filter blocks, when the backend reports them.
    pub filter_size: Option<usize>,
    /// Bytes of SST data blocks, when the backend reports them.
    pub data_block_size: Option<usize>,
    /// Backend estimate of memory held by table readers (index + filter blocks
    /// not served from the block cache).
    pub estimate_table_readers_mem: Option<usize>,
}

/// This interface provides methods for reading, writing from/to database.
/// Developers should implement this interface to provide new storage engines for NEO.
pub trait Store: ReadOnlyStore + WriteStore<Vec<u8>, Vec<u8>> + Send + Sync + Any {
    /// Creates a snapshot of the database.
    fn snapshot(&self) -> Arc<dyn StoreSnapshot>;

    /// Event raised when a new snapshot is created
    fn on_new_snapshot(&self, handler: OnNewSnapshotDelegate);

    /// Enables storage-level fast-sync optimizations when supported.
    fn enable_fast_sync_mode(&self) {}

    /// Disables storage-level fast-sync optimizations.
    fn disable_fast_sync_mode(&self) {}

    /// Flushes pending writes to durable storage when supported.
    fn flush(&self) {}

    /// Runs a blocking full-range compaction when the backend supports it.
    ///
    /// This is an operational maintenance hook, not part of the write path: it
    /// merges accumulated sorted tables so later reads stop paying an unbounded
    /// file fan-out. Backends without a compaction concept keep the no-op
    /// default, and callers must treat it as best effort.
    fn compact_all(&self) {}

    /// Returns per-level table file counts, lowest level first, when the
    /// backend exposes them. Used to observe whether compaction keeps up.
    fn level_file_counts(&self) -> Vec<(u32, u64)> {
        Vec::new()
    }

    /// Forces all writes already accepted by the backend to durable storage.
    /// Unlike legacy `flush`, failures are returned so callers can fail closed.
    fn try_flush_durable(&self) -> crate::error::StorageResult<()> {
        Err(crate::error::StorageError::InvalidOperation {
            message: "store backend does not support explicit durable flushes".to_string(),
        })
    }

    /// Returns whether snapshots can force WAL-backed synchronous durability.
    fn supports_durable_writes(&self) -> bool {
        false
    }

    /// Returns a monotonic version of the store's committed contents.
    ///
    /// Caches fronting a mutable store use it to detect commits performed by
    /// other writers and to drop values those commits superseded. Backends that
    /// cannot report a version fall back to `0`, which disables that check.
    fn state_generation(&self) -> u64 {
        0
    }

    /// Downcast support for concrete implementations.
    fn as_any(&self) -> &dyn Any;

    /// Returns acceleration telemetry for the caching structures this backend
    /// actually runs.
    ///
    /// The default reports nothing: a backend without a read cache or a write
    /// buffer must not fabricate counters. Implementations return a snapshot
    /// only; the store's behaviour never depends on who reads it.
    fn acceleration_stats(&self) -> Option<StoreAccelerationStats> {
        None
    }

    /// Returns the storage-engine cache counters behind this store, if the
    /// backend has any.
    ///
    /// The default reports nothing: a backend without block or row caches must
    /// not fabricate counters. Reading this never changes cache contents.
    fn cache_stats(&self) -> Option<StoreCacheStats> {
        None
    }
}
