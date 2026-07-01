//! Write batch buffer for optimized batch operations.
//!
//! This module provides a write batch buffer that accumulates write operations
//! and flushes them periodically based on size or time thresholds.

use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::{Mutex, RwLock};
use tracing::{debug, error, trace};

use crate::{StorageError, StorageResult};

use rocksdb::{DB, WriteBatch, WriteOptions};

/// Statistics for write batch operations.
#[derive(Debug, Default)]
pub struct WriteBatchStats {
    /// Total number of batches flushed
    pub batches_flushed: AtomicU64,
    /// Total number of operations written
    pub operations_written: AtomicU64,
    /// Total bytes written
    pub bytes_written: AtomicU64,
    /// Total flush duration in milliseconds
    pub total_flush_duration_ms: AtomicU64,
    /// Number of flush timeouts
    pub flush_timeouts: AtomicU64,
    /// Current pending operations
    pub pending_operations: AtomicUsize,
}

impl WriteBatchStats {
    /// Creates new statistics.
    pub fn new() -> Self {
        Self::default()
    }

    /// Records a flush operation.
    pub fn record_flush(&self, operations: usize, bytes: usize, duration_ms: u64) {
        self.batches_flushed.fetch_add(1, Ordering::Relaxed);
        self.operations_written
            .fetch_add(operations as u64, Ordering::Relaxed);
        self.bytes_written
            .fetch_add(bytes as u64, Ordering::Relaxed);
        self.total_flush_duration_ms
            .fetch_add(duration_ms, Ordering::Relaxed);
    }

    /// Records a flush timeout.
    pub fn record_timeout(&self) {
        self.flush_timeouts.fetch_add(1, Ordering::Relaxed);
    }

    /// Updates pending operations count.
    pub fn set_pending(&self, count: usize) {
        self.pending_operations.store(count, Ordering::Relaxed);
    }

    /// Gets a snapshot of statistics.
    pub fn snapshot(&self) -> WriteBatchStatsSnapshot {
        WriteBatchStatsSnapshot {
            batches_flushed: self.batches_flushed.load(Ordering::Relaxed),
            operations_written: self.operations_written.load(Ordering::Relaxed),
            bytes_written: self.bytes_written.load(Ordering::Relaxed),
            total_flush_duration_ms: self.total_flush_duration_ms.load(Ordering::Relaxed),
            flush_timeouts: self.flush_timeouts.load(Ordering::Relaxed),
            pending_operations: self.pending_operations.load(Ordering::Relaxed),
        }
    }
}

/// Snapshot of write batch statistics.
#[derive(Debug, Clone, Copy)]
pub struct WriteBatchStatsSnapshot {
    /// Number of write batches flushed to RocksDB.
    pub batches_flushed: u64,
    /// Number of individual put/delete operations written.
    pub operations_written: u64,
    /// Approximate payload bytes written through the batch buffer.
    pub bytes_written: u64,
    /// Cumulative time spent flushing batches.
    pub total_flush_duration_ms: u64,
    /// Number of flush attempts that timed out.
    pub flush_timeouts: u64,
    /// Number of operations currently buffered but not yet flushed.
    pub pending_operations: usize,
}

impl WriteBatchStatsSnapshot {
    /// Calculates average flush duration in milliseconds.
    pub fn avg_flush_duration_ms(&self) -> f64 {
        if self.batches_flushed == 0 {
            0.0
        } else {
            self.total_flush_duration_ms as f64 / self.batches_flushed as f64
        }
    }

    /// Calculates average bytes per flush.
    pub fn avg_bytes_per_flush(&self) -> f64 {
        if self.batches_flushed == 0 {
            0.0
        } else {
            self.bytes_written as f64 / self.batches_flushed as f64
        }
    }

    /// Calculates average operations per flush.
    pub fn avg_ops_per_flush(&self) -> f64 {
        if self.batches_flushed == 0 {
            0.0
        } else {
            self.operations_written as f64 / self.batches_flushed as f64
        }
    }
}

