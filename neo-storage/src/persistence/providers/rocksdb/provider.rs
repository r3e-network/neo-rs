use crate::error::{StorageError, StorageResult};
use crate::persistence::{
    read_cache::ReadCacheConfig,
    storage::{CompactionStrategy, CompressionAlgorithm, StorageConfig},
    store::{CacheUsageStats, Store},
    store_provider::StoreProvider,
    write_batch_buffer::{WriteBatchBuffer, WriteBatchConfig},
};
use rocksdb::{
    BlockBasedOptions, Cache, DB, DBIteratorWithThreadMode, DataBlockIndexType, Direction,
    IteratorMode, Options, ReadOptions, Snapshot as DbSnapshot,
};
use std::{path::PathBuf, sync::Arc};

use super::store::RocksDbStore;

pub use crate::persistence::write_batch_buffer::{
    WriteBatchConfig as BatchCommitConfig, WriteBatchStats as BatchCommitStats,
    WriteBatchStatsSnapshot as BatchCommitStatsSnapshot,
};

/// Commits buffered writes to RocksDB in batches; exposes aggregate stats.
pub struct BatchCommitter {
    pub(crate) buffer: WriteBatchBuffer,
}

impl BatchCommitter {
    pub(crate) fn new(db: Arc<DB>, config: WriteBatchConfig) -> Self {
        let buffer = WriteBatchBuffer::new(db, config);

        Self { buffer }
    }
}

/// Live handles to the caches one RocksDB instance was opened with.
///
/// RocksDB keeps its own references to both caches, so retaining these costs a
/// pointer bump and is the only way to read row-cache counters: no RocksDB
/// property reports row-cache usage, and the block-cache properties cannot say
/// which role (data block, index, filter) an entry belongs to.
#[derive(Clone)]
pub(crate) struct CacheHandles {
    block_cache: Cache,
    block_cache_capacity: usize,
    row_cache: Option<Cache>,
    row_cache_capacity: usize,
}

impl CacheHandles {
    /// Block cache capacity, usage and pinned usage as the database sees them.
    pub(crate) fn block_cache_usage(&self) -> CacheUsageStats {
        CacheUsageStats {
            capacity: self.block_cache_capacity,
            usage: self.block_cache.get_usage(),
            pinned_usage: self.block_cache.get_pinned_usage(),
        }
    }

    /// Row cache counters, `None` when this database runs without a row cache.
    ///
    /// `rocksdb` 0.21 exposes no row-cache hit/miss counters, so usage here is
    /// the cache's own accounting rather than a lookup statistic.
    pub(crate) fn row_cache_usage(&self) -> Option<CacheUsageStats> {
        self.row_cache.as_ref().map(|cache| CacheUsageStats {
            capacity: self.row_cache_capacity,
            usage: cache.get_usage(),
            pinned_usage: cache.get_pinned_usage(),
        })
    }
}

/// Resolves the row cache capacity from the operator setting and the block
/// cache size.
///
/// `None` keeps the historical derivation (a quarter of the block cache,
/// clamped to 64-512 MiB) so existing deployments size identically. `Some(0)`
/// means "no row cache", and any other explicit value is used verbatim: an
/// operator who names a size must not silently get a clamped one.
pub(crate) fn resolve_row_cache_capacity(
    block_cache_capacity: usize,
    configured: Option<usize>,
) -> usize {
    match configured {
        None => (block_cache_capacity / 4).clamp(64 * 1024 * 1024, 512 * 1024 * 1024),
        Some(bytes) => bytes,
    }
}

/// RocksDB-backed store provider compatible with Neo's `Store`.
#[derive(Debug, Clone)]
pub struct RocksDBStoreProvider {
    base_config: StorageConfig,
    batch_config: BatchCommitConfig,
    batch_stats: Arc<BatchCommitStats>,
    /// Read cache configuration
    read_cache_config: Option<ReadCacheConfig>,
    /// Enable bloom filters for SST files
    enable_bloom_filters: bool,
    /// Enable read-ahead for sequential scans
    enable_read_ahead: bool,
    /// Verify block checksums on every read (safety default).
    verify_checksums: bool,
}

