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

    /// Writes entries whose final bytes depend on the value currently stored
    /// under the same key (for example reference-counted state-service MPT
    /// nodes) by reading and replacing them at the write cursor.
    ///
    /// Backends that support cursor-fused commits call this once per commit,
    /// after [`RawOverlaySource::visit_raw_overlay`] has been fully consumed
    /// and before the transaction commits. Entries must be processed in raw
    /// byte-key order. Any error aborts the commit before publish. The
    /// default source has no cursor-resolved entries.
    fn commit_raw_overlay_at_cursor(
        &mut self,
        cursor: &mut dyn RawOverlayCursor,
    ) -> StorageResult<()> {
        let _ = cursor;
        Ok(())
    }
}

/// Cursor facade handed to [`RawOverlaySource::commit_raw_overlay_at_cursor`]
/// so an overlay source can resolve entries against the rows already stored
/// in the table being written.
///
/// Implementations drive a write cursor in raw byte-key order. Sources whose
/// absent value is known up front should use [`Self::insert_stored_if_absent`]
/// so a backend can combine the absence probe and insert. When that method
/// returns an existing value, the immediately following `write_stored` for the
/// same key may replace the positioned row in place.
pub trait RawOverlayCursor {
    /// Returns the value currently stored for `key`, or `None` when absent.
    fn read_stored(&mut self, key: &[u8]) -> StorageResult<Option<Vec<u8>>>;

    /// Writes the final `value` for `key`, replacing any probed row.
    fn write_stored(&mut self, key: &[u8], value: &[u8]) -> StorageResult<()>;

    /// Inserts `absent_value` when `key` is absent, otherwise returns the
    /// existing value with the cursor positioned for a following
    /// [`Self::write_stored`].
    ///
    /// `Ok(None)` means the supplied value was inserted. `Ok(Some(_))` means
    /// no write occurred. The default preserves compatibility by composing
    /// the ordinary read and write operations; persistent backends can
    /// override it with a single insert-if-absent search.
    fn insert_stored_if_absent(
        &mut self,
        key: &[u8],
        absent_value: &[u8],
    ) -> StorageResult<Option<Vec<u8>>> {
        match self.read_stored(key)? {
            Some(stored) => Ok(Some(stored)),
            None => {
                self.write_stored(key, absent_value)?;
                Ok(None)
            }
        }
    }
}

/// Secondary-overlay entries captured during a coordinated commit, in the
/// order the backend wrote them (visited entries first, cursor-resolved
/// entries second), handed to a shadow dual-writer.
pub type ShadowOverlayEntries = Vec<(Vec<u8>, Option<Vec<u8>>)>;

/// Maintenance-table row a shadow dual-writer asks the canonical transaction
/// to persist atomically with the overlays it mirrors.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShadowCommitMarker {
    /// Maintenance-table key.
    pub key: Vec<u8>,
    /// Maintenance-table value.
    pub value: Vec<u8>,
}

/// Result of mirroring one secondary overlay inside a canonical transaction.
///
/// A degraded result still carries a maintenance marker. This is deliberate:
/// the canonical transaction may continue, but the failed shadow history must
/// be durably poisoned so a later process cannot resume after a missing window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShadowCommitOutcome {
    /// The overlay carried no rows owned by the shadow.
    Unchanged,
    /// The shadow bytes are durable and this high-water marker may be committed.
    Prepared(ShadowCommitMarker),
    /// The shadow failed; commit `marker` atomically and report `error`.
    Degraded {
        /// Durable fail-closed marker for the incomplete shadow history.
        marker: ShadowCommitMarker,
        /// Bounded diagnostic text logged by the storage backend.
        error: String,
    },
}

/// Mandatory maintenance marker committed with coordinated overlays.
///
/// This uses the same key/value carrier as shadow publication, but callers of
/// the required-marker API receive strict all-or-nothing semantics instead of
/// shadow mode's best-effort failure policy.
pub type CoordinatedCommitMarker = ShadowCommitMarker;

/// Shadow dual-write hook invoked inside a coordinated commit after both
/// overlays are applied and before the transaction commits.
///
/// The hook receives the captured secondary-overlay entries and returns an
/// explicit outcome. A failure must carry a degraded marker so the canonical
/// transaction records that the shadow history is incomplete. Backends log and
/// count degraded outcomes while continuing the authoritative commit.
pub type ShadowCommitHook<'a> = dyn FnMut(ShadowOverlayEntries) -> ShadowCommitOutcome + Send + 'a;

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

    /// Returns whether this backend's raw-overlay commit paths invoke
    /// [`RawOverlaySource::commit_raw_overlay_at_cursor`], letting overlay
    /// sources resolve values already stored in the table at the write cursor
    /// instead of pre-resolving them through a separate read sweep.
    fn supports_raw_overlay_cursor(&self) -> bool {
        false
    }
}
