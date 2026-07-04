use super::{
    read_only_store::{RawReadOnlyStore, ReadOnlyStore},
    store_snapshot::StoreSnapshot,
    write_store::WriteStore,
};
use crate::error::StorageResult;
use std::any::Any;
use std::sync::Arc;

use super::fast_sync_store::FastSyncStore;
use super::raw_overlay_store::RawOverlayStore;

/// Delegate for OnNewSnapshot event
pub type OnNewSnapshotDelegate = Box<dyn Fn(&dyn Store, Arc<dyn StoreSnapshot>) + Send + Sync>;

/// This interface provides methods for reading, writing from/to database.
/// Developers should implement this interface to provide new storage engines for NEO.
///
/// # Concerns
///
/// The `Store` trait covers four concerns:
/// - **Read** — via `ReadOnlyStore` + `RawReadOnlyStore` supertraits
/// - **Write** — via `WriteStore` supertrait + `flush()`
/// - **Snapshot** — `snapshot()` + `on_new_snapshot()`
/// - **Downcast** — `as_any()`
///
/// Two additional concerns live in separate extension traits (ADR-020):
/// - **Fast-sync** — [`FastSyncStore`] (WAL disabling, buffered writes).
///   Accessed via [`Store::as_fast_sync_store`].
/// - **Raw overlay** — [`RawOverlayStore`] (direct overlay commit without
///   snapshot). Accessed via [`Store::as_raw_overlay_store`].
pub trait Store:
    ReadOnlyStore
    + RawReadOnlyStore
    + WriteStore<Vec<u8>, Vec<u8>>
    + Send
    + Sync
    + std::fmt::Debug
    + Any
{
    /// Creates a snapshot of the database.
    fn snapshot(&self) -> Arc<dyn StoreSnapshot>;

    /// Event raised when a new snapshot is created
    fn on_new_snapshot(&self, handler: OnNewSnapshotDelegate);

    /// Flushes pending writes to durable storage when supported.
    ///
    /// Returns an error if the backend fails to persist pending writes so that
    /// callers can react to durability failures instead of silently losing data.
    fn flush(&self) -> StorageResult<()> {
        Ok(())
    }

    /// Downcast support for concrete implementations.
    fn as_any(&self) -> &dyn Any;

    /// Returns a fast-sync extension handle if the backend supports fast-sync
    /// optimizations. Returns `None` for backends that don't implement
    /// [`FastSyncStore`].
    ///
    /// Backends that support fast-sync should override this to return
    /// `Some(self)`.
    fn as_fast_sync_store(&self) -> Option<&dyn FastSyncStore> {
        None
    }

    /// Returns a raw-overlay extension handle if the backend supports direct
    /// overlay commit. Returns `None` for backends that don't implement
    /// [`RawOverlayStore`].
    ///
    /// Backends that support raw overlay commit should override this to
    /// return `Some(self)`.
    fn as_raw_overlay_store(&self) -> Option<&dyn RawOverlayStore> {
        None
    }
}
