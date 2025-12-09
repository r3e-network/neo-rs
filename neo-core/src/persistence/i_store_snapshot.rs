// Copyright (C) 2015-2025 The Neo Project.
//
// i_store_snapshot.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use super::{
    i_read_only_store::IReadOnlyStoreGeneric, i_store::IStore, i_write_store::IWriteStore,
    storage::StorageError,
};
use std::sync::Arc;

/// Result type for snapshot commit operations.
pub type SnapshotCommitResult = Result<(), StorageError>;

/// This interface provides methods for reading, writing, and committing from/to snapshot.
pub trait IStoreSnapshot:
    IReadOnlyStoreGeneric<Vec<u8>, Vec<u8>> + IWriteStore<Vec<u8>, Vec<u8>> + Send + Sync
{
    /// Get the underlying store
    fn store(&self) -> Arc<dyn IStore>;

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
}