impl RocksDBStoreProvider {
    /// Creates a provider with balanced batching and the default read cache.
    pub fn new(base_config: StorageConfig) -> Self {
        Self {
            base_config,
            batch_config: BatchCommitConfig::durable(),
            batch_stats: Arc::new(BatchCommitStats::new()),
            read_cache_config: Some(ReadCacheConfig::default()),
            enable_bloom_filters: true,
            enable_read_ahead: true,
            verify_checksums: true,
        }
    }

    /// Overrides the write-batching configuration.
    pub fn with_batch_config(mut self, config: BatchCommitConfig) -> Self {
        self.batch_config = config;
        self
    }

    /// Installs a point read cache with the given configuration.
    pub fn with_read_cache(mut self, config: ReadCacheConfig) -> Self {
        self.read_cache_config = Some(config);
        self
    }

    /// Disables the point read cache.
    pub fn without_read_cache(mut self) -> Self {
        self.read_cache_config = None;
        self
    }

    /// Enables or disables SST bloom filters.
    pub fn with_bloom_filters(mut self, enable: bool) -> Self {
        self.enable_bloom_filters = enable;
        self
    }

    /// Enables or disables read-ahead for sequential scans.
    pub fn with_read_ahead(mut self, enable: bool) -> Self {
        self.enable_read_ahead = enable;
        self
    }

    /// Enables or disables block checksum verification on reads.
    ///
    /// Verification is on by default. Disabling it trades corruption detection
    /// for scan throughput and must only be used on trusted, non-consensus data.
    pub fn with_checksum_verification(mut self, enable: bool) -> Self {
        self.verify_checksums = enable;
        self
    }

    /// Returns a shared handle to the batch commit statistics.
    pub fn batch_stats(&self) -> Arc<BatchCommitStats> {
        Arc::clone(&self.batch_stats)
    }

    fn resolved_path(&self, override_path: &str) -> PathBuf {
        if override_path.is_empty() {
            self.base_config.path.clone()
        } else {
            PathBuf::from(override_path)
        }
    }

    /// Opens the concrete storage-owned store for compatibility facades.
    pub fn get_rocksdb_store(&self, path: &str) -> StorageResult<RocksDbStore> {
        let resolved = self.resolved_path(path);
        let config = StorageConfig {
            path: resolved,
            ..self.base_config.clone()
        };
        let mut store = RocksDbStore::open(
            &config,
            self.batch_config,
            &self.read_cache_config,
            self.enable_bloom_filters,
            self.enable_read_ahead,
        )
        .map_err(|err| StorageError::Io {
            message: format!(
                "failed to open RocksDB store at {}: {err}",
                config.path.display()
            ),
        })?;
        store.set_verify_checksums(self.verify_checksums);
        Ok(store)
    }
}

impl StoreProvider for RocksDBStoreProvider {
    fn name(&self) -> &str {
        "RocksDBStore"
    }

    fn get_store(&self, path: &str) -> StorageResult<Arc<dyn Store>> {
        Ok(Arc::new(self.get_rocksdb_store(path)?))
    }
}

/// Read-ahead configuration for sequential scans.
#[derive(Debug, Clone, Copy)]
pub struct ReadAheadConfig {
    /// Enable read-ahead
    pub enabled: bool,
    /// Read-ahead size in bytes
    pub read_ahead_size: usize,
    /// Verify checksums during iteration
    pub verify_checksums: bool,
    /// Fill cache during read-ahead
    pub fill_cache: bool,
}

impl Default for ReadAheadConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            read_ahead_size: 256 * 1024, // 256KB
            // Verification is on by default: a corrupt block that is read as
            // data silently forks consensus state. Throughput-sensitive callers
            // opt out explicitly via `with_checksum_verification(false)`.
            verify_checksums: true,
            fill_cache: true,
        }
    }
}

