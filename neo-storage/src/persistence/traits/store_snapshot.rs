use super::{
    read_only_store::{RawReadOnlyStore, ReadOnlyStoreGeneric},
    store::Store,
    write_store::WriteStore,
};
use crate::error::StorageError;
use std::sync::Arc;

/// Result type for snapshot commit operations.
pub type SnapshotCommitResult = Result<(), StorageError>;

/// Point-in-time mutable view over a concrete storage backend.
///
/// Snapshots stay typed to their backend so hot storage paths do not erase the
/// store behind a `Store` trait object. Runtime-selected backends should expose a concrete
/// enum snapshot instead of returning a trait object.
pub trait StoreSnapshot:
    ReadOnlyStoreGeneric<Vec<u8>, Vec<u8>>
    + RawReadOnlyStore
    + WriteStore<Vec<u8>, Vec<u8>>
    + Send
    + Sync
    + std::fmt::Debug
    + Sized
{
    /// Concrete store type that can create more snapshots of this shape.
    type Store: Store<Snapshot = Self>;

    /// Gets the underlying concrete store.
    fn store(&self) -> Arc<Self::Store>;

    /// Commits all changes in the snapshot to the database, returning an error on failure.
    /// Backend failures are always explicit because silently accepting an
    /// uncommitted snapshot can publish an invalid canonical outcome.
    fn try_commit(&mut self) -> SnapshotCommitResult;
}
