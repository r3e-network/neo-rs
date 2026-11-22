use super::*;
// These methods form the operational API for RocksDbStore (fast-sync, cache
// management, diagnostics).  The struct is crate-private so the compiler flags
// them as dead code, but they are intentionally kept for use by higher-level
// subsystems that will be wired up during node integration.
#[allow(dead_code)]
impl RocksDbStore {
    /// Enables fast sync mode optimizations (disable WAL, reduce fsync).
    ///
    /// Auto compaction is deliberately **not** disabled here. A long import
    /// runs for hours, and with compaction off RocksDB accumulated hundreds of
    /// level-0 files that never merged downwards; every point read then had to
    /// probe the whole L0 backlog, so read throughput decayed monotonically as
    /// the import progressed (and writes eventually stalled at the L0 stop
    /// trigger). The speed win of skipping compaction is paid back many times
    /// over by that read amplification, so the WAL is the only fast-sync knob
    /// that gets switched off.
    pub fn enable_fast_sync_mode(&self) {
        // Switch to high-throughput batch config (disable WAL, no fsync)
        *self.batch_config.write() = WriteBatchConfig::high_throughput();

        debug!(target: "neo", "enabled fast sync mode optimizations (WAL disabled, auto compaction kept enabled)");
    }

    /// Disables fast sync mode optimizations.
    pub fn disable_fast_sync_mode(&self) {
        // Restore balanced batch config (WAL enabled)
        *self.batch_config.write() = WriteBatchConfig::durable();
        debug!(target: "neo", "disabled fast sync mode optimizations (WAL restored)");
    }

    /// Runs a blocking, full-range manual compaction over the whole keyspace.
    ///
    /// The store opens a single default column family, so one `compact_range`
    /// covers every key. Pending buffered writes and memtables are flushed
    /// first: values still in memory are invisible to compaction and would
    /// otherwise be written back into level 0 right after it finishes.
    ///
    /// Returns once RocksDB reports the compaction complete, so callers can log
    /// the elapsed time as a one-time maintenance step.
    pub fn compact_all(&self) -> StorageResult<()> {
        self.flush_batch_writes()?;

        let mut flush_options = rocksdb::FlushOptions::default();
        flush_options.set_wait(true);
        self.db.flush_opt(&flush_options).map_err(|err| {
            StorageError::CommitFailed(format!("RocksDB pre-compaction flush failed: {err}"))
        })?;

        self.db.compact_range::<&[u8], &[u8]>(None, None);
        Ok(())
    }

    /// Returns the storage-engine cache counters for this database.
    ///
    /// Block and row cache numbers come from the live `Cache` handles the
    /// database was opened with, which is the only source for the row cache:
    /// RocksDB publishes no row-cache property. Index, filter and data-block
    /// sizes come from the aggregated table properties, so they describe
    /// flushed SST files only; values still in a memtable are not counted.
    pub fn cache_stats(&self) -> StoreCacheStats {
        let table_block_sizes = self
            .db
            .property_value("rocksdb.aggregated-table-properties")
            .ok()
            .flatten()
            .map(|properties| {
                (
                    property_bytes(&properties, "index block size"),
                    property_bytes(&properties, "filter block size"),
                    property_bytes(&properties, "data block size"),
                )
            })
            .unwrap_or((None, None, None));
        StoreCacheStats {
            block_cache: self.caches.block_cache_usage(),
            row_cache: self.caches.row_cache_usage(),
            index_size: table_block_sizes.0,
            filter_size: table_block_sizes.1,
            data_block_size: table_block_sizes.2,
            estimate_table_readers_mem: self
                .db
                .property_int_value("rocksdb.estimate-table-readers-mem")
                .ok()
                .flatten()
                .and_then(|bytes| usize::try_from(bytes).ok()),
        }
    }

    /// Returns the per-level SST file counts, lowest level first.
    ///
    /// Level 0 accumulates one file per memtable flush, so its count is the
    /// cheapest signal that background compaction is (not) keeping up: a
    /// monotonically growing L0 count means reads are paying an ever larger
    /// file fan-out per lookup.
    pub fn level_file_counts(&self) -> Vec<(u32, u64)> {
        (0..=6u32)
            .filter_map(|level| {
                self.db
                    .property_int_value(&format!("rocksdb.num-files-at-level{level}"))
                    .ok()
                    .flatten()
                    .map(|count| (level, count))
            })
            .collect()
    }

