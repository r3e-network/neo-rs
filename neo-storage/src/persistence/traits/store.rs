use super::{
    read_only_store::{RawReadOnlyStore, ReadOnlyStore},
    store_snapshot::StoreSnapshot,
    write_store::WriteStore,
};
use crate::error::StorageResult;
use std::sync::Arc;

/// Stable identifier for a store backend selected through the provider/factory
/// layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoreBackendKind {
    /// Ephemeral in-memory store used by tests and remote-ledger mode.
    Memory,
    /// Production MDBX store.
    Mdbx,
    /// A custom store implementation outside the built-in backend set.
    Custom(&'static str),
}

impl StoreBackendKind {
    /// Returns the stable backend label used in diagnostics and metrics.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Memory => "memory",
            Self::Mdbx => "mdbx",
            Self::Custom(name) => name,
        }
    }
}

/// MDBX environment information projected into storage-owned diagnostics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MdbxEnvironmentInfo {
    /// Current MDBX memory-map size in bytes.
    pub map_size: usize,
    /// Last used MDBX page number.
    pub last_pgno: usize,
    /// Last committed MDBX transaction id.
    pub last_txnid: usize,
    /// Configured MDBX reader slot capacity.
    pub max_readers: usize,
    /// MDBX reader slots currently used.
    pub num_readers: usize,
}

/// Sink for ordered raw byte-key overlay entries.
pub trait RawOverlaySink {
    /// Receives a put (`Some(value)`) or delete (`None`) operation.
    fn visit(&mut self, key: &[u8], value: Option<&[u8]>);
}

impl<F> RawOverlaySink for F
where
    F: FnMut(&[u8], Option<&[u8]>),
{
    fn visit(&mut self, key: &[u8], value: Option<&[u8]>) {
        self(key, value);
    }
}

/// Source of ordered raw byte-key overlay entries.
pub trait RawOverlaySource {
    /// Emits put/delete entries into `sink`.
    fn visit_raw_overlay<S>(&mut self, sink: &mut S)
    where
        S: RawOverlaySink + ?Sized;
}

/// This interface provides methods for reading, writing from/to database.
/// Developers should implement this interface to provide new storage engines for NEO.
///
/// # Concerns
///
/// The `Store` trait covers four concerns:
/// - **Read** — via `ReadOnlyStore` + `RawReadOnlyStore` supertraits
/// - **Write** — via `WriteStore` supertrait + `flush()`
/// - **Snapshot** — concrete `Snapshot` associated type + `snapshot()`
/// - **Backend capabilities** — explicit backend identity and optional metrics
///
/// Optional backend diagnostics and direct raw-overlay commits are exposed as
/// default methods. Canonical atomic commits and isolated maintenance metadata
/// belong to the stronger
/// [`super::transactional_store::TransactionalStore`] contract, so node
/// composition cannot discover those requirements at runtime.
pub trait Store:
    ReadOnlyStore
    + RawReadOnlyStore
    + WriteStore<Vec<u8>, Vec<u8>>
    + Send
    + Sync
    + std::fmt::Debug
    + 'static
{
    /// Concrete point-in-time snapshot produced by this backend.
    type Snapshot: StoreSnapshot;

    /// Creates a snapshot of the database.
    fn snapshot(&self) -> Arc<Self::Snapshot>;

    /// Flushes pending writes to durable storage when supported.
    ///
    /// Returns an error if the backend fails to persist pending writes so that
    /// callers can react to durability failures instead of silently losing data.
    fn flush(&self) -> StorageResult<()> {
        Ok(())
    }

    /// Returns the selected backend identity without requiring callers to
    /// downcast the store.
    fn backend_kind(&self) -> StoreBackendKind {
        StoreBackendKind::Custom("custom")
    }

    /// Returns MDBX environment information when this store is backed by MDBX.
    fn mdbx_environment_info(&self) -> Option<StorageResult<MdbxEnvironmentInfo>> {
        None
    }

    /// Commits raw byte-key overlay entries directly when the backend can do so
    /// without constructing a mutable snapshot.
    ///
    /// Implementations may sort this materialized overlay by raw key before
    /// writing so persistent backends receive locality-friendly batches.
    /// Backends that do not support a direct overlay commit return `Ok(false)`
    /// so callers can fall back to snapshot-based commit.
    fn try_commit_raw_overlay(
        &self,
        overlay: &[(Vec<u8>, Option<Vec<u8>>)],
    ) -> StorageResult<bool> {
        let _ = overlay;
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
    fn try_commit_borrowed_raw_overlay<O>(&self, overlay: &mut O) -> StorageResult<bool>
    where
        O: RawOverlaySource + ?Sized,
    {
        let _ = overlay;
        Ok(false)
    }
}
