//! Legacy core-result facades over storage-owned write batching.

pub use neo_storage::persistence::write_batch_buffer::{
    WriteBatchConfig, WriteBatchStats, WriteBatchStatsSnapshot,
};

#[cfg(feature = "rocksdb")]
use neo_smart_contract::error::{CoreError, CoreResult};
#[cfg(feature = "rocksdb")]
use neo_storage::StorageError;
#[cfg(feature = "rocksdb")]
use neo_storage::persistence::write_batch_buffer as raw;
#[cfg(feature = "rocksdb")]
use rocksdb::DB;
#[cfg(feature = "rocksdb")]
use std::{sync::Arc, time::Duration};

// The original flush API produced CoreError::Io directly. Do not use the
// blanket storage conversion, which would change the category and retryability.
#[cfg(feature = "rocksdb")]
pub(crate) fn map_storage_flush_error(error: StorageError) -> CoreError {
    match error {
        StorageError::Io { message } => CoreError::Io { message },
        StorageError::KeyNotFound { key } => CoreError::InvalidOperation {
            message: StorageError::KeyNotFound { key }.to_string(),
        },
        StorageError::ReadOnly => CoreError::InvalidOperation {
            message: StorageError::ReadOnly.to_string(),
        },
        StorageError::Serialization { message } => CoreError::InvalidOperation {
            message: StorageError::Serialization { message }.to_string(),
        },
        StorageError::Backend { message } => CoreError::InvalidOperation {
            message: StorageError::Backend { message }.to_string(),
        },
        StorageError::InvalidOperation { message } => CoreError::InvalidOperation {
            message: StorageError::InvalidOperation { message }.to_string(),
        },
        StorageError::CommitFailed(message) => CoreError::InvalidOperation {
            message: StorageError::CommitFailed(message).to_string(),
        },
    }
}

/// Compatibility facade; buffering and RocksDB ownership live in neo-storage.
#[cfg(feature = "rocksdb")]
pub struct WriteBatchBuffer {
    inner: Arc<raw::WriteBatchBuffer>,
}

#[cfg(feature = "rocksdb")]
impl WriteBatchBuffer {
    /// Creates a new write batch buffer.
    pub fn new(db: Arc<DB>, config: WriteBatchConfig) -> Self {
        Self {
            inner: Arc::new(raw::WriteBatchBuffer::new(db, config)),
        }
    }

    /// Creates a new write batch buffer with default configuration.
    pub fn with_defaults(db: Arc<DB>) -> Self {
        Self::new(db, WriteBatchConfig::default())
    }

    /// Gets the statistics.
    pub fn stats(&self) -> Arc<WriteBatchStats> {
        self.inner.stats()
    }

    /// Gets a snapshot of statistics.
    pub fn stats_snapshot(&self) -> WriteBatchStatsSnapshot {
        self.inner.stats_snapshot()
    }

    /// Gets the configuration.
    pub fn config(&self) -> &WriteBatchConfig {
        self.inner.config()
    }

    /// Adds a put operation to the batch.
    pub fn put(&self, key: &[u8], value: &[u8]) {
        self.inner.put(key, value);
    }

    /// Adds a delete operation to the batch.
    pub fn delete(&self, key: &[u8]) {
        self.inner.delete(key);
    }

    /// Flushes the batch to the database using the legacy core error contract.
    pub fn flush(&self) -> CoreResult<()> {
        self.inner.flush().map_err(map_storage_flush_error)
    }

    /// Forces a flush regardless of batch size or time.
    pub fn force_flush(&self) -> CoreResult<()> {
        self.inner.force_flush().map_err(map_storage_flush_error)
    }

    /// Returns the number of pending operations.
    pub fn pending_count(&self) -> usize {
        self.inner.pending_count()
    }

    /// Returns the number of pending bytes.
    pub fn pending_bytes(&self) -> usize {
        self.inner.pending_bytes()
    }

    /// Returns true if there are pending operations.
    pub fn has_pending(&self) -> bool {
        self.inner.has_pending()
    }

    /// Returns the time since last flush.
    pub fn time_since_flush(&self) -> Duration {
        self.inner.time_since_flush()
    }

    /// Returns the duration of the last flush operation.
    pub fn last_flush_duration(&self) -> Duration {
        self.inner.last_flush_duration()
    }