/// Configuration for write batch buffering.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WriteBatchConfig {
    /// Maximum number of operations before flush
    pub max_batch_size: usize,
    /// Maximum time before flush (milliseconds)
    pub max_delay_ms: u64,
    /// Minimum operations to trigger time-based flush
    pub min_operations: usize,
    /// Maximum bytes in batch before flush
    pub max_batch_bytes: usize,
    /// Whether to sync on flush
    pub sync_on_flush: bool,
    /// Whether to disable WAL for better performance
    pub disable_wal: bool,
}

impl Default for WriteBatchConfig {
    fn default() -> Self {
        Self {
            max_batch_size: 1000,
            max_delay_ms: 100,
            min_operations: 10,
            max_batch_bytes: 4 * 1024 * 1024, // 4MB
            sync_on_flush: false,
            disable_wal: false,
        }
    }
}

impl WriteBatchConfig {
    /// Creates configuration for high-throughput scenarios.
    pub fn high_throughput() -> Self {
        Self {
            max_batch_size: 50_000,
            max_delay_ms: 250,
            min_operations: 5_000,
            max_batch_bytes: 64 * 1024 * 1024, // 64MB
            sync_on_flush: false,
            disable_wal: true,
        }
    }

    /// Creates configuration for durability-focused scenarios.
    pub fn durable() -> Self {
        Self {
            max_batch_size: 100,
            max_delay_ms: 10,
            min_operations: 1,
            max_batch_bytes: 1024 * 1024, // 1MB
            sync_on_flush: true,
            disable_wal: false,
        }
    }

    /// Creates configuration for balanced scenarios.
    pub fn balanced() -> Self {
        Self {
            max_batch_size: 500,
            max_delay_ms: 50,
            min_operations: 20,
            max_batch_bytes: 4 * 1024 * 1024, // 4MB
            sync_on_flush: false,
            disable_wal: false,
        }
    }
}

/// Buffered write batch for RocksDB.
pub struct WriteBatchBuffer {
    config: RwLock<WriteBatchConfig>,
    stats: Arc<WriteBatchStats>,
    db: Arc<DB>,
    pending: Mutex<PendingWriteBatch>,
    flush_gate: Mutex<()>,
    last_flush: Mutex<Instant>,
    last_flush_time_ms: AtomicU64,
}

#[derive(Default)]
struct PendingWriteBatch {
    batch: WriteBatch,
    config: Option<WriteBatchConfig>,
    count: usize,
    bytes: usize,
}

impl PendingWriteBatch {
    fn capture_config(&mut self, config: WriteBatchConfig) {
        if self.config.is_none() {
            self.config = Some(config);
        }
    }
}

impl WriteBatchBuffer {
    /// Creates a new write batch buffer.
    pub fn new(db: Arc<DB>, config: WriteBatchConfig) -> Self {
        let stats = Arc::new(WriteBatchStats::new());
        Self {
            config: RwLock::new(config),
            stats,
            db,
            pending: Mutex::new(PendingWriteBatch::default()),
            flush_gate: Mutex::new(()),
            last_flush: Mutex::new(Instant::now()),
            last_flush_time_ms: AtomicU64::new(0),
        }
    }

    /// Creates a new write batch buffer with default configuration.
    pub fn with_defaults(db: Arc<DB>) -> Self {
        Self::new(db, WriteBatchConfig::default())
    }

    /// Gets the statistics.
    pub fn stats(&self) -> Arc<WriteBatchStats> {
        Arc::clone(&self.stats)
    }

    /// Gets a snapshot of statistics.
    pub fn stats_snapshot(&self) -> WriteBatchStatsSnapshot {
        self.stats.snapshot()
    }

    /// Gets the configuration.
    pub fn config(&self) -> WriteBatchConfig {
        *self.config.read()
    }

    /// Updates the configuration used by future buffered writes.
    pub fn set_config(&self, config: WriteBatchConfig) -> StorageResult<()> {
        let _flush_guard = self.flush_gate.lock();
        let mut current_config = self.config.write();
        self.flush_locked(*current_config)?;
        *current_config = config;
        Ok(())
    }