/// Build read options with read-ahead configuration.
pub(crate) fn build_read_options(
    snapshot: Option<&DbSnapshot>,
    read_ahead_config: &ReadAheadConfig,
) -> ReadOptions {
    let mut options = ReadOptions::default();

    if let Some(snap) = snapshot {
        options.set_snapshot(snap);
    }

    if read_ahead_config.enabled {
        // Enable read-ahead for sequential scans
        options.set_readahead_size(read_ahead_config.read_ahead_size);
    }

    options.set_verify_checksums(read_ahead_config.verify_checksums);
    options.fill_cache(read_ahead_config.fill_cache);

    options
}

pub(crate) fn iterator_from<'a>(
    db: &'a DB,
    read_options: Option<ReadOptions>,
    key_or_prefix: &[u8],
    direction: crate::persistence::seek_direction::SeekDirection,
    read_ahead_config: &ReadAheadConfig,
) -> DBIteratorWithThreadMode<'a, DB> {
    use crate::persistence::seek_direction::SeekDirection;

    let mode = match direction {
        SeekDirection::Forward => {
            if key_or_prefix.is_empty() {
                IteratorMode::Start
            } else {
                IteratorMode::From(key_or_prefix, Direction::Forward)
            }
        }
        SeekDirection::Backward => {
            if key_or_prefix.is_empty() {
                IteratorMode::End
            } else {
                IteratorMode::From(key_or_prefix, Direction::Reverse)
            }
        }
    };

    let opts = read_options.unwrap_or_else(|| build_read_options(None, read_ahead_config));
    db.iterator_opt(mode, opts)
}

/// Returns the smallest key that is strictly greater than all keys with the
/// given prefix. Increments the last non-`0xFF` byte and truncates trailing
/// `0xFF` bytes. Returns `None` when every byte is `0xFF` (no upper bound).
fn prefix_successor(prefix: &[u8]) -> Option<Vec<u8>> {
    let mut result = prefix.to_vec();
    for i in (0..result.len()).rev() {
        if result[i] < 0xFF {
            result[i] += 1;
            result.truncate(i + 1);
            return Some(result);
        }
    }
    // All bytes are 0xFF — no finite upper bound exists.
    None
}

pub(crate) fn reverse_prefix_iterator<'a>(
    db: &'a DB,
    read_options: Option<ReadOptions>,
    prefix: &[u8],
    read_ahead_config: &ReadAheadConfig,
) -> DBIteratorWithThreadMode<'a, DB> {
    reverse_prefix_iterator_from(db, read_options, prefix, None, read_ahead_config)
}

/// Reverse iterator over `prefix` that starts at `from` instead of the range end.
///
/// `from` is an *inclusive* seek target: the first row may be `from` itself, so
/// callers that need an exclusive cursor skip it. Because the iterator is still
/// bounded by the prefix range, starting it early can only ever expose rows of
/// the same namespace — it never walks into the neighbouring range.
pub(crate) fn reverse_prefix_iterator_from<'a>(
    db: &'a DB,
    read_options: Option<ReadOptions>,
    prefix: &[u8],
    from: Option<&[u8]>,
    read_ahead_config: &ReadAheadConfig,
) -> DBIteratorWithThreadMode<'a, DB> {
    let mut opts = read_options.unwrap_or_else(|| build_read_options(None, read_ahead_config));
    // PrefixRange is silently ignored without a configured prefix extractor.
    // Use explicit byte-range bounds instead: lower bound is the prefix itself;
    // upper bound is prefix_successor (the first key that sorts past the prefix).
    opts.set_iterate_lower_bound(prefix.to_vec());
    if let Some(upper) = prefix_successor(prefix) {
        opts.set_iterate_upper_bound(upper);
    }
    let mode = match from {
        Some(from) => IteratorMode::From(from, Direction::Reverse),
        None => IteratorMode::End,
    };
    db.iterator_opt(mode, opts)
}