    /// Clears all pending operations without flushing.
    pub fn clear(&self) {
        self.inner.clear();
    }
}

/// Compatibility facade over the storage-owned background flush loop.
#[cfg(feature = "rocksdb")]
pub struct AutoFlushBatchBuffer {
    auto: raw::AutoFlushBatchBuffer,
    inner: WriteBatchBuffer,
}

#[cfg(feature = "rocksdb")]
impl AutoFlushBatchBuffer {
    /// Creates a new auto-flushing batch buffer.
    pub fn new(db: Arc<DB>, config: WriteBatchConfig) -> Self {
        let auto = raw::AutoFlushBatchBuffer::new(db, config);
        let inner = WriteBatchBuffer {
            inner: auto.shared_inner(),
        };
        Self { auto, inner }
    }

    /// Gets the inner batch buffer with the legacy result signatures.
    pub fn inner(&self) -> &WriteBatchBuffer {
        &self.inner
    }

    /// Adds a put operation.
    pub fn put(&self, key: &[u8], value: &[u8]) {
        self.auto.put(key, value);
    }

    /// Adds a delete operation.
    pub fn delete(&self, key: &[u8]) {
        self.auto.delete(key);
    }

    /// Forces a flush.
    pub fn flush(&self) -> CoreResult<()> {
        self.auto.flush().map_err(map_storage_flush_error)
    }
}

#[cfg(all(test, feature = "rocksdb"))]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn batch_flush_compatibility_preserves_io_message_category_and_retryability() {
        let message = "RocksDB write batch flush failed: test failure".to_string();
        let error = map_storage_flush_error(StorageError::Io {
            message: message.clone(),
        });
        assert_eq!(error, CoreError::Io { message });
        assert_eq!(error.category(), "io");
        assert!(error.is_retryable());

        for error in [
            StorageError::KeyNotFound { key: "key".into() },
            StorageError::ReadOnly,
            StorageError::Serialization {
                message: "codec".into(),
            },
            StorageError::Backend {
                message: "backend".into(),
            },
            StorageError::InvalidOperation {
                message: "operation".into(),
            },
            StorageError::CommitFailed("commit".into()),
        ] {
            let expected_message = error.to_string();
            let mapped = map_storage_flush_error(error);
            assert_eq!(
                mapped,
                CoreError::InvalidOperation {
                    message: expected_message
                }
            );
            assert!(!mapped.is_retryable());
        }
    }

    #[test]
    fn batch_facade_flushes_the_storage_owned_buffer() {
        let tmp = TempDir::new().unwrap();
        let mut options = rocksdb::Options::default();
        options.create_if_missing(true);
        let db = Arc::new(DB::open(&options, tmp.path().join("batch-compat")).unwrap());
        let buffer = WriteBatchBuffer::new(Arc::clone(&db), WriteBatchConfig::durable());
        buffer.put(b"key", b"value");
        let result: CoreResult<()> = buffer.force_flush();
        result.unwrap();
        assert_eq!(db.get(b"key").unwrap(), Some(b"value".to_vec()));
        assert_eq!(buffer.pending_count(), 0);
        assert_eq!(buffer.stats_snapshot().operations_written, 1);
        let _: fn(&AutoFlushBatchBuffer) -> CoreResult<()> = AutoFlushBatchBuffer::flush;
        let _: fn(&AutoFlushBatchBuffer) -> &WriteBatchBuffer = AutoFlushBatchBuffer::inner;
    }

    #[test]
    fn batch_facade_read_only_flush_failure_keeps_core_io() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("batch-read-only-compat");
        let mut options = rocksdb::Options::default();
        options.create_if_missing(true);
        drop(DB::open(&options, &path).unwrap());
        let db = Arc::new(DB::open_for_read_only(&options, &path, false).unwrap());
        let buffer = WriteBatchBuffer::with_defaults(db);
        buffer.put(b"key", b"value");
        let error = buffer.flush().unwrap_err();
        match &error {
            CoreError::Io { message } => {
                assert!(message.starts_with("RocksDB write batch flush failed: "));
            }
            other => panic!("unexpected flush error: {other}"),
        }
        assert_eq!(error.category(), "io");
        assert!(error.is_retryable());
    }
}
