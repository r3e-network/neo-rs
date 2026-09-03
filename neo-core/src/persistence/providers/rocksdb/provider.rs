use crate::{
    error::{CoreError, CoreResult},
    persistence::{
        read_cache::ReadCacheConfig,
        storage::{CompactionStrategy, CompressionAlgorithm, StorageConfig},
        store::Store,
        store_provider::StoreProvider,
        write_batch_buffer::{WriteBatchBuffer, WriteBatchConfig},
    },
};
use rocksdb::{
    BlockBasedOptions, Cache, DB, DBIteratorWithThreadMode, Direction, IteratorMode, Options,
    PrefixRange, ReadOptions, Snapshot as DbSnapshot,
};
use std::{path::PathBuf, sync::Arc};

use super::store::RocksDbStore;

pub use crate::persistence::write_batch_buffer::{
    WriteBatchConfig as BatchCommitConfig, WriteBatchStats as BatchCommitStats,
    WriteBatchStatsSnapshot as BatchCommitStatsSnapshot,
};

pub struct BatchCommitter {
    pub(crate) buffer: WriteBatchBuffer,
}

impl BatchCommitter {
    pub(crate) fn new(db: Arc<DB>, config: WriteBatchConfig) -> Self {
        let buffer = WriteBatchBuffer::new(db, config);

        Self { buffer }
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
}

impl RocksDBStoreProvider {
    pub fn new(base_config: StorageConfig) -> Self {
        Self {
            base_config,
            batch_config: BatchCommitConfig::balanced(),
            batch_stats: Arc::new(BatchCommitStats::new()),
            read_cache_config: Some(ReadCacheConfig::default()),
            enable_bloom_filters: true,
            enable_read_ahead: true,
        }
    }

    pub fn with_batch_config(mut self, config: BatchCommitConfig) -> Self {
        self.batch_config = config;
        self
    }

    pub fn with_read_cache(mut self, config: ReadCacheConfig) -> Self {
        self.read_cache_config = Some(config);
        self
    }

    pub fn without_read_cache(mut self) -> Self {
        self.read_cache_config = None;
        self
    }

    pub fn with_bloom_filters(mut self, enable: bool) -> Self {
        self.enable_bloom_filters = enable;
        self
    }

    pub fn with_read_ahead(mut self, enable: bool) -> Self {
        self.enable_read_ahead = enable;
        self
    }

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
}

impl StoreProvider for RocksDBStoreProvider {
    fn name(&self) -> &str {
        "RocksDBStore"
    }

    fn get_store(&self, path: &str) -> CoreResult<Arc<dyn Store>> {
        let resolved = self.resolved_path(path);
        let config = StorageConfig {
            path: resolved,
            ..self.base_config.clone()
        };
        let store = RocksDbStore::open(
            &config,
            self.batch_config,
            &self.read_cache_config,
            self.enable_bloom_filters,
            self.enable_read_ahead,
        )
        .map_err(|err| CoreError::Io {
            message: format!(
                "failed to open RocksDB store at {}: {err}",
                config.path.display()
            ),
        })?;
        Ok(Arc::new(store))
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
            verify_checksums: false,
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

pub(crate) fn reverse_prefix_iterator<'a>(
    db: &'a DB,
    read_options: Option<ReadOptions>,
    prefix: &[u8],
    read_ahead_config: &ReadAheadConfig,
) -> DBIteratorWithThreadMode<'a, DB> {
    let mut opts = read_options.unwrap_or_else(|| build_read_options(None, read_ahead_config));
    opts.set_iterate_range(PrefixRange(prefix));
    db.iterator_opt(IteratorMode::End, opts)
}

pub(crate) fn build_db_options(config: &StorageConfig, enable_bloom_filters: bool) -> Options {
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
    options.set_allow_mmap_reads(true);
    options.set_allow_mmap_writes(false);
    options.set_enable_pipelined_write(true);
    options.set_memtable_prefix_bloom_ratio(0.1); // better hit rate on memtable lookups
    // Delay write stalls during heavy initial sync
    options.set_level_zero_slowdown_writes_trigger(30);
    options.set_level_zero_stop_writes_trigger(48);
    options.set_max_total_wal_size(512 * 1024 * 1024); // 512MB WAL cap

    // Configure block cache and bloom filters
    let cache_size = config.cache_size.unwrap_or(256 * 1024 * 1024);
    options.optimize_for_point_lookup((cache_size / 2) as u64);
    let cache = Cache::new_lru_cache(cache_size);
    let row_cache_size = (cache_size / 4).clamp(64 * 1024 * 1024, 512 * 1024 * 1024);
    let row_cache = Cache::new_lru_cache(row_cache_size);
    options.set_row_cache(&row_cache);
    let mut table_options = BlockBasedOptions::default();
    table_options.set_block_cache(&cache);

    if enable_bloom_filters {
        // Enable bloom filters with 10 bits per key for ~1% false positive rate
        table_options.set_bloom_filter(10.0, false);
        // Cache index and filter blocks in block cache
        table_options.set_cache_index_and_filter_blocks(true);
        table_options.set_pin_l0_filter_and_index_blocks_in_cache(true);
    }

    options.set_block_based_table_factory(&table_options);

    if config.enable_statistics {
        options.enable_statistics();
    }

    options
}