/// Builds the database options together with the cache handles the database
/// will run with.
///
/// Callers must keep the returned [`CacheHandles`] alive for the lifetime of the
/// database to report cache statistics; RocksDB owns its own references, so a
/// dropped handle only loses observability, never correctness.
pub(crate) fn build_db_options_with_caches(
    config: &StorageConfig,
    enable_bloom_filters: bool,
) -> (Options, CacheHandles) {
    let mut options = Options::default();
    options.create_if_missing(true);
    options.set_error_if_exists(false);
    if let Ok(parallelism) = std::thread::available_parallelism() {
        options.increase_parallelism(parallelism.get() as i32);
    }
    options.set_compression_type(match config.compression_algorithm {
        CompressionAlgorithm::None => rocksdb::DBCompressionType::None,
        CompressionAlgorithm::Lz4 => rocksdb::DBCompressionType::Lz4,
        CompressionAlgorithm::Zstd => rocksdb::DBCompressionType::Zstd,
    });

    match config.compaction_strategy {
        CompactionStrategy::Level => {
            options.set_compaction_style(rocksdb::DBCompactionStyle::Level)
        }
        CompactionStrategy::Universal => {
            options.set_compaction_style(rocksdb::DBCompactionStyle::Universal)
        }
        CompactionStrategy::Fifo => options.set_compaction_style(rocksdb::DBCompactionStyle::Fifo),
    }

    if let Some(max_open) = config.max_open_files {
        options.set_max_open_files(max_open as i32);
    } else {
        options.set_max_open_files(4000);
    }

    if let Ok(parallelism) = std::thread::available_parallelism() {
        options.set_max_background_jobs(parallelism.get() as i32);
    } else {
        options.set_max_background_jobs(16);
    }
    options.set_bytes_per_sync(1048576); // 1MB — smooth I/O instead of bursty
    // Enable optimize_filters_for_hits when bloom filters are enabled
    // This reduces filter block size at the cost of slightly more disk seeks on negative lookups
    options.set_optimize_filters_for_hits(enable_bloom_filters);

    if let Some(write_buffer) = config.write_buffer_size {
        options.set_write_buffer_size(write_buffer);
    } else {
        options.set_write_buffer_size(256 * 1024 * 1024); // 256MB for fewer flushes during sync
    }
    options.set_max_write_buffer_number(6);
    options.set_min_write_buffer_number_to_merge(2);

    // Advanced Performance Tuning
    // CRITICAL: On Windows, mmap reads cause massive soft page faults (~750K per block)
    // when reading from cold RocksDB SST files. Disable mmap entirely on Windows.
    if !cfg!(windows) {
        options.set_allow_mmap_reads(true);
        tracing::debug!("Enabled mmap reads for RocksDB (non-Windows)");
    } else {
        options.set_allow_mmap_reads(false);
        tracing::warn!("Disabled mmap reads for RocksDB (Windows - prevents soft page faults)");
    }
    options.set_allow_mmap_writes(false);
    options.set_enable_pipelined_write(true);
    // Memtable bloom filters only help whole-key point lookups: no prefix
    // extractor is configured, so a prefix-only filter would never match.
    // `optimize_for_point_lookup` used to set both implicitly (at ratio 0.02);
    // they are pinned here so dropping that call cannot weaken point lookups.
    options.set_memtable_prefix_bloom_ratio(0.1); // better hit rate on memtable lookups
    options.set_memtable_whole_key_filtering(true);
    // Delay write stalls during heavy initial sync
    options.set_level_zero_slowdown_writes_trigger(30);
    options.set_level_zero_stop_writes_trigger(48);
    options.set_max_total_wal_size(512 * 1024 * 1024); // 512MB WAL cap

    // Configure block cache and bloom filters.
    //
    // `Options::optimize_for_point_lookup` is deliberately not called: its
    // argument is a cache size in MiB (RocksDB multiplies it by 1024 * 1024),
    // while `cache_size` is expressed in bytes, so the old call requested a
    // ~137 TiB cache. It also installs its own table factory that the explicit
    // `BlockBasedOptions` below replaces anyway. Its point-lookup intent is kept
    // explicitly here instead: byte-sized LRU block cache, bloom filter,
    // index/filter pinning, and the binary-and-hash data-block index.
    let cache_size = config.cache_size.unwrap_or(256 * 1024 * 1024);
    let cache = Cache::new_lru_cache(cache_size);
    let row_cache_size = resolve_row_cache_capacity(cache_size, config.row_cache_size_bytes);
    // A zero row cache means the operator disabled it: no cache is created and
    // RocksDB keeps its "no row cache" default instead of a 0-byte cache.
    let row_cache = (row_cache_size > 0).then(|| Cache::new_lru_cache(row_cache_size));
    if let Some(row_cache) = &row_cache {
        options.set_row_cache(row_cache);
    }
    let mut table_options = BlockBasedOptions::default();
    table_options.set_block_cache(&cache);
    // Hash-probe the data block after the binary search narrows its key range;
    // the default data-block index is binary search only. A 0.75 hash
    // utilisation ratio matches the RocksDB default and the ratio the removed
    // point-lookup preset requested.
    table_options.set_data_block_index_type(DataBlockIndexType::BinaryAndHash);
    table_options.set_data_block_hash_ratio(0.75);

    if enable_bloom_filters {
        // Enable bloom filters with 10 bits per key for ~1% false positive rate
        table_options.set_bloom_filter(10.0, false);
        // Cache index and filter blocks in block cache
        table_options.set_cache_index_and_filter_blocks(true);
        table_options.set_pin_l0_filter_and_index_blocks_in_cache(true);
        // `rocksdb` 0.21 has no binding for
        // `cache_index_and_filter_blocks_with_high_priority`, so index and
        // filter blocks cannot be marked high priority in the cache. Pinning
        // the top-level index and filter instead keeps the per-table metadata
        // every lookup needs resident, so data-block pressure cannot evict it.
        table_options.set_pin_top_level_index_and_filter(true);
    }

    options.set_block_based_table_factory(&table_options);

    if config.enable_statistics {
        options.enable_statistics();
    }

    (
        options,
        CacheHandles {
            block_cache: cache,
            block_cache_capacity: cache_size,
            row_cache,
            row_cache_capacity: row_cache_size,
        },
    )
}