    /// Flushes buffered writes and makes them durable.
    ///
    /// WAL-backed writes are already crash-durable once the WAL is synced, so a
    /// WAL sync suffices. Fast-sync batches disable the WAL, which means the only
    /// durable copy lives in the memtable; those require a checked memtable flush
    /// instead, otherwise a crash silently loses the writes that were reported as
    /// durable.
    pub fn try_flush_durable(&self) -> StorageResult<()> {
        self.flush_batch_writes()?;

        self.db.flush_wal(true).map_err(|err| {
            StorageError::CommitFailed(format!("RocksDB durable WAL flush failed: {err}"))
        })?;

        // WAL-disabled writes have no durable copy until their memtable reaches
        // disk. The flag also covers writes made before the batch configuration
        // was restored to WAL-backed mode, which a plain WAL sync would miss.
        let memtable_only = self.batch_config.read().disable_wal
            || self.memtable_only_writes.load(Ordering::Acquire);
        if !memtable_only {
            return Ok(());
        }

        let mut flush_options = rocksdb::FlushOptions::default();
        flush_options.set_wait(true);
        self.db.flush_opt(&flush_options).map_err(|err| {
            StorageError::CommitFailed(format!("RocksDB durable memtable flush failed: {err}"))
        })?;
        self.memtable_only_writes.store(false, Ordering::Release);
        Ok(())
    }

    /// Force flush all memtables to disk.
    pub fn flush_memtables(&self) {
        if let Err(err) = self.db.flush_wal(true) {
            warn!(target: "neo", error = %err, "failed to flush WAL");
        }
        if let Err(err) = self.db.flush_opt(&rocksdb::FlushOptions::default()) {
            warn!(target: "neo", error = %err, "failed to flush memtables");
        }
    }

    /// Returns memory usage statistics.
    pub fn memory_usage(&self) -> Option<(u64, u64)> {
        self.db
            .property_int_value("rocksdb.cur-size-active-mem-table")
            .ok()
            .flatten()
            .map(|active| {
                let total = self
                    .db
                    .property_int_value("rocksdb.cur-size-all-mem-tables")
                    .ok()
                    .flatten()
                    .unwrap_or(0);
                (active, total)
            })
    }

    /// Enables or disables block checksum verification for reads and scans.
    /// Verification is on by default; disabling it weakens corruption detection.
    pub fn set_verify_checksums(&mut self, enable: bool) {
        self.read_ahead_config.verify_checksums = enable;
    }

    /// Enables read caching with the specified configuration.
    pub fn enable_read_cache(&mut self, config: ReadCacheConfig) {
        self.read_cache = Some(Arc::new(StorageReadCache::new(config)));
        debug!(target: "neo", "enabled read cache");
    }

    /// Disables read caching.
    pub fn disable_read_cache(&mut self) {
        self.read_cache = None;
        debug!(target: "neo", "disabled read cache");
    }

    /// Gets read cache statistics if caching is enabled.
    pub fn read_cache_stats(
        &self,
    ) -> Option<crate::persistence::read_cache::ReadCacheStatsSnapshot> {
        self.read_cache.as_ref().map(|c| c.stats())
    }

    /// Clears the read cache.
    pub fn clear_read_cache(&self) {
        if let Some(ref cache) = self.read_cache {
            cache.clear();
            debug!(target: "neo", "read cache cleared");
        }
    }

    /// Returns batch commit statistics.
    pub fn batch_commit_stats(&self) -> WriteBatchStatsSnapshot {
        self.batch_committer.buffer.stats_snapshot()
    }

    /// Forces a flush of pending batch writes.
    ///
    /// A batch that carried writes supersedes cached reads, and the buffer does
    /// not report which keys it wrote, so the cache is dropped wholesale once the
    /// write succeeded. A failed flush leaves both the buffer and the cache
    /// untouched so the caller can retry.
    pub fn flush_batch_writes(&self) -> StorageResult<()> {
        let pending = self.batch_committer.buffer.pending_count();
        if pending == 0 {
            return Ok(());
        }
        if self.batch_committer.buffer.config().disable_wal {
            self.memtable_only_writes.store(true, Ordering::Release);
        }
        self.batch_committer.buffer.force_flush()?;
        self.invalidate_all_reads();
        Ok(())
    }
}
