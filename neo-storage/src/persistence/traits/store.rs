use super::{
    read_only_store::{RawReadOnlyStore, ReadOnlyStore},
    store_snapshot::StoreSnapshot,
    write_store::WriteStore,
};
use crate::error::StorageResult;
use std::any::Any;
use std::sync::Arc;

/// Delegate for OnNewSnapshot event
pub type OnNewSnapshotDelegate = Box<dyn Fn(&dyn Store, Arc<dyn StoreSnapshot>) + Send + Sync>;

/// This interface provides methods for reading, writing from/to database.
/// Developers should implement this interface to provide new storage engines for NEO.
pub trait Store:
    ReadOnlyStore + RawReadOnlyStore + WriteStore<Vec<u8>, Vec<u8>> + Send + Sync + Any
{
    /// Creates a snapshot of the database.
    fn snapshot(&self) -> Arc<dyn StoreSnapshot>;

    /// Event raised when a new snapshot is created
    fn on_new_snapshot(&self, handler: OnNewSnapshotDelegate);

    /// Enables storage-level fast-sync optimizations when supported.
    fn enable_fast_sync_mode(&self) {}

    /// Disables storage-level fast-sync optimizations.
    fn disable_fast_sync_mode(&self) {}

    /// Drops pending fast-sync buffered writes that have not reached durable
    /// storage. Used only when an import aborts before its accepted prefix is
    /// finalized; successful imports must flush instead.
    fn discard_pending_fast_sync_writes(&self) {}

    /// Returns whether fast-sync writes have been accepted by the backend but
    /// are not guaranteed visible through fresh snapshots yet.
    fn has_pending_fast_sync_writes(&self) -> bool {
        false
    }

    /// Flushes pending writes to durable storage when supported.
    ///
    /// Returns an error if the backend fails to persist pending writes so that
    /// callers can react to durability failures instead of silently losing data.
    fn flush(&self) -> StorageResult<()> {
        Ok(())
    }

    /// Returns whether this store can consume a materialized raw byte-key
    /// overlay via [`Store::try_commit_raw_overlay`].
    ///
    /// The default is `false` so callers do not have to clone a large change
    /// set merely to discover that the backend will reject it.
    fn supports_raw_overlay_commit(&self) -> bool {
        false
    }

    /// Commits raw byte-key overlay entries directly when the backend can do so
    /// without constructing a mutable snapshot. Backends that do not support a
    /// direct overlay commit should return `Ok(false)` so callers can fall back
    /// to [`Store::snapshot`].
    ///
    /// Implementations may sort this materialized overlay by raw key before
    /// writing so B+tree and LSM backends receive locality-friendly batches.
    fn try_commit_raw_overlay(
        &self,
        _overlay: &[(Vec<u8>, Option<Vec<u8>>)],
    ) -> StorageResult<bool> {
        Ok(false)
    }

    /// Commits raw byte-key overlay entries from a borrowed visitor when the
    /// backend can consume the changes without the caller first cloning them
    /// into a `Vec`.
    ///
    /// Callers should visit entries in raw byte-key order. `StoreCache`
    /// satisfies this contract through `DataCache::visit_raw_changes`, keeping
    /// the hot commit path sorted without forcing every backend to clone the
    /// overlay just to sort it again.
    ///
    /// Implementations should return `Ok(false)` when unsupported so callers
    /// can fall back to [`Store::try_commit_raw_overlay`] or snapshots.
    fn try_commit_borrowed_raw_overlay(
        &self,
        _visit: &mut dyn FnMut(&mut dyn FnMut(&[u8], Option<&[u8]>)),
    ) -> StorageResult<bool> {
        Ok(false)
    }

    /// Downcast support for concrete implementations.
    fn as_any(&self) -> &dyn Any;
}
