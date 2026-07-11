use super::{
    read_only_store::{RawReadOnlyStore, ReadOnlyStore},
    store_snapshot::StoreSnapshot,
    write_store::WriteStore,
};
use crate::error::StorageResult;
use crate::persistence::store_maintenance::StoreMaintenanceBatch;
use std::sync::Arc;

/// Stable identifier for a store backend selected through the provider/factory
/// layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoreBackendKind {
    /// Ephemeral in-memory store used by tests and remote-ledger mode.
    Memory,
    /// Production MDBX store.
    Mdbx,
    /// RocksDB compatibility/backend store.
    RocksDb,
    /// A custom store implementation outside the built-in backend set.
    Custom(&'static str),
}

impl StoreBackendKind {
    /// Returns the stable backend label used in diagnostics and metrics.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Memory => "memory",
            Self::Mdbx => "mdbx",
            Self::RocksDb => "rocksdb",
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

/// RocksDB fast-sync batch diagnostics projected without exposing the concrete
/// store type to callers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RocksDbBatchMetrics {
    /// Current write operations buffered before RocksDB flush.
    pub pending_operations: u64,
    /// Total RocksDB write batches flushed by fast-sync buffering.
    pub batches_flushed: u64,
    /// Total put/delete operations flushed through RocksDB write batches.
    pub operations_written: u64,
    /// Approximate payload bytes flushed through RocksDB write batches.
    pub bytes_written: u64,
    /// Total RocksDB write-batch flush timeout observations.
    pub flush_timeouts: u64,
    /// Average write operations per flushed RocksDB batch.
    pub avg_ops_per_flush: u64,
    /// Average payload bytes per flushed RocksDB batch.
    pub avg_bytes_per_flush: u64,
    /// Average RocksDB write-batch flush duration in milliseconds.
    pub avg_flush_duration_ms: u64,
    /// Active RocksDB write-batch operation threshold.
    pub max_batch_size: u64,
    /// Active RocksDB write-batch byte threshold.
    pub max_batch_bytes: u64,
    /// Whether RocksDB WAL is disabled for fast-sync batch writes.
    pub disable_wal: bool,
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
/// Additional backend capabilities are exposed as default methods: fast-sync
/// controls, direct raw-overlay commits, isolated maintenance metadata, and
/// backend metrics. Most are optional. Atomic durable-overlay commit is
/// mandatory when a store backs the canonical chain writer; its default
/// `Ok(false)` only permits non-canonical implementations to compile without
/// claiming a durability guarantee.
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

    /// Returns RocksDB fast-sync batch diagnostics when this store is backed by
    /// RocksDB.
    fn rocksdb_batch_metrics(&self) -> Option<RocksDbBatchMetrics> {
        None
    }

    /// Returns whether this backend implements storage-level fast-sync
    /// optimizations.
    fn supports_fast_sync_mode(&self) -> bool {
        false
    }

    /// Enables storage-level fast-sync optimizations when this backend supports
    /// them. Backends that do not support fast-sync mode leave this as a no-op.
    fn enable_fast_sync_mode(&self) {}

    /// Disables storage-level fast-sync optimizations, restoring normal
    /// durability guarantees when this backend supports them.
    fn disable_fast_sync_mode(&self) {}

    /// Drops pending fast-sync buffered writes that have not reached durable
    /// storage. Backends without buffered fast-sync writes leave this as a
    /// no-op.
    fn discard_pending_fast_sync_writes(&self) {}

    /// Returns whether fast-sync writes have been accepted by the backend but
    /// are not guaranteed visible through fresh snapshots yet.
    fn has_pending_fast_sync_writes(&self) -> bool {
        false
    }

    /// Commits raw byte-key overlay entries directly when the backend can do so
    /// without constructing a mutable snapshot.
    ///
    /// Implementations may sort this materialized overlay by raw key before
    /// writing so B+tree and LSM backends receive locality-friendly batches.
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

    /// Commits a borrowed overlay as one durable backend transaction.
    ///
    /// Canonical-tip publishers use this capability to bypass throughput
    /// buffers whose writes may be visible before their durability fence. A
    /// backend returns `Ok(false)` when it has no stronger implementation.
    /// Canonical publishers must reject that result: commit-then-flush cannot
    /// roll back a transaction when the later flush fails.
    fn try_commit_durable_borrowed_raw_overlay<O>(&self, overlay: &mut O) -> StorageResult<bool>
    where
        O: RawOverlaySource + ?Sized,
    {
        let _ = overlay;
        Ok(false)
    }

    /// Reads one value from the backend's isolated maintenance namespace.
    ///
    /// Persistent backends keep this namespace outside the normal Neo data
    /// table so these bytes never appear in contract-storage scans or state
    /// roots. Backends without that capability return `Ok(None)`.
    fn maintenance_metadata(&self, key: &[u8]) -> StorageResult<Option<Vec<u8>>> {
        let _ = key;
        Ok(None)
    }

    /// Atomically and durably applies normal data operations together with
    /// isolated maintenance-metadata operations.
    ///
    /// A backend returning `Ok(false)` does not provide the cross-namespace
    /// transaction guarantee. Callers must not advance a maintenance
    /// checkpoint through a non-atomic fallback.
    fn try_commit_durable_maintenance(&self, batch: &StoreMaintenanceBatch) -> StorageResult<bool> {
        let _ = batch;
        Ok(false)
    }
}