    /// Adds a put operation to the batch.
    pub fn put(&self, key: &[u8], value: &[u8]) {
        let config = self.config.read();
        let key_len = key.len();
        let value_len = value.len();

        let (new_count, new_bytes) = {
            let mut pending = self.pending.lock();
            pending.capture_config(*config);
            pending.batch.put(key, value);
            pending.count += 1;
            pending.bytes += key_len + value_len;
            (pending.count, pending.bytes)
        };

        self.stats.set_pending(new_count);

        trace!(
            target: "neo",
            key_len,
            value_len,
            pending_ops = new_count,
            pending_bytes = new_bytes,
            "write batch put"
        );

        // Check if we should flush
        if self.should_flush(new_count, new_bytes) {
            let _ = self.flush();
        }
    }

    /// Adds a delete operation to the batch.
    pub fn delete(&self, key: &[u8]) {
        let config = self.config.read();
        let key_len = key.len();

        let (new_count, new_bytes) = {
            let mut pending = self.pending.lock();
            pending.capture_config(*config);
            pending.batch.delete(key);
            pending.count += 1;
            pending.bytes += key_len;
            (pending.count, pending.bytes)
        };

        self.stats.set_pending(new_count);

        trace!(
            target: "neo",
            key_len,
            pending_ops = new_count,
            pending_bytes = new_bytes,
            "write batch delete"
        );

        // Check if we should flush
        if self.should_flush(new_count, new_bytes) {
            let _ = self.flush();
        }
    }

    /// Adds multiple put/delete operations to the batch with one lock and one
    /// flush-threshold check.
    pub fn extend<'a, I>(&self, operations: I)
    where
        I: IntoIterator<Item = (&'a [u8], Option<&'a [u8]>)>,
    {
        let mut iter = operations.into_iter();
        self.extend_from(|sink| {
            for (key, value) in iter.by_ref() {
                sink(key, value);
            }
        });
    }

    /// Adds multiple put/delete operations produced by a visitor with one lock
    /// and one flush-threshold check.
    pub fn extend_from<F>(&self, mut visit: F)
    where
        F: FnMut(&mut dyn FnMut(&[u8], Option<&[u8]>)),
    {
        let config = self.config.read();
        let (added_count, added_bytes, new_count, new_bytes) = {
            let mut pending = self.pending.lock();
            let mut added_count = 0usize;
            let mut added_bytes = 0usize;
            let mut sink = |key: &[u8], value: Option<&[u8]>| {
                pending.capture_config(*config);
                added_count += 1;
                added_bytes += key.len();
                match value {
                    Some(value) => {
                        added_bytes += value.len();
                        pending.batch.put(key, value);
                    }
                    None => pending.batch.delete(key),
                }
            };
            visit(&mut sink);
            if added_count == 0 {
                return;
            }
            pending.count += added_count;
            pending.bytes += added_bytes;
            (added_count, added_bytes, pending.count, pending.bytes)
        };

        self.stats.set_pending(new_count);

        trace!(
            target: "neo",
            operations = added_count,
            bytes = added_bytes,
            pending_ops = new_count,
            pending_bytes = new_bytes,
            "write batch extend"
        );

        if self.should_flush(new_count, new_bytes) {
            let _ = self.flush();
        }
    }

    /// Checks if the batch should be flushed.
    fn should_flush(&self, pending_count: usize, pending_bytes: usize) -> bool {
        let config = self.config();
        // Check size-based flush
        if pending_count >= config.max_batch_size {
            return true;
        }

        // Check bytes-based flush
        if pending_bytes >= config.max_batch_bytes {
            return true;
        }

        // Check time-based flush (only if we have minimum operations)
        if pending_count >= config.min_operations {
            let last_flush = self.last_flush.lock();
            let elapsed = last_flush.elapsed();
            drop(last_flush);

            if elapsed.as_millis() as u64 >= config.max_delay_ms {
                return true;
            }
        }

        false
    }

    /// Flushes the batch to the database.
    pub fn flush(&self) -> StorageResult<()> {
        let _flush_guard = self.flush_gate.lock();
        let default_config = self.config();
        self.flush_locked(default_config)
    }

