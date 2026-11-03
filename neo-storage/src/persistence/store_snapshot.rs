use super::{read_only_store::ReadOnlyStoreGeneric, store::Store, write_store::WriteStore};
use crate::error::StorageError;
use std::sync::Arc;

/// Result type for snapshot commit operations.
pub type SnapshotCommitResult = Result<(), StorageError>;

/// This interface provides methods for reading, writing, and committing from/to snapshot.
pub trait StoreSnapshot:
    ReadOnlyStoreGeneric<Vec<u8>, Vec<u8>> + WriteStore<Vec<u8>, Vec<u8>> + Send + Sync
{
    /// Get the underlying store
    fn store(&self) -> Arc<dyn Store>;

    /// Commits all changes in the snapshot to the database.
    ///
    /// DEPRECATED: Use `try_commit()` instead to properly handle errors.
    /// This method exists for backward compatibility and will log errors but not propagate them.
    fn commit(&mut self) {
        if let Err(e) = self.try_commit() {
            tracing::error!(target: "neo::storage", error = %e, "snapshot commit failed");
        }
    }

    /// Commits all changes in the snapshot to the database, returning an error on failure.
    ///
    /// SECURITY: This method should be used instead of `commit()` to ensure storage errors
    /// are properly handled and not silently ignored, which could lead to data loss or
    /// blockchain state inconsistency.
    fn try_commit(&mut self) -> SnapshotCommitResult;

    /// Commits with synchronous durability and WAL enabled.
    ///
    /// Backends must override this method explicitly; falling back to a normal
    /// commit would make consensus persistence configuration unsafe.
    fn try_commit_durable(&mut self) -> SnapshotCommitResult {
        Err(StorageError::InvalidOperation {
            message: "snapshot backend does not support durable commits".to_string(),
        })
    }
}
