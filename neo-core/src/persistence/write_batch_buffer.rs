//! Write batch buffer for optimized batch operations.
//!
//! This module provides a write batch buffer that accumulates write operations
//! and flushes them periodically based on size or time thresholds.

use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

#[cfg(feature = "rocksdb")]
use std::sync::Arc;
#[cfg(feature = "rocksdb")]
use std::time::{Duration, Instant};

#[cfg(feature = "rocksdb")]
use parking_lot::Mutex;
#[cfg(feature = "rocksdb")]
use tracing::{debug, error, trace};

#[cfg(feature = "rocksdb")]
use crate::{CoreError, CoreResult};

#[cfg(feature = "rocksdb")]
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
    pub batches_flushed: u64,
    pub operations_written: u64,
    pub bytes_written: u64,
    pub total_flush_duration_ms: u64,
    pub flush_timeouts: u64,
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
#[derive(Debug, Clone, Copy)]
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
            disable_wal: true,
        }
    }
}

impl WriteBatchConfig {
    /// Creates configuration for high-throughput scenarios.
    pub fn high_throughput() -> Self {
        Self {
            max_batch_size: 5000,
            max_delay_ms: 50,
            min_operations: 50,
            max_batch_bytes: 16 * 1024 * 1024, // 16MB
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
#[cfg(feature = "rocksdb")]
pub struct WriteBatchBuffer {
    config: WriteBatchConfig,
    stats: Arc<WriteBatchStats>,
    db: Arc<DB>,
    batch: Mutex<WriteBatch>,
    pending_count: AtomicUsize,
    pending_bytes: AtomicUsize,
    last_flush: Mutex<Instant>,
    last_flush_time_ms: AtomicU64,
}

#[cfg(feature = "rocksdb")]
impl WriteBatchBuffer {
    /// Creates a new write batch buffer.
    pub fn new(db: Arc<DB>, config: WriteBatchConfig) -> Self {
        let stats = Arc::new(WriteBatchStats::new());
        Self {
            config,
            stats,
            db,
            batch: Mutex::new(WriteBatch::default()),
            pending_count: AtomicUsize::new(0),
            pending_bytes: AtomicUsize::new(0),
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
    pub fn config(&self) -> &WriteBatchConfig {
        &self.config
    }

    /// Adds a put operation to the batch.
    pub fn put(&self, key: &[u8], value: &[u8]) {
        let key_len = key.len();
        let value_len = value.len();

        {
            let mut batch = self.batch.lock();
            batch.put(key, value);
        }

        let new_count = self.pending_count.fetch_add(1, Ordering::Relaxed) + 1;
        let new_bytes = self
            .pending_bytes
            .fetch_add(key_len + value_len, Ordering::Relaxed)
            + key_len
            + value_len;

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
        let key_len = key.len();

        {
            let mut batch = self.batch.lock();
            batch.delete(key);
        }

        let new_count = self.pending_count.fetch_add(1, Ordering::Relaxed) + 1;
        let new_bytes = self.pending_bytes.fetch_add(key_len, Ordering::Relaxed) + key_len;

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

    /// Checks if the batch should be flushed.
    fn should_flush(&self, pending_count: usize, pending_bytes: usize) -> bool {
        // Check size-based flush
        if pending_count >= self.config.max_batch_size {
            return true;
        }

        // Check bytes-based flush
        if pending_bytes >= self.config.max_batch_bytes {
            return true;
        }

        // Check time-based flush (only if we have minimum operations)
        if pending_count >= self.config.min_operations {
            let last_flush = self.last_flush.lock();
            let elapsed = last_flush.elapsed();
            drop(last_flush);

            if elapsed.as_millis() as u64 >= self.config.max_delay_ms {
                return true;
            }
        }

        false
    }

    /// Flushes the batch to the database.
    pub fn flush(&self) -> CoreResult<()> {
        let mut batch = self.batch.lock();

        if batch.is_empty() {
            return Ok(());
        }

        let count = self.pending_count.load(Ordering::Relaxed);
        let bytes = self.pending_bytes.load(Ordering::Relaxed);

        let start = Instant::now();

        // Build write options
        let mut write_opts = WriteOptions::default();
        write_opts.set_sync(self.config.sync_on_flush);
        if self.config.disable_wal {
            write_opts.disable_wal(true);
        }

        // Take ownership of the batch and create a new one
        let batch_to_write = std::mem::take(&mut *batch);

        // Write the batch
        match self.db.write_opt(batch_to_write, &write_opts) {
            Ok(()) => {
                let duration = start.elapsed();
                let duration_ms = duration.as_millis() as u64;

                // Reset counters
                self.pending_count.store(0, Ordering::Relaxed);
                self.pending_bytes.store(0, Ordering::Relaxed);

                // Update last flush time
                *self.last_flush.lock() = Instant::now();
                self.last_flush_time_ms
                    .store(duration_ms, Ordering::Relaxed);

                // Update statistics
                self.stats.record_flush(count, bytes, duration_ms);

                self.stats.set_pending(0);

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
                error!(target: "neo", error = %e, "write batch flush failed");
                Err(CoreError::Io {
                    message: format!("RocksDB write batch flush failed: {}", e),
                })
            }
        }
    }

    /// Forces a flush regardless of batch size or time.
    pub fn force_flush(&self) -> CoreResult<()> {
        debug!(target: "neo", "force flushing write batch");
        self.flush()
    }

    /// Returns the number of pending operations.
    pub fn pending_count(&self) -> usize {
        self.pending_count.load(Ordering::Relaxed)
    }

    /// Returns the number of pending bytes.
    pub fn pending_bytes(&self) -> usize {
        self.pending_bytes.load(Ordering::Relaxed)
    }

    /// Returns true if there are pending operations.
    pub fn has_pending(&self) -> bool {
        self.pending_count.load(Ordering::Relaxed) > 0
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
        let mut batch = self.batch.lock();
        *batch = WriteBatch::default();
        self.pending_count.store(0, Ordering::Relaxed);
        self.pending_bytes.store(0, Ordering::Relaxed);
        self.stats.set_pending(0);
        debug!(target: "neo", "write batch cleared");
    }
}

/// Auto-flushing write batch buffer with background timer.
#[cfg(feature = "rocksdb")]
pub struct AutoFlushBatchBuffer {
    inner: Arc<WriteBatchBuffer>,
}

#[cfg(feature = "rocksdb")]
impl AutoFlushBatchBuffer {
    /// Creates a new auto-flushing batch buffer.
    pub fn new(db: Arc<DB>, config: WriteBatchConfig) -> Self {
        let inner = Arc::new(WriteBatchBuffer::new(db, config));

        // Start background flush task
        let inner_clone = Arc::clone(&inner);
        std::thread::spawn(move || {
            Self::flush_loop(inner_clone, config.max_delay_ms);
        });

        Self { inner }
    }

    /// Background flush loop.
    fn flush_loop(inner: Arc<WriteBatchBuffer>, interval_ms: u64) {
        let interval = Duration::from_millis(interval_ms);

        loop {
            std::thread::sleep(interval);

            // Check if we should flush based on time
            if inner.has_pending() && inner.time_since_flush() >= interval {
                if let Err(e) = inner.flush() {
                    error!(target: "neo", error = %e, "auto flush failed");
                }
            }
        }
    }

    /// Gets the inner batch buffer.
    pub fn inner(&self) -> &WriteBatchBuffer {
        &self.inner
    }

    /// Adds a put operation.
    pub fn put(&self, key: &[u8], value: &[u8]) {
        self.inner.put(key, value);
    }

    /// Adds a delete operation.
    pub fn delete(&self, key: &[u8]) {
        self.inner.delete(key);
    }

    /// Forces a flush.
    pub fn flush(&self) -> CoreResult<()> {
        self.inner.force_flush()
    }
}

#[cfg(all(test, feature = "rocksdb"))]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn create_test_db() -> (Arc<DB>, TempDir) {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("test_db");

        let mut opts = rocksdb::Options::default();
        opts.create_if_missing(true);

        let db = Arc::new(DB::open(&opts, &path).unwrap());
        (db, tmp)
    }

    #[test]
    fn write_batch_buffer_put_and_flush() {
        let (db, _tmp) = create_test_db();
        let buffer = WriteBatchBuffer::with_defaults(db);

        buffer.put(b"key1", b"value1");
        buffer.put(b"key2", b"value2");

        assert_eq!(buffer.pending_count(), 2);

        buffer.flush().unwrap();

        assert_eq!(buffer.pending_count(), 0);

        let stats = buffer.stats_snapshot();
        assert_eq!(stats.batches_flushed, 1);
        assert_eq!(stats.operations_written, 2);
    }

    #[test]
    fn write_batch_buffer_delete() {
        let (db, _tmp) = create_test_db();
        let buffer = WriteBatchBuffer::with_defaults(db.clone());

        // First put a value
        buffer.put(b"key1", b"value1");
        buffer.flush().unwrap();

        // Then delete it
        buffer.delete(b"key1");
        buffer.flush().unwrap();

        // Verify it's gone
        let result = db.get(b"key1").unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn write_batch_buffer_auto_flush_on_size() {
        let (db, _tmp) = create_test_db();
        let config = WriteBatchConfig {
            max_batch_size: 5,
            max_delay_ms: 10000, // Long delay to ensure size triggers flush
            min_operations: 10,
            max_batch_bytes: 1024 * 1024,
            sync_on_flush: false,
            disable_wal: true,
        };

        let buffer = WriteBatchBuffer::new(db, config);

        // Add 4 items - should not flush yet
        for i in 0..4 {
            buffer.put(format!("key{}", i).as_bytes(), b"value");
        }

        assert_eq!(buffer.pending_count(), 4);

        // Add 5th item - should trigger auto-flush
        buffer.put(b"key5", b"value");

        // May need a small delay for the flush to complete
        std::thread::sleep(Duration::from_millis(10));

        // Should be flushed or very close to it
        assert!(buffer.pending_count() < 5);
    }

    #[test]
    fn write_batch_buffer_clear() {
        let (db, _tmp) = create_test_db();
        let buffer = WriteBatchBuffer::with_defaults(db);

        buffer.put(b"key1", b"value1");
        buffer.put(b"key2", b"value2");

        assert_eq!(buffer.pending_count(), 2);

        buffer.clear();

        assert_eq!(buffer.pending_count(), 0);
    }

    #[test]
    fn write_batch_stats_snapshot() {
        let stats = WriteBatchStats::new();

        stats.record_flush(10, 1000, 5);
        stats.record_flush(20, 2000, 10);

        let snapshot = stats.snapshot();

        assert_eq!(snapshot.batches_flushed, 2);
        assert_eq!(snapshot.operations_written, 30);
        assert_eq!(snapshot.bytes_written, 3000);
        assert_eq!(snapshot.total_flush_duration_ms, 15);

        assert_eq!(snapshot.avg_ops_per_flush(), 15.0);
        assert_eq!(snapshot.avg_bytes_per_flush(), 1500.0);
        assert_eq!(snapshot.avg_flush_duration_ms(), 7.5);
    }

    #[test]
    fn write_batch_config_presets() {
        let high_throughput = WriteBatchConfig::high_throughput();
        assert_eq!(high_throughput.max_batch_size, 5000);
        assert!(high_throughput.disable_wal);

        let durable = WriteBatchConfig::durable();
        assert_eq!(durable.max_batch_size, 100);
        assert!(durable.sync_on_flush);
        assert!(!durable.disable_wal);

        let balanced = WriteBatchConfig::balanced();
        assert_eq!(balanced.max_batch_size, 500);
    }
}