#[cfg(test)]
mod tests {
    use super::resolve_row_cache_capacity;

    const MIB: usize = 1024 * 1024;

    #[test]
    fn unset_row_cache_derives_a_quarter_of_the_block_cache() {
        assert_eq!(resolve_row_cache_capacity(1024 * MIB, None), 256 * MIB);
    }

    #[test]
    fn derived_row_cache_stays_inside_the_historical_clamp() {
        // Below the floor: a small block cache still yields 64 MiB of row cache.
        assert_eq!(resolve_row_cache_capacity(32 * MIB, None), 64 * MIB);
        assert_eq!(resolve_row_cache_capacity(0, None), 64 * MIB);
        // Above the ceiling: the derivation caps at 512 MiB.
        assert_eq!(resolve_row_cache_capacity(4 * 1024 * MIB, None), 512 * MIB);
    }

    #[test]
    fn zero_disables_the_row_cache() {
        assert_eq!(resolve_row_cache_capacity(1024 * MIB, Some(0)), 0);
    }

    #[test]
    fn explicit_row_cache_size_is_never_clamped() {
        assert_eq!(
            resolve_row_cache_capacity(1024 * MIB, Some(256 * MIB)),
            256 * MIB
        );
        // Below the derived floor and above its ceiling are both honoured.
        assert_eq!(
            resolve_row_cache_capacity(1024 * MIB, Some(8 * MIB)),
            8 * MIB
        );
        assert_eq!(
            resolve_row_cache_capacity(64 * MIB, Some(2 * 1024 * MIB)),
            2 * 1024 * MIB
        );
    }
}