    fn flush_locked(&self, default_config: WriteBatchConfig) -> StorageResult<()> {
        let (batch_to_write, config, count, bytes, batch_data) = {
            let mut pending = self.pending.lock();
            if pending.batch.is_empty() {
                return Ok(());
            }

            let config = pending.config.unwrap_or(default_config);
            let count = pending.count;
            let bytes = pending.bytes;
            let batch_data = pending.batch.data().to_vec();
            let batch_to_write = std::mem::take(&mut pending.batch);
            pending.config = None;
            pending.count = 0;
            pending.bytes = 0;
            self.stats.set_pending(0);
            (batch_to_write, config, count, bytes, batch_data)
        };

        let start = Instant::now();

        // Build write options
        let mut write_opts = WriteOptions::default();
        write_opts.set_sync(config.sync_on_flush);
        if config.disable_wal {
            write_opts.disable_wal(true);
        }

        // Write the batch
        match self.db.write_opt(batch_to_write, &write_opts) {
            Ok(()) => {
                let duration = start.elapsed();
                let duration_ms = duration.as_millis() as u64;

                // Update last flush time
                *self.last_flush.lock() = Instant::now();
                self.last_flush_time_ms
                    .store(duration_ms, Ordering::Relaxed);

                // Update statistics
                self.stats.record_flush(count, bytes, duration_ms);
                debug!(
                    target: "neo",
                    operations = count,
                    bytes,
                    duration_ms,
                    "write batch flushed"
                );

                Ok(())
            }
            Err(e) => {
                self.restore_failed_flush_batch(&batch_data, config, count, bytes);
                error!(target: "neo", error = %e, "write batch flush failed");
                Err(StorageError::Io {
                    message: format!("RocksDB write batch flush failed: {}", e),
                })
            }
        }
    }

    fn restore_failed_flush_batch(
        &self,
        batch_data: &[u8],
        config: WriteBatchConfig,
        count: usize,
        bytes: usize,
    ) {
        let mut pending = self.pending.lock();
        if pending.batch.is_empty() {
            pending.batch = WriteBatch::from_data(batch_data);
            pending.config = Some(config);
            pending.count = count;
            pending.bytes = bytes;
        } else {
            let current_data = pending.batch.data().to_vec();
            let current_count = pending.count;
            let current_bytes = pending.bytes;
            // rocksdb 0.21 does not expose WriteBatch append. Preserve the failed
            // batch and then replay newer operations after it by rebuilding from
            // serialized batches.
            let mut combined = WriteBatch::from_data(batch_data);
            let newer = WriteBatch::from_data(&current_data);
            newer.iterate(&mut ReplayIntoBatch {
                target: &mut combined,
            });
            pending.batch = combined;
            pending.config = Some(config);
            pending.count = count + current_count;
            pending.bytes = bytes + current_bytes;
        }
        self.stats.set_pending(pending.count);
    }

    /// Forces a flush regardless of batch size or time.
    pub fn force_flush(&self) -> StorageResult<()> {
        debug!(target: "neo", "force flushing write batch");
        self.flush()
    }

    /// Returns the number of pending operations.
    pub fn pending_count(&self) -> usize {
        self.pending.lock().count
    }

    /// Returns the number of pending bytes.
    pub fn pending_bytes(&self) -> usize {
        self.pending.lock().bytes
    }

    /// Returns true if there are pending operations.
    pub fn has_pending(&self) -> bool {
        self.pending.lock().count > 0
    }

    /// Returns the time since last flush.
    pub fn time_since_flush(&self) -> Duration {
        self.last_flush.lock().elapsed()
    }

    /// Returns the duration of the last flush operation.
    pub fn last_flush_duration(&self) -> Duration {
        Duration::from_millis(self.last_flush_time_ms.load(Ordering::Relaxed))
    }

    /// Clears all pending operations without flushing.
    pub fn clear(&self) {
        let mut pending = self.pending.lock();
        *pending = PendingWriteBatch::default();
        self.stats.set_pending(0);
        debug!(target: "neo", "write batch cleared");
    }
}

struct ReplayIntoBatch<'a> {
    target: &'a mut WriteBatch,
}

impl rocksdb::WriteBatchIterator for ReplayIntoBatch<'_> {
    fn put(&mut self, key: Box<[u8]>, value: Box<[u8]>) {
        self.target.put(&*key, &*value);
    }

    fn delete(&mut self, key: Box<[u8]>) {
        self.target.delete(&*key);
    }
}

#[cfg(test)]
#[path = "../tests/rocksdb/write_batch_buffer.rs"]
mod tests;
