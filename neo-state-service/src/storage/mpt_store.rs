//! [`MptStore`] - persisted MPT-node + local-state-root storage for the
//! state service.
//!
//! Mirrors the storage layer of the C# `StateService` plugin
//! (`Storage/StateStore.cs` + `Storage/StateSnapshot.cs` +
//! `Storage/Keys.cs`, vendored under
//! `neo_csharp/src/Plugins/StateService`): one flat key/value namespace
//! holds
//!
//! - the MPT nodes, written by the [`Trie`] write-through cache under
//!   `0xf0 || node_hash` (the prefix is applied inside
//!   `neo_crypto::mpt_trie`, matching the C# `Cache` prefix);
//! - the per-block state-root records under `0x01 || index_be`
//!   ([`Keys::state_root`]), serialized in the C# `StateRoot` wire
//!   format (unsigned fields + a var-int witness count);
//! - the current local root index under `0x02`
//!   ([`Keys::CURRENT_LOCAL_ROOT_INDEX`]), little-endian `u32` exactly
//!   like the C# `BitConverter.GetBytes(uint)` write.
//!
//! The C# plugin opens a LevelDB store at `Data_MPT_{network}`; this
//! build can keep the namespace in process memory for tests or place it
//! behind the workspace [`Store`] provider boundary for production
//! backends such as MDBX. The byte layout is the C# one, so changing the
//! physical backend is a storage swap, not a format migration.
//!
//! Block-changeset application mirrors the C# commit pipeline:
//! `Blockchain.Committing` calls `UpdateLocalStateRootSnapshot(height,
//! changeSet)` (Put for Added/Changed, Delete for Deleted, then
//! `Trie.Root.Hash` becomes the block's state root) and
//! `Blockchain.Committed` calls `UpdateLocalStateRoot(height)` (trie +
//! snapshot commit). [`MptStore::apply_block_changes`] performs both
//! halves in one synchronous call, which is the seam the node's
//! persist pipeline is expected to drive.
//!
//! # Reader snapshot isolation
//!
//! C# readers never walk the live store: every RPC handler opens an
//! immutable LevelDB snapshot first (`StateStore.GetStoreSnapshot()`)
//! and builds the `Trie` over it, so a concurrent block commit (which,
//! without `FullState`, *prunes* superseded nodes) can never delete a
//! node out from under an in-flight read. This port reproduces that
//! with a copy-on-write generation map:
//!
//! - the key/value namespace is an `Arc<HashMap>` behind an `RwLock`;
//! - [`MptStore::snapshot`] clones the `Arc` under the read lock,
//!   yielding an [`MptReadSnapshot`] — a frozen, point-in-time view
//!   (the `GetStoreSnapshot` analogue) every read trie resolves from;
//! - the single writer ([`MptStore::apply_block_changes`]) stages all
//!   trie-node and state-root mutations in a private write batch and
//!   publishes them in one `Arc::make_mut` critical section: when no
//!   reader holds the previous generation the map is updated in place,
//!   otherwise it is cloned once and the old generation stays alive
//!   (and fully resolvable) until the last reader drops it.

use crate::Keys;
use crate::metrics::{StateRootApplyCountKind, StateRootApplyMetrics, StateRootApplyStage};
use crate::state_root::{CURRENT_VERSION, StateRoot};
use neo_crypto::mpt_trie::{
    MptError, MptMutationStats, MptResult, MptStoreSnapshot, Node, Trie, UnresolvedDeferredNode,
};
use neo_io::SerializableExtensions;
use neo_primitives::{UINT256_SIZE, UInt256};
use neo_storage::persistence::providers::memory_store::MemoryStore;
use neo_storage::persistence::{
    RawOverlayCursor, RawOverlaySink, RawOverlaySource, RawReadOnlyStore, Store, StoreSnapshot,
    WriteStore,
};
use neo_storage::{StorageError, StorageResult};
use parking_lot::{Mutex, RwLock};
use rustc_hash::FxHashMap;
use std::collections::HashMap;
use std::fs;
use std::sync::Arc;
use std::time::Instant;

mod node_source;
mod write_batch;
pub use node_source::{MptNodeReadGeneration, MptNodeReadSnapshot, MptNodeSnapshotFactory};
use write_batch::MptWriteBatch;

/// Transient MPT publications are keyed by prefixed SHA-256 node hashes plus a
/// small fixed set of StateService metadata keys. They do not need SipHash's
/// defense for attacker-selected hash-table keys and are sorted before commit.
type MptOverlay = FxHashMap<Vec<u8>, Option<Vec<u8>>>;

/// Size of the serialized unsigned `StateRoot` prefix:
/// `version (1) + index (4, LE) + root_hash (32)`.
const STATE_ROOT_UNSIGNED_LEN: usize = 1 + 4 + UINT256_SIZE;
/// Prefix of exact serialized MPT node rows in the StateService namespace.
pub const MPT_NODE_PREFIX: u8 = 0xf0;
/// Complete key length of a prefix plus UInt256 node hash row.
pub const MPT_NODE_KEY_BYTES: usize = 1 + UINT256_SIZE;
const DEFERRED_NODE_LOOKUP_MAX_KEYS: usize = 1024 * 1024;
const DEFERRED_NODE_LOOKUP_MAX_ESTIMATED_BYTES: usize = 256 * 1024 * 1024;
const SERIALIZED_NODE_FIXED_BYTES: usize = 1 + std::mem::size_of::<i32>();

/// MDBX named table used when StateService shares the canonical environment.
pub const MDBX_STATE_SERVICE_NAMESPACE: &str = "neo_state_service";

/// One storage mutation from a block's change set.
///
/// Mirrors the `TrackState` cases the C# `StateStore.
/// UpdateLocalStateRootSnapshot` consumes: `Added` / `Changed` both
/// become a trie `Put`, `Deleted` becomes a trie `Delete`. Use
/// [`crate::StateStore::apply_snapshot_changes`] for the canonical
/// block-snapshot projection that filters `None` entries and Ledger
/// native-contract records.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MptChange {
    /// Insert or update the value stored under `key` (the full
    /// storage-key bytes: little-endian `i32` contract id + key
    /// suffix, i.e. C# `StorageKey.ToArray()`).
    Put {
        /// Full storage-key bytes.
        key: Vec<u8>,
        /// Raw storage-item value bytes (C# `StorageItem.ToArray()`).
        value: Vec<u8>,
    },
    /// Remove the entry stored under `key`.
    Delete {
        /// Full storage-key bytes.
        key: Vec<u8>,
    },
}

/// Borrowed storage changes for one block in an ordered MPT batch.
pub(crate) struct MptBlockChanges<'a> {
    /// Block height these changes belong to.
    pub(crate) block_index: u32,
    /// Projected non-Ledger storage mutations for the block.
    pub(crate) changes: &'a [MptChange],
}

/// Persisted MPT-node + state-root store for the state service.
///
/// Thread-safe: readers take a point-in-time [`MptReadSnapshot`] (see
/// the module docs for the isolation design), trie commits and
/// state-root writes go through [`MptStore::apply_block_changes`],
/// which serializes writers behind a dedicated gate (the C# plugin
/// achieves the same single-writer discipline by running `StateStore`
/// as an actor).
pub struct MptStore<S: Store = MemoryStore> {
    /// Flat key/value namespace shared by MPT nodes and state-root
    /// records (the C# `IStore` equivalent). The `Arc` is the
    /// copy-on-write generation pointer: readers clone it to freeze a
    /// view, the writer republishes it atomically per applied block.
    kv: RwLock<Arc<HashMap<Vec<u8>, Option<Vec<u8>>>>>,
    /// Serializes block-changeset application.
    write_gate: Mutex<()>,
    /// Whether historical trie nodes are retained (C#
    /// `StateServiceSettings.FullState`, default `false`). With
    /// `false`, applying a block prunes the nodes the change set made
    /// unreachable, so only the *current* root stays resolvable.
    full_state: bool,
    /// Whether ordered full-state batches defer lookup work while retaining
    /// every serialized mutation. The default remains the C#-compatible eager
    /// storage policy.
    defer_full_state_finalization: bool,
    /// Cached current local root `(index, hash)` for hot contiguity checks.
    ///
    /// Durable state-root records remain the source of truth for historical
    /// reads and for opening/reopening stores; this cache mirrors only the
    /// current pointer so block import does not need to open a backing snapshot
    /// just to confirm the previous block root.
    latest_local_root: RwLock<Option<(u32, UInt256)>>,
    /// Optional durable backend for the same flat C# byte namespace.
    backing: Option<Arc<S>>,
    /// Optional authoritative source for the physically separated MPT node
    /// namespace. When present, node misses never fall back to `backing`.
    node_snapshots: Option<Arc<dyn MptNodeSnapshotFactory>>,
}

/// Immutable, point-in-time view of an [`MptStore`] — the analogue of
/// the C# `StateStore.GetStoreSnapshot()` LevelDB snapshot every read
/// path opens before walking the trie.
///
/// A snapshot observes the key/value namespace exactly as it was when
/// [`MptStore::snapshot`] was called: blocks applied afterwards
/// (including pruning-mode node deletion) are invisible, so a trie
/// walk over the snapshot can never lose nodes mid-traversal. The
/// trie-node map and the state-root records are captured together,
/// making `current_local_root_*` + `open_trie` reads mutually
/// consistent.
pub struct MptReadSnapshot<S: Store = MemoryStore> {
    /// Frozen generation of the key/value namespace.
    map: Arc<HashMap<Vec<u8>, Option<Vec<u8>>>>,
    /// Frozen durable snapshot used for entries not present in `map`.
    backing_snapshot: Option<Arc<S::Snapshot>>,
    /// Pinned authoritative node generation paired with this read view.
    node_snapshot: Option<Arc<dyn MptNodeReadSnapshot>>,
    /// Copied [`MptStore::full_state`] flag.
    full_state: bool,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct OverlayCounts {
    puts: u64,
    deletes: u64,
    node_puts: u64,
    node_deletes: u64,
    node_value_sizes: [u64; 8],
    node_value_bytes: [u64; 8],
}

impl OverlayCounts {
    fn record(&mut self, key: &[u8], value: Option<&[u8]>) {
        match (is_mpt_node_key(key), value) {
            (true, Some(value)) => self.record_node_put(value.len()),
            (true, None) => {
                self.record_delete();
                self.node_deletes += 1;
            }
            (false, Some(_)) => self.record_put(),
            (false, None) => self.record_delete(),
        }
    }

    fn record_put(&mut self) {
        self.puts += 1;
    }

    fn record_delete(&mut self) {
        self.deletes += 1;
    }

    fn record_node_put(&mut self, value_len: usize) {
        self.record_put();
        self.node_puts += 1;
        let bucket = node_value_size_bucket(value_len);
        self.node_value_sizes[bucket] += 1;
        self.node_value_bytes[bucket] += value_len as u64;
    }

    fn entries(self) -> u64 {
        self.puts + self.deletes
    }
}

#[inline]
/// Returns whether a raw StateService key is exactly prefix plus node hash.
pub fn is_mpt_node_key(key: &[u8]) -> bool {
    key.len() == MPT_NODE_KEY_BYTES && key.first() == Some(&MPT_NODE_PREFIX)
}

const fn node_value_size_bucket(value_len: usize) -> usize {
    match value_len {
        0..=64 => 0,
        65..=128 => 1,
        129..=256 => 2,
        257..=512 => 3,
        513..=1_024 => 4,
        1_025..=4_096 => 5,
        4_097..=16_384 => 6,
        _ => 7,
    }
}

#[cfg(test)]
mod overlay_count_tests {
    use super::{MPT_NODE_PREFIX, OverlayCounts};
    use neo_primitives::UINT256_SIZE;

    #[test]
    fn node_counts_exclude_metadata_and_cover_value_boundaries() {
        let mut counts = OverlayCounts::default();
        let mut node_key = [0u8; 1 + UINT256_SIZE];
        node_key[0] = MPT_NODE_PREFIX;

        for value_len in [64, 65, 129, 257, 513, 1_025, 4_097, 16_385] {
            let value = vec![0u8; value_len];
            counts.record(&node_key, Some(&value));
        }
        counts.record(&node_key, None);
        counts.record(&[MPT_NODE_PREFIX, 0x01], Some(&[0u8; 4]));
        counts.record(&[0x01], None);

        assert_eq!(counts.puts, 9);
        assert_eq!(counts.deletes, 2);
        assert_eq!(counts.node_puts, 8);
        assert_eq!(counts.node_deletes, 1);
        assert_eq!(counts.node_value_sizes, [1; 8]);
        assert_eq!(
            counts.node_value_bytes,
            [64, 65, 129, 257, 513, 1_025, 4_097, 16_385]
        );
    }
}

#[cfg(test)]
mod prepared_overlay_partition_tests {
    use super::*;

    fn node_key(tag: u8) -> Vec<u8> {
        let mut key = vec![tag; 1 + UINT256_SIZE];
        key[0] = MPT_NODE_PREFIX;
        key
    }

    #[test]
    fn prepared_overlay_exposes_exact_sorted_node_and_metadata_partitions() {
        let first_node = node_key(1);
        let second_node = node_key(2);
        let root_record = vec![0x01, 0, 0, 0, 7];
        let short_f0_metadata = vec![MPT_NODE_PREFIX, 0x01];
        let mut overlay = MptOverlay::default();
        overlay.insert(second_node.clone(), Some(b"second".to_vec()));
        overlay.insert(root_record.clone(), Some(b"root".to_vec()));
        overlay.insert(first_node.clone(), None);
        overlay.insert(short_f0_metadata.clone(), Some(b"metadata".to_vec()));

        let mut prepared = PreparedMptCommit::new(7, UInt256::default(), overlay, 1);
        let mut nodes = Vec::new();
        prepared.visit_materialized_node_overlay(&mut |key: &[u8], value: Option<&[u8]>| {
            nodes.push((key.to_vec(), value.map(<[u8]>::to_vec)));
        });
        assert_eq!(
            nodes,
            vec![
                (first_node.clone(), None),
                (second_node.clone(), Some(b"second".to_vec())),
            ]
        );

        let mut metadata = Vec::new();
        prepared.visit_metadata_overlay(&mut |key: &[u8], value: Option<&[u8]>| {
            metadata.push((key.to_vec(), value.map(<[u8]>::to_vec)));
        });
        assert_eq!(
            metadata,
            vec![
                (root_record.clone(), Some(b"root".to_vec())),
                (short_f0_metadata.clone(), Some(b"metadata".to_vec())),
            ]
        );
        assert!(prepared.unresolved_node_journal().is_empty());

        let mut combined = Vec::new();
        prepared.visit_raw_overlay(&mut |key: &[u8], value: Option<&[u8]>| {
            combined.push((key.to_vec(), value.map(<[u8]>::to_vec)));
        });
        assert!(combined.windows(2).all(|pair| pair[0].0 < pair[1].0));
        assert_eq!(combined.len(), nodes.len() + metadata.len());
    }
}

struct EmptyRootBatchOverlaySource<'a> {
    blocks: &'a [MptBlockChanges<'a>],
    root_hash: UInt256,
    empty_root_record: Option<&'a ([u8; 1 + UINT256_SIZE], Vec<u8>)>,
    current_index_value: [u8; 4],
    counts: OverlayCounts,
}

impl<'a> EmptyRootBatchOverlaySource<'a> {
    fn new(
        blocks: &'a [MptBlockChanges<'a>],
        root_hash: UInt256,
        empty_root_record: Option<&'a ([u8; 1 + UINT256_SIZE], Vec<u8>)>,
        current_index: u32,
    ) -> Self {
        Self {
            blocks,
            root_hash,
            empty_root_record,
            current_index_value: current_index.to_le_bytes(),
            counts: OverlayCounts::default(),
        }
    }
}

impl RawOverlaySource for EmptyRootBatchOverlaySource<'_> {
    fn visit_raw_overlay<S>(&mut self, sink: &mut S)
    where
        S: RawOverlaySink + ?Sized,
    {
        for block in self.blocks {
            let key = MptStore::<MemoryStore>::state_root_key_bytes(block.block_index);
            let value = MptStore::<MemoryStore>::encode_state_root_fields(
                block.block_index,
                self.root_hash,
            );
            self.counts.record_put();
            sink.visit(&key, Some(&value));
        }
        self.counts.record_put();
        sink.visit(
            Keys::CURRENT_LOCAL_ROOT_INDEX,
            Some(self.current_index_value.as_slice()),
        );
        if let Some((key, value)) = self.empty_root_record {
            self.counts.record_node_put(value.len());
            sink.visit(key.as_slice(), Some(value.as_slice()));
        }
    }
}

struct SortedOverlaySource<'a> {
    entries: &'a [(&'a Vec<u8>, &'a Option<Vec<u8>>)],
    counts: OverlayCounts,
}

/// StateService mutations prepared for an externally coordinated commit.
///
/// The value is exposed only as an ordered [`RawOverlaySource`]. Composition
/// code may place these bytes in the same physical transaction as canonical
/// Ledger bytes, but cannot alter the root/index metadata or publish the local
/// in-memory generation directly. The external coordinator remains responsible
/// for actually committing every visited byte atomically with its primary
/// overlay; `PreparedMptCommit` can detect a callback that never visits it, but
/// cannot roll back an external transaction after the callback returns.
pub struct PreparedMptCommit {
    block_index: u32,
    root_hash: UInt256,
    /// Exact materialized `0xf0 || node_hash` puts and tombstones.
    node_overlay: MptOverlay,
    /// State-root records, the current-root pointer, and future non-node rows.
    metadata_overlay: MptOverlay,
    /// Unresolved deferred full-state journal, ordered by raw key. Non-empty
    /// only when the batch ran in deferred full-state mode and either its
    /// backing store can resolve entries at a write cursor or it pinned a
    /// physically separate authoritative node generation.
    deferred_journal: Vec<UnresolvedDeferredNode>,
    /// Exact authoritative node generation used to prepare the deferred
    /// journal. Retaining it closes the prepare/commit generation race without
    /// holding the authority publication lock during trie mutation.
    deferred_node_snapshot: Option<Arc<dyn MptNodeReadSnapshot>>,
    /// Set once the journal has been resolved and written at the commit
    /// cursor. Verified fail-closed before publishing the local root.
    journal_committed_at_cursor: bool,
    /// Set once split authority has materialized the journal into exact node
    /// bytes. Resolution is deliberately one-shot because reference counts are
    /// additive.
    journal_materialized_from_snapshot: bool,
    samples: usize,
    counts: OverlayCounts,
    nodes_visited: bool,
    metadata_visited: bool,
}

/// Borrowed StateService metadata view used by split-store coordinators.
///
/// Visiting this source consumes only the metadata half of the prepared
/// publication. The node half must be consumed separately through
/// [`PreparedMptCommit::visit_materialized_node_overlay`].
pub struct PreparedMptMetadataOverlay<'a> {
    prepared: &'a mut PreparedMptCommit,
}

impl PreparedMptCommit {
    fn new(block_index: u32, root_hash: UInt256, overlay: MptOverlay, samples: usize) -> Self {
        let mut counts = OverlayCounts::default();
        for (key, value) in &overlay {
            counts.record(key, value.as_deref());
        }
        let (node_overlay, metadata_overlay) = partition_mpt_overlay(overlay);
        Self {
            block_index,
            root_hash,
            node_overlay,
            metadata_overlay,
            deferred_journal: Vec::new(),
            deferred_node_snapshot: None,
            journal_committed_at_cursor: false,
            journal_materialized_from_snapshot: false,
            samples: samples.max(1),
            counts,
            nodes_visited: false,
            metadata_visited: false,
        }
    }

    /// Attaches an unresolved deferred full-state journal for the commit
    /// cursor to resolve. The journal must be ordered by raw key and its keys
    /// must be disjoint from every key already present in `node_overlay`.
    fn with_deferred_journal(
        mut self,
        journal: Vec<UnresolvedDeferredNode>,
        node_snapshot: Option<Arc<dyn MptNodeReadSnapshot>>,
    ) -> Self {
        self.deferred_journal = journal;
        self.deferred_node_snapshot = node_snapshot;
        self
    }

    /// Returns the final block index represented by this overlay.
    pub fn block_index(&self) -> u32 {
        self.block_index
    }

    /// Returns the final StateService trie root represented by this overlay.
    pub fn root_hash(&self) -> UInt256 {
        self.root_hash
    }

    /// Visits sorted, already-materialized MPT node operations only.
    ///
    /// Deferred full-state journal entries are deliberately excluded because
    /// their exact reference-count bytes require a read from the authoritative
    /// prefix. Visiting this view consumes the node half of a split
    /// publication, even when the overlay is empty.
    pub fn visit_materialized_node_overlay<S>(&mut self, sink: &mut S)
    where
        S: RawOverlaySink + ?Sized,
    {
        visit_sorted_overlay(&self.node_overlay, sink);
        self.nodes_visited = true;
    }

    /// Borrows sorted non-node StateService metadata as a raw overlay source
    /// suitable for the canonical database transaction.
    ///
    /// Dropping the returned source without a visit leaves publication
    /// fail-closed.
    pub fn metadata_overlay_source(&mut self) -> PreparedMptMetadataOverlay<'_> {
        PreparedMptMetadataOverlay { prepared: self }
    }

    /// Visits sorted non-node StateService metadata operations only.
    pub fn visit_metadata_overlay<S>(&mut self, sink: &mut S)
    where
        S: RawOverlaySink + ?Sized,
    {
        visit_sorted_overlay(&self.metadata_overlay, sink);
        self.metadata_visited = true;
    }

    /// Number of exact node puts and tombstones in this commit.
    #[must_use]
    pub fn materialized_node_operation_count(&self) -> usize {
        self.node_overlay.len()
    }

    /// Unresolved full-state node journal in raw-key order.
    ///
    /// Each entry becomes one exact node put only after
    /// [`UnresolvedDeferredNode::resolve_bytes`] validates and combines it
    /// with the authoritative prefix value.
    #[must_use]
    pub fn unresolved_node_journal(&self) -> &[UnresolvedDeferredNode] {
        &self.deferred_journal
    }

    /// Resolves an exported deferred journal against the exact authoritative
    /// node generation used during preparation and moves the resulting bytes
    /// into the ordinary node overlay.
    ///
    /// Reads are issued in bounded sorted batches. All values are validated and
    /// resolved before this commit is mutated, so a backend error, malformed
    /// node, duplicate key, or result-count mismatch leaves the prepared commit
    /// unchanged and safe to discard.
    pub fn materialize_deferred_node_overlay(&mut self) -> StorageResult<()> {
        if self.journal_materialized_from_snapshot {
            return Err(StorageError::invalid_operation(
                "deferred journal already materialized from its node snapshot",
            ));
        }
        if self.journal_committed_at_cursor {
            return Err(StorageError::invalid_operation(
                "deferred journal already resolved at a commit cursor",
            ));
        }
        MptStore::<MemoryStore>::record_count_samples(
            StateRootApplyCountKind::DeferredJournalEntries,
            self.deferred_journal.len() as u64,
            self.samples as u64,
        );
        if self.deferred_journal.is_empty() {
            return Ok(());
        }
        if self.nodes_visited {
            return Err(StorageError::invalid_operation(
                "cannot materialize a deferred journal after visiting the node overlay",
            ));
        }
        if self
            .deferred_journal
            .windows(2)
            .any(|entries| entries[0].key >= entries[1].key)
        {
            return Err(StorageError::invalid_operation(
                "deferred journal keys must be strictly ordered and unique",
            ));
        }
        if self
            .deferred_journal
            .iter()
            .any(|entry| !is_mpt_node_key(&entry.key) || self.node_overlay.contains_key(&entry.key))
        {
            return Err(StorageError::invalid_operation(
                "deferred journal keys must be exact MPT node keys disjoint from the materialized overlay",
            ));
        }

        let snapshot = self.deferred_node_snapshot.as_ref().ok_or_else(|| {
            StorageError::invalid_operation(
                "deferred journal has no pinned authoritative node snapshot",
            )
        })?;
        let mut resolved_values = Vec::with_capacity(self.deferred_journal.len());
        let mut keys = Vec::new();
        let mut lookup_us = 0u64;
        let mut resolve_us = 0u64;
        let mut backing_hits = 0u64;
        let mut backing_misses = 0u64;
        let mut chunk_start = 0usize;
        while chunk_start < self.deferred_journal.len() {
            let mut chunk_end = chunk_start;
            let mut estimated_bytes = 0usize;
            while chunk_end < self.deferred_journal.len()
                && chunk_end - chunk_start < DEFERRED_NODE_LOOKUP_MAX_KEYS
            {
                let entry_bytes = self.deferred_journal[chunk_end]
                    .payload_without_reference
                    .len()
                    .saturating_add(SERIALIZED_NODE_FIXED_BYTES);
                if chunk_end > chunk_start
                    && estimated_bytes.saturating_add(entry_bytes)
                        > DEFERRED_NODE_LOOKUP_MAX_ESTIMATED_BYTES
                {
                    break;
                }
                estimated_bytes = estimated_bytes.saturating_add(entry_bytes);
                chunk_end += 1;
            }
            let journal_chunk = &self.deferred_journal[chunk_start..chunk_end];
            keys.clear();
            keys.reserve(journal_chunk.len());
            keys.extend(journal_chunk.iter().map(|entry| entry.key.as_slice()));
            let lookup_start = Instant::now();
            // Deferred full-state finalization performs only a few sorted
            // batches. Probe process resource counters immediately around the
            // provider call; ordinary point reads never pay this cost.
            let resources_before = process_resource_snapshot();
            let lookup_result = snapshot.try_get_node_bytes_sorted(&keys);
            let resources_after = process_resource_snapshot();
            record_deferred_resource_delta(resources_before, resources_after);
            let stored = match lookup_result {
                Ok(stored) => stored,
                Err(error) => {
                    let failed_lookup_us = lookup_us.saturating_add(elapsed_us(lookup_start));
                    MptStore::<MemoryStore>::record_stage_samples(
                        StateRootApplyStage::DeferredFinalizationLookup,
                        failed_lookup_us,
                        self.samples as u64,
                    );
                    MptStore::<MemoryStore>::record_count_samples(
                        StateRootApplyCountKind::FinalizationLookupErrors,
                        journal_chunk.len() as u64,
                        self.samples as u64,
                    );
                    return Err(StorageError::backend(format!(
                        "authoritative deferred-journal batch read failed: {error}"
                    )));
                }
            };
            lookup_us = lookup_us.saturating_add(elapsed_us(lookup_start));
            if stored.len() != journal_chunk.len() {
                MptStore::<MemoryStore>::record_stage_samples(
                    StateRootApplyStage::DeferredFinalizationLookup,
                    lookup_us,
                    self.samples as u64,
                );
                MptStore::<MemoryStore>::record_count_samples(
                    StateRootApplyCountKind::FinalizationLookupErrors,
                    journal_chunk.len() as u64,
                    self.samples as u64,
                );
                return Err(StorageError::backend(format!(
                    "authoritative deferred-journal batch returned {} results for {} keys",
                    stored.len(),
                    journal_chunk.len()
                )));
            }

            let resolve_start = Instant::now();
            for (entry, stored) in journal_chunk.iter().zip(stored) {
                if stored.is_some() {
                    backing_hits = backing_hits.saturating_add(1);
                } else {
                    backing_misses = backing_misses.saturating_add(1);
                }
                let value = match entry.resolve_bytes(stored) {
                    Ok(value) => value,
                    Err(error) => {
                        resolve_us = resolve_us.saturating_add(elapsed_us(resolve_start));
                        MptStore::<MemoryStore>::record_stage_samples(
                            StateRootApplyStage::DeferredFinalizationResolve,
                            resolve_us,
                            self.samples as u64,
                        );
                        MptStore::<MemoryStore>::record_count_samples(
                            StateRootApplyCountKind::FinalizationBackingHits,
                            backing_hits,
                            self.samples as u64,
                        );
                        MptStore::<MemoryStore>::record_count_samples(
                            StateRootApplyCountKind::FinalizationBackingMisses,
                            backing_misses,
                            self.samples as u64,
                        );
                        return Err(StorageError::backend(format!(
                            "authoritative deferred-journal resolution failed: {error}"
                        )));
                    }
                };
                resolved_values.push(value);
            }
            resolve_us = resolve_us.saturating_add(elapsed_us(resolve_start));
            chunk_start = chunk_end;
        }

        MptStore::<MemoryStore>::record_stage_samples(
            StateRootApplyStage::DeferredFinalizationLookup,
            lookup_us,
            self.samples as u64,
        );
        MptStore::<MemoryStore>::record_stage_samples(
            StateRootApplyStage::DeferredFinalizationResolve,
            resolve_us,
            self.samples as u64,
        );
        MptStore::<MemoryStore>::record_count_samples(
            StateRootApplyCountKind::FinalizationBackingHits,
            backing_hits,
            self.samples as u64,
        );
        MptStore::<MemoryStore>::record_count_samples(
            StateRootApplyCountKind::FinalizationBackingMisses,
            backing_misses,
            self.samples as u64,
        );
        let journal = std::mem::take(&mut self.deferred_journal);
        debug_assert_eq!(journal.len(), resolved_values.len());
        for (entry, value) in journal.into_iter().zip(resolved_values) {
            let key = entry.key;
            self.counts.record(&key, Some(&value));
            let previous = self.node_overlay.insert(key, Some(value));
            debug_assert!(
                previous.is_none(),
                "deferred node key was prevalidated disjoint"
            );
        }
        self.deferred_node_snapshot = None;
        self.journal_materialized_from_snapshot = true;
        Ok(())
    }
}

impl RawOverlaySource for PreparedMptMetadataOverlay<'_> {
    fn visit_raw_overlay<S>(&mut self, sink: &mut S)
    where
        S: RawOverlaySink + ?Sized,
    {
        visit_sorted_overlay(&self.prepared.metadata_overlay, sink);
        self.prepared.metadata_visited = true;
    }
}

fn partition_mpt_overlay(overlay: MptOverlay) -> (MptOverlay, MptOverlay) {
    // Node rows dominate high-height windows. Retain their existing table and
    // capacity, extracting only the small metadata subset so partitioning does
    // not rehash hundreds of thousands of content-addressed node keys.
    let mut node_overlay = overlay;
    let metadata_overlay = node_overlay
        .extract_if(|key, _| !is_mpt_node_key(key))
        .collect();
    (node_overlay, metadata_overlay)
}

fn visit_sorted_overlay<S>(overlay: &MptOverlay, sink: &mut S)
where
    S: RawOverlaySink + ?Sized,
{
    let mut entries = overlay.iter().collect::<Vec<_>>();
    entries.sort_unstable_by(|left, right| left.0.cmp(right.0));
    for (key, value) in entries {
        sink.visit(key, value.as_deref());
    }
}

fn join_mpt_overlay(mut node: MptOverlay, metadata: MptOverlay) -> MptOverlay {
    node.extend(metadata);
    node
}

impl std::fmt::Debug for PreparedMptCommit {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PreparedMptCommit")
            .field("block_index", &self.block_index)
            .field("root_hash", &self.root_hash)
            .field("node_entries", &self.node_overlay.len())
            .field("metadata_entries", &self.metadata_overlay.len())
            .field("deferred_journal", &self.deferred_journal.len())
            .field("samples", &self.samples)
            .finish()
    }
}

impl RawOverlaySource for PreparedMptCommit {
    fn visit_raw_overlay<S>(&mut self, sink: &mut S)
    where
        S: RawOverlaySink + ?Sized,
    {
        let sort_start = Instant::now();
        let mut entries = self
            .metadata_overlay
            .iter()
            .chain(self.node_overlay.iter())
            .collect::<Vec<_>>();
        entries.sort_unstable_by(|left, right| left.0.cmp(right.0));
        MptStore::<MemoryStore>::record_stage_samples(
            StateRootApplyStage::BackingSort,
            elapsed_us(sort_start),
            self.samples as u64,
        );
        self.nodes_visited = true;
        self.metadata_visited = true;
        for (key, value) in entries {
            sink.visit(key, value.as_deref());
        }
    }

    /// Resolves the deferred full-state journal at the commit's write cursor,
    /// replacing the classic resolve-then-encode sweep against a frozen
    /// backing snapshot.
    ///
    /// Correctness invariants:
    /// - The StateService write gate serializes every writer of this table,
    ///   and the coordinated commit runs inside it, so the RW transaction
    ///   observes exactly the base the frozen snapshot would have seen when
    ///   the batch was prepared. The primary overlay of the coordinated
    ///   commit targets a different table (validated by the coordinator), and
    ///   the materialized overlay keys are disjoint from the journaled node
    ///   hashes (a hash is tracked either in the cache entries or in the
    ///   deferred journal, never both), so every probe reads the same bytes
    ///   the classic two-sweep path would have resolved.
    /// - `reference = (persisted reference if the key exists else 0) +
    ///   journaled put count`; the stored payload and node type win when the
    ///   key exists, exactly like the journal replay in `MptCache`.
    /// - Full-state journals never delete (`Trie::previous_hash` yields
    ///   `None` in full-state mode), so every journal entry is a put.
    /// - Fail-closed: any cursor or resolution error aborts the commit before
    ///   the transaction publishes.
    fn commit_raw_overlay_at_cursor(
        &mut self,
        cursor: &mut dyn RawOverlayCursor,
    ) -> StorageResult<()> {
        if !self.deferred_journal.is_empty() && self.deferred_node_snapshot.is_some() {
            return Err(StorageError::invalid_operation(
                "split-authority deferred journals must be materialized from their pinned node snapshot",
            ));
        }
        // Resolution is read-modify-write, not idempotent: a second pass would
        // read the first pass's rows and double-accumulate reference counts.
        if self.journal_committed_at_cursor {
            return Err(StorageError::invalid_operation(
                "deferred journal already resolved at a commit cursor".to_string(),
            ));
        }
        if self.journal_materialized_from_snapshot {
            return Err(StorageError::invalid_operation(
                "deferred journal already materialized from its node snapshot".to_string(),
            ));
        }
        MptStore::<MemoryStore>::record_count_samples(
            StateRootApplyCountKind::DeferredJournalEntries,
            self.deferred_journal.len() as u64,
            self.samples as u64,
        );
        for entry in &self.deferred_journal {
            let absent_value =
                entry
                    .resolve_bytes(None)
                    .map_err(|error| StorageError::Backend {
                        message: format!(
                            "state-service deferred journal resolution failed: {error}"
                        ),
                    })?;
            let value = match cursor.insert_stored_if_absent(&entry.key, &absent_value)? {
                None => absent_value,
                Some(stored) => {
                    let value = entry.resolve_bytes(Some(stored)).map_err(|error| {
                        StorageError::Backend {
                            message: format!(
                                "state-service deferred journal resolution failed: {error}"
                            ),
                        }
                    })?;
                    cursor.write_stored(&entry.key, &value)?;
                    value
                }
            };
            self.counts.record(&entry.key, Some(&value));
        }
        self.journal_committed_at_cursor = true;
        Ok(())
    }
}

impl RawOverlaySource for SortedOverlaySource<'_> {
    fn visit_raw_overlay<S>(&mut self, sink: &mut S)
    where
        S: RawOverlaySink + ?Sized,
    {
        for (key, value) in self.entries {
            self.counts.record(key, value.as_deref());
            sink.visit(key, value.as_deref());
        }
    }
}

impl<S> MptReadSnapshot<S>
where
    S: Store,
{
    /// Returns whether historical trie versions are retained.
    pub fn full_state(&self) -> bool {
        self.full_state
    }

    /// Opens a read-only trie rooted at `root` (`None` for the empty
    /// trie) over this frozen view. Mutating the returned [`Trie`] is
    /// rejected at commit time (the snapshot's store surface is
    /// read-only).
    pub fn open_trie(self: &Arc<Self>, root: Option<UInt256>) -> Trie<Self> {
        Trie::new(Arc::clone(self), root, self.full_state)
    }

    /// Fallible state-root lookup as of this snapshot.
    ///
    /// Distinguishes a missing record (`Ok(None)`) from a durable backend
    /// failure (`Err`), so callers can abort instead of treating I/O errors as
    /// absent state.
    pub fn try_get_state_root(&self, index: u32) -> MptResult<Option<StateRoot>> {
        MptStore::<S>::read_state_root(&self.map, self.backing_snapshot.as_deref(), index)
    }

    /// Returns the state-root record persisted for `index`, if any,
    /// as of this snapshot.
    ///
    /// Prefer [`Self::try_get_state_root`] on consensus-critical paths. This
    /// compatibility wrapper logs backend failures and returns `None`.
    pub fn get_state_root(&self, index: u32) -> Option<StateRoot> {
        match self.try_get_state_root(index) {
            Ok(root) => root,
            Err(error) => {
                tracing::error!(
                    target: "neo.state_service",
                    index,
                    error = %error,
                    "MPT state-root read failed; treating as absent for legacy Option API"
                );
                None
            }
        }
    }

    /// Fallible current local root index as of this snapshot.
    pub fn try_current_local_root_index(&self) -> MptResult<Option<u32>> {
        MptStore::<S>::read_current_local_root_index(&self.map, self.backing_snapshot.as_deref())
    }

    /// Returns the local root index current as of this snapshot (C#
    /// `StateSnapshot.CurrentLocalRootIndex`).
    pub fn current_local_root_index(&self) -> Option<u32> {
        match self.try_current_local_root_index() {
            Ok(index) => index,
            Err(error) => {
                tracing::error!(
                    target: "neo.state_service",
                    error = %error,
                    "MPT current local root index read failed; treating as absent for legacy Option API"
                );
                None
            }
        }
    }

    /// Returns the local root hash current as of this snapshot (C#
    /// `StateSnapshot.CurrentLocalRootHash`).
    pub fn current_local_root_hash(&self) -> Option<UInt256> {
        let index = self.current_local_root_index()?;
        Some(*self.get_state_root(index)?.root_hash())
    }
}

impl<S> std::fmt::Debug for MptReadSnapshot<S>
where
    S: Store,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MptReadSnapshot")
            .field("entries", &self.map.len())
            .field("full_state", &self.full_state)
            .finish()
    }
}

impl<S> MptStoreSnapshot for MptReadSnapshot<S>
where
    S: Store,
{
    fn try_get(&self, key: &[u8]) -> MptResult<Option<Vec<u8>>> {
        if is_mpt_node_key(key)
            && let Some(snapshot) = self.node_snapshot.as_ref()
        {
            return snapshot.try_get_node_bytes(key).map_err(|error| {
                MptError::storage(format!(
                    "MPT authoritative node snapshot get failed: {error}"
                ))
            });
        }
        match self.map.get(key) {
            Some(value) => Ok(value.clone()),
            None => self.read_ordinary_backing(key),
        }
    }

    fn put(&self, _key: Vec<u8>, _value: Vec<u8>) -> MptResult<()> {
        Err(MptError::invalid(
            "cannot write through a read-only MPT store snapshot",
        ))
    }

    fn delete(&self, _key: Vec<u8>) -> MptResult<()> {
        Err(MptError::invalid(
            "cannot write through a read-only MPT store snapshot",
        ))
    }
}

impl<S> MptReadSnapshot<S>
where
    S: Store,
{
    fn read_ordinary_backing(&self, key: &[u8]) -> MptResult<Option<Vec<u8>>> {
        match self.backing_snapshot.as_ref() {
            None => Ok(None),
            Some(snapshot) => snapshot.try_get_bytes_result(key).map_err(|error| {
                MptError::storage(format!("MPT read-snapshot backing get failed: {error}"))
            }),
        }
    }
}

/// Lazily opens the write trie only when a block has at least one effective
/// StateService MPT mutation.
pub(crate) struct LazyMptChangeSink<'a, S: Store = MemoryStore> {
    store: &'a MptStore<S>,
    root_before: Option<UInt256>,
    overlay_capacity: usize,
    batch: Option<Arc<MptWriteBatch<S>>>,
    trie: Option<Trie<MptWriteBatch<S>>>,
}

impl<'a, S> LazyMptChangeSink<'a, S>
where
    S: Store,
{
    fn new(store: &'a MptStore<S>, root_before: Option<UInt256>, overlay_capacity: usize) -> Self {
        Self {
            store,
            root_before,
            overlay_capacity,
            batch: None,
            trie: None,
        }
    }

    fn ensure_trie(&mut self) -> MptResult<&mut Trie<MptWriteBatch<S>>> {
        if self.trie.is_none() {
            let batch = Arc::new(MptWriteBatch::<S>::new(
                Arc::clone(&self.store.kv.read()),
                self.store.backing_snapshot(),
                self.store.node_snapshot(),
                self.overlay_capacity,
            ));
            self.trie = Some(Trie::new(
                Arc::clone(&batch),
                self.root_before,
                self.store.full_state,
            ));
            self.batch = Some(batch);
        }
        self.trie.as_mut().ok_or_else(|| {
            MptError::invalid("state-service lazy MPT sink failed to initialize trie")
        })
    }

    /// Inserts or updates a key, opening the write trie on first use.
    pub(crate) fn put_with_scratch(
        &mut self,
        key: &[u8],
        value: &[u8],
        path_scratch: &mut Vec<u8>,
    ) -> MptResult<()> {
        self.ensure_trie()?
            .put_with_scratch(key, value, path_scratch)
    }

    /// Deletes a key, opening the write trie on first use.
    pub(crate) fn delete_with_scratch(
        &mut self,
        key: &[u8],
        path_scratch: &mut Vec<u8>,
    ) -> MptResult<bool> {
        self.ensure_trie()?.delete_with_scratch(key, path_scratch)
    }
}

impl<S> MptStore<S>
where
    S: Store,
{
    fn record_mutation_stats(stats: MptMutationStats, overlay_working_set_entries: usize) {
        StateRootApplyMetrics::record_count(
            StateRootApplyCountKind::PutNodeCachedCalls,
            stats.put_node_cached_calls,
        );
        StateRootApplyMetrics::record_count(
            StateRootApplyCountKind::SerializedPayloadBytes,
            stats.serialized_payload_bytes,
        );
        StateRootApplyMetrics::record_count(
            StateRootApplyCountKind::HashComputations,
            stats.hash_computations,
        );
        StateRootApplyMetrics::record_count(
            StateRootApplyCountKind::MaxRecursionDepth,
            stats.max_recursion_depth,
        );
        StateRootApplyMetrics::record_count(
            StateRootApplyCountKind::RepeatedAncestorFinalizations,
            stats.repeated_ancestor_finalizations,
        );
        StateRootApplyMetrics::record_count(
            StateRootApplyCountKind::TrieResolveCacheHits,
            stats.trie_resolve_cache_hits,
        );
        StateRootApplyMetrics::record_count(
            StateRootApplyCountKind::TrieResolveStoreHits,
            stats.trie_resolve_store_hits,
        );
        StateRootApplyMetrics::record_count(
            StateRootApplyCountKind::TrieResolveStoreMisses,
            stats.trie_resolve_store_misses,
        );
        StateRootApplyMetrics::record_count(
            StateRootApplyCountKind::DeferredFinalizationReadBytes,
            stats.deferred_finalization_read_bytes,
        );
        StateRootApplyMetrics::record_count(
            StateRootApplyCountKind::DeferredFinalizationMinorFaults,
            stats.deferred_finalization_minor_faults,
        );
        StateRootApplyMetrics::record_count(
            StateRootApplyCountKind::DeferredFinalizationMajorFaults,
            stats.deferred_finalization_major_faults,
        );
        StateRootApplyMetrics::record_count(
            StateRootApplyCountKind::OverlayWorkingSetEntries,
            overlay_working_set_entries as u64,
        );
        StateRootApplyMetrics::record_count(
            StateRootApplyCountKind::FinalizationCacheHits,
            stats.finalization_cache_hits,
        );
        StateRootApplyMetrics::record_count(
            StateRootApplyCountKind::FinalizationMemoryHits,
            stats.finalization_memory_hits,
        );
        StateRootApplyMetrics::record_count(
            StateRootApplyCountKind::FinalizationMemoryMisses,
            stats.finalization_memory_misses,
        );
        StateRootApplyMetrics::record_count(
            StateRootApplyCountKind::FinalizationBackingHits,
            stats.finalization_backing_hits,
        );
        StateRootApplyMetrics::record_count(
            StateRootApplyCountKind::FinalizationBackingMisses,
            stats.finalization_backing_misses,
        );
        StateRootApplyMetrics::record_count(
            StateRootApplyCountKind::FinalizationLookupErrors,
            stats.finalization_lookup_errors,
        );
        for (stage, elapsed_us) in [
            (
                StateRootApplyStage::TrieResolveStore,
                stats.trie_resolve_store_ns / 1_000,
            ),
            (
                StateRootApplyStage::DeferredFinalizationPrepare,
                stats.deferred_finalization_prepare_us,
            ),
            (
                StateRootApplyStage::DeferredFinalizationLookup,
                stats.deferred_finalization_lookup_us,
            ),
            (
                StateRootApplyStage::DeferredFinalizationParse,
                stats.deferred_finalization_parse_us,
            ),
            (
                StateRootApplyStage::DeferredFinalizationReplay,
                stats.deferred_finalization_replay_us,
            ),
            (
                StateRootApplyStage::DeferredFinalizationEncode,
                stats.deferred_finalization_encode_us,
            ),
        ] {
            if elapsed_us > 0 {
                StateRootApplyMetrics::record_stage(stage, elapsed_us);
            }
        }
    }

    fn record_empty_mutation_samples(samples: u64) {
        for kind in [
            StateRootApplyCountKind::PutNodeCachedCalls,
            StateRootApplyCountKind::SerializedPayloadBytes,
            StateRootApplyCountKind::HashComputations,
            StateRootApplyCountKind::MaxRecursionDepth,
            StateRootApplyCountKind::RepeatedAncestorFinalizations,
            StateRootApplyCountKind::TrieResolveCacheHits,
            StateRootApplyCountKind::TrieResolveStoreHits,
            StateRootApplyCountKind::TrieResolveStoreMisses,
            StateRootApplyCountKind::DeferredFinalizationReadBytes,
            StateRootApplyCountKind::DeferredFinalizationMinorFaults,
            StateRootApplyCountKind::DeferredFinalizationMajorFaults,
            StateRootApplyCountKind::OverlayWorkingSetEntries,
            StateRootApplyCountKind::FinalizationCacheHits,
            StateRootApplyCountKind::FinalizationMemoryHits,
            StateRootApplyCountKind::FinalizationMemoryMisses,
            StateRootApplyCountKind::FinalizationBackingHits,
            StateRootApplyCountKind::FinalizationBackingMisses,
            StateRootApplyCountKind::FinalizationLookupErrors,
        ] {
            Self::record_count_samples(kind, 0, samples);
        }
    }
    /// Flushes the optional durable backend after all MPT writes before this
    /// call have completed.
    pub(crate) fn flush_backing(&self) -> MptResult<()> {
        let _write_guard = self.write_gate.lock();
        if let Some(backing) = &self.backing {
            backing.flush().map_err(|error| {
                MptError::storage(format!("state-service backing flush failed: {error}"))
            })?;
        }
        Ok(())
    }

    /// Opens a store over an existing durable byte namespace.
    pub fn from_store(backing: Arc<S>, full_state: bool) -> MptResult<Self> {
        Self::from_store_with_options(backing, full_state, false)
    }

    /// Opens a store with an explicit full-state finalization policy.
    pub fn from_store_with_options(
        backing: Arc<S>,
        full_state: bool,
        defer_full_state_finalization: bool,
    ) -> MptResult<Self> {
        Self::from_store_parts(backing, full_state, defer_full_state_finalization, None)
    }

    /// Opens StateService metadata over `backing` while routing every MPT node
    /// read through an independently pinned authoritative generation.
    ///
    /// This compatibility wrapper retains eager finalization. Call
    /// [`Self::from_store_with_node_snapshot_options`] to enable deferred
    /// finalization against the pinned authoritative node generation.
    pub fn from_store_with_node_snapshots(
        backing: Arc<S>,
        full_state: bool,
        node_snapshots: Arc<dyn MptNodeSnapshotFactory>,
    ) -> MptResult<Self> {
        Self::from_store_with_node_snapshot_options(backing, full_state, false, node_snapshots)
    }

    /// Opens split StateService storage with an explicit full-state
    /// finalization policy.
    pub fn from_store_with_node_snapshot_options(
        backing: Arc<S>,
        full_state: bool,
        defer_full_state_finalization: bool,
        node_snapshots: Arc<dyn MptNodeSnapshotFactory>,
    ) -> MptResult<Self> {
        Self::from_store_parts(
            backing,
            full_state,
            defer_full_state_finalization,
            Some(node_snapshots),
        )
    }

    fn from_store_parts(
        backing: Arc<S>,
        full_state: bool,
        defer_full_state_finalization: bool,
        node_snapshots: Option<Arc<dyn MptNodeSnapshotFactory>>,
    ) -> MptResult<Self> {
        let latest_local_root = Self::load_latest_local_root_from_backing(backing.as_ref());
        Ok(Self {
            kv: RwLock::new(Arc::new(HashMap::new())),
            write_gate: Mutex::new(()),
            full_state,
            defer_full_state_finalization: full_state && defer_full_state_finalization,
            latest_local_root: RwLock::new(latest_local_root),
            backing: Some(backing),
            node_snapshots,
        })
    }

    /// Opens a store over a concrete in-memory backend.
    ///
    /// This is primarily for tests and ephemeral nodes that need persistence
    /// across `MptStore` instances without erasing the backend behind
    /// a `Store` trait object.
    /// Returns whether historical trie versions are retained.
    pub fn full_state(&self) -> bool {
        self.full_state
    }

    /// Returns whether full-state batch finalization lookups are deferred.
    pub fn defers_full_state_finalization(&self) -> bool {
        self.defer_full_state_finalization
    }

    /// Returns whether this MPT can participate in an external durable commit.
    pub fn has_backing_store(&self) -> bool {
        self.backing.is_some()
    }

    fn backing_snapshot(&self) -> Option<Arc<S::Snapshot>> {
        self.backing.as_ref().map(|backing| backing.snapshot())
    }

    fn node_snapshot(&self) -> Option<Arc<dyn MptNodeReadSnapshot>> {
        self.node_snapshots
            .as_ref()
            .map(|factory| factory.snapshot())
    }

    fn paired_read_snapshots(
        &self,
    ) -> (
        Option<Arc<S::Snapshot>>,
        Option<Arc<dyn MptNodeReadSnapshot>>,
    ) {
        let Some(factory) = self.node_snapshots.as_ref() else {
            return (self.backing_snapshot(), None);
        };
        loop {
            let generation = factory.pinned_generation();
            let backing = self.backing_snapshot();
            if factory.is_generation_current(generation.sequence()) {
                return (backing, Some(generation.snapshot()));
            }
            std::thread::yield_now();
        }
    }

    fn load_latest_local_root_from_backing(backing: &S) -> Option<(u32, UInt256)> {
        let snapshot = backing.snapshot();
        let index =
            match Self::read_current_local_root_index(&HashMap::new(), Some(snapshot.as_ref())) {
                Ok(index) => index?,
                Err(error) => {
                    tracing::error!(
                        target: "neo.state_service",
                        error = %error,
                        "failed to load current local root index from durable backing"
                    );
                    return None;
                }
            };
        match Self::read_state_root(&HashMap::new(), Some(snapshot.as_ref()), index) {
            Ok(Some(root)) => Some((index, *root.root_hash())),
            Ok(None) => None,
            Err(error) => {
                tracing::error!(
                    target: "neo.state_service",
                    index,
                    error = %error,
                    "failed to load latest local root from durable backing"
                );
                None
            }
        }
    }

    /// Captures an immutable, point-in-time view of the store (the C#
    /// `GetStoreSnapshot()` analogue). All read paths that walk the
    /// trie must go through a snapshot so a concurrent
    /// [`MptStore::apply_block_changes`] (which prunes superseded
    /// nodes without `full_state`) cannot delete nodes out from under
    /// the walk.
    pub fn snapshot(&self) -> Arc<MptReadSnapshot<S>> {
        let (backing_snapshot, node_snapshot) = self.paired_read_snapshots();
        Arc::new(MptReadSnapshot {
            map: Arc::clone(&self.kv.read()),
            backing_snapshot,
            node_snapshot,
            full_state: self.full_state,
        })
    }

    /// Opens a read view of the trie rooted at `root` (`None` for the
    /// empty trie) over a fresh [`MptReadSnapshot`]. The returned
    /// [`Trie`] resolves nodes from the frozen view; read operations
    /// never write back.
    ///
    /// Callers that need the current root *and* the trie to agree
    /// (e.g. RPC handlers) should take one [`MptStore::snapshot`] and
    /// read both from it instead.
    pub fn open_trie(&self, root: Option<UInt256>) -> Trie<MptReadSnapshot<S>> {
        self.snapshot().open_trie(root)
    }

    /// Applies one block's storage change set on top of `root_before`
    /// and returns the new state root.
    ///
    /// Mirrors the C# `StateStore.UpdateLocalStateRootSnapshot(height,
    /// changeSet)` + `UpdateLocalStateRoot(height)` pair:
    ///
    /// 1. every change is applied to a trie opened at `root_before`
    ///    (`Put` for Added/Changed, `Delete` for Deleted);
    /// 2. the new root hash is computed (`Trie.Root.Hash`);
    /// 3. the trie nodes are committed to the store;
    /// 4. the block's `StateRoot` record is written under
    ///    `Keys::state_root(block_index)` and the current local root
    ///    index is advanced (`Keys::CURRENT_LOCAL_ROOT_INDEX`).
    ///
    /// [`crate::StateStore::apply_snapshot_changes`] is the normal node path:
    /// it performs the C# `Blockchain_Committing_Handler` filtering before
    /// calling this lower-level method. Tests and replayers may call this
    /// directly when they already have a projected change set.
    ///
    /// `root_before` must be the root produced by the previous block
    /// (or `None` before the first block); it is taken explicitly so
    /// replays and tests can branch from arbitrary roots.
    pub fn apply_block_changes(
        &self,
        block_index: u32,
        root_before: Option<UInt256>,
        changes: &[MptChange],
    ) -> MptResult<UInt256> {
        self.apply_block_changes_with_len(block_index, root_before, changes.len(), |trie| {
            let mut effective_change_count = 0usize;
            let mut path_scratch = Vec::new();
            for change in changes {
                effective_change_count += 1;
                match change {
                    MptChange::Put { key, value } => {
                        trie.put_with_scratch(key, value, &mut path_scratch)?
                    }
                    MptChange::Delete { key } => {
                        // C# ignores the `Trie.Delete` return value: deleting
                        // a key that is already absent is a no-op.
                        let _ = trie.delete_with_scratch(key, &mut path_scratch)?;
                    }
                }
            }
            Ok(effective_change_count)
        })
    }

    /// Applies one block while delegating durable publication to `commit`.
    ///
    /// The trie mutation, root calculation, and overlay preparation run under
    /// the StateService writer gate. `commit` receives the concrete backing view
    /// and an ordered raw overlay. The local root and read generation advance
    /// only after `commit` returns success, allowing a caller to include this
    /// overlay in the canonical Ledger transaction without exposing MPT details.
    ///
    /// The callback is a trusted composition boundary. It must consume the
    /// complete overlay and return `Ok(())` only after the canonical and MPT
    /// mutations have committed atomically. The post-callback visit check
    /// catches accidental omission; it cannot undo an external commit.
    pub fn apply_block_changes_coordinated<F>(
        &self,
        block_index: u32,
        root_before: Option<UInt256>,
        changes: &[MptChange],
        commit: F,
    ) -> MptResult<UInt256>
    where
        F: FnOnce(&S, &mut PreparedMptCommit) -> StorageResult<()>,
    {
        self.apply_block_changes_with_len_coordinated(
            block_index,
            root_before,
            changes.len(),
            |trie| {
                let mut effective_change_count = 0usize;
                let mut path_scratch = Vec::new();
                for change in changes {
                    effective_change_count += 1;
                    match change {
                        MptChange::Put { key, value } => {
                            trie.put_with_scratch(key, value, &mut path_scratch)?
                        }
                        MptChange::Delete { key } => {
                            let _ = trie.delete_with_scratch(key, &mut path_scratch)?;
                        }
                    }
                }
                Ok(effective_change_count)
            },
            commit,
        )
    }

    fn apply_block_changes_with_len<F>(
        &self,
        block_index: u32,
        root_before: Option<UInt256>,
        change_count: usize,
        mutate: F,
    ) -> MptResult<UInt256>
    where
        F: FnOnce(&mut Trie<MptWriteBatch<S>>) -> MptResult<usize>,
    {
        let _writer = self.write_gate.lock();
        let prepared =
            self.prepare_block_changes_with_len(block_index, root_before, change_count, mutate)?;
        self.publish_prepared(prepared)
    }

    fn apply_block_changes_with_len_coordinated<M, C>(
        &self,
        block_index: u32,
        root_before: Option<UInt256>,
        change_count: usize,
        mutate: M,
        commit: C,
    ) -> MptResult<UInt256>
    where
        M: FnOnce(&mut Trie<MptWriteBatch<S>>) -> MptResult<usize>,
        C: FnOnce(&S, &mut PreparedMptCommit) -> StorageResult<()>,
    {
        let _writer = self.write_gate.lock();
        let prepared =
            self.prepare_block_changes_with_len(block_index, root_before, change_count, mutate)?;
        self.publish_prepared_coordinated(prepared, commit)
    }

    fn prepare_block_changes_with_len<F>(
        &self,
        block_index: u32,
        root_before: Option<UInt256>,
        change_count: usize,
        mutate: F,
    ) -> MptResult<PreparedMptCommit>
    where
        F: FnOnce(&mut Trie<MptWriteBatch<S>>) -> MptResult<usize>,
    {
        if change_count == 0
            && let Some(root_before) = root_before
        {
            StateRootApplyMetrics::record_count(StateRootApplyCountKind::Changes, 0);
            let overlay = Self::local_root_overlay(block_index, root_before);
            Self::record_mutation_stats(MptMutationStats::default(), 0);
            return Ok(PreparedMptCommit::new(block_index, root_before, overlay, 1));
        }

        // Stage every mutation against the current generation. The
        // writer gate guarantees the base cannot change underneath us.
        let batch = Arc::new(MptWriteBatch::<S>::new(
            Arc::clone(&self.kv.read()),
            self.backing_snapshot(),
            self.node_snapshot(),
            change_count.saturating_mul(2) + 2,
        ));

        let mut trie = Trie::new(Arc::clone(&batch), root_before, self.full_state);
        let stage_start = Instant::now();
        let effective_change_count = mutate(&mut trie)?;
        StateRootApplyMetrics::record_count(
            StateRootApplyCountKind::Changes,
            effective_change_count as u64,
        );
        StateRootApplyMetrics::record_stage(
            StateRootApplyStage::MutateChanges,
            elapsed_us(stage_start),
        );

        // C# reads `Trie.Root.Hash` (well-defined even for the empty
        // root, which hashes its single sentinel byte).
        let stage_start = Instant::now();
        let new_root = trie.try_root_hash()?;
        StateRootApplyMetrics::record_stage(StateRootApplyStage::RootHash, elapsed_us(stage_start));
        if effective_change_count == 0 && root_before.is_some() {
            let mutation_stats = trie.take_mutation_stats();
            Self::record_mutation_stats(mutation_stats, batch.overlay.lock().len());
            drop(trie);
            drop(batch);
            let overlay = Self::local_root_overlay(block_index, new_root);
            return Ok(PreparedMptCommit::new(block_index, new_root, overlay, 1));
        }

        let stage_start = Instant::now();
        trie.commit()?;
        StateRootApplyMetrics::record_stage(
            StateRootApplyStage::TrieCommit,
            elapsed_us(stage_start),
        );
        let mutation_stats = trie.take_mutation_stats();
        Self::record_mutation_stats(mutation_stats, batch.overlay.lock().len());
        drop(trie);

        // The local (unwitnessed) state-root record and the current
        // local root index advance in the same published generation
        // (C# `StateSnapshot.AddLocalStateRoot` + snapshot commit), so
        // a reader can never observe the new root record without the
        // trie nodes that back it.
        let stage_start = Instant::now();
        let overlay = {
            let mut overlay = batch.overlay.lock();
            Self::insert_local_root_records(&mut overlay, block_index, new_root);
            Self::ensure_empty_root_record(&mut overlay, new_root)?;
            std::mem::take(&mut *overlay)
        };
        StateRootApplyMetrics::record_stage(
            StateRootApplyStage::OverlayPrepare,
            elapsed_us(stage_start),
        );
        // Release the batch's base Arc before publishing so that, with
        // no reader snapshots outstanding, `make_mut` updates in place
        // instead of cloning the map.
        drop(batch);

        Ok(PreparedMptCommit::new(block_index, new_root, overlay, 1))
    }

    pub(crate) fn apply_block_changes_lazy<F>(
        &self,
        block_index: u32,
        root_before: Option<UInt256>,
        change_capacity_hint: usize,
        mutate: F,
    ) -> MptResult<UInt256>
    where
        F: FnOnce(&mut LazyMptChangeSink<'_, S>) -> MptResult<usize>,
    {
        let _writer = self.write_gate.lock();
        let mut sink = LazyMptChangeSink::new(
            self,
            root_before,
            change_capacity_hint.saturating_mul(2).saturating_add(2),
        );

        let stage_start = Instant::now();
        let effective_change_count = mutate(&mut sink)?;
        StateRootApplyMetrics::record_count(
            StateRootApplyCountKind::Changes,
            effective_change_count as u64,
        );
        StateRootApplyMetrics::record_stage(
            StateRootApplyStage::MutateChanges,
            elapsed_us(stage_start),
        );

        let Some(mut trie) = sink.trie.take() else {
            let stage_start = Instant::now();
            let computed_empty_root = root_before.is_none();
            let new_root = match root_before {
                Some(root_before) => root_before,
                None => Node::new().try_hash()?,
            };
            StateRootApplyMetrics::record_stage(
                StateRootApplyStage::RootHash,
                elapsed_us(stage_start),
            );

            let mut overlay = Self::local_root_overlay(block_index, new_root);
            Self::ensure_empty_root_record(&mut overlay, new_root)?;
            Self::record_mutation_stats(
                MptMutationStats {
                    hash_computations: u64::from(computed_empty_root),
                    ..MptMutationStats::default()
                },
                0,
            );
            return self.publish_overlay(block_index, new_root, overlay);
        };
        let Some(batch) = sink.batch.take() else {
            return Err(MptError::invalid(
                "state-service lazy MPT sink opened a trie without a write batch",
            ));
        };

        let stage_start = Instant::now();
        let new_root = trie.try_root_hash()?;
        StateRootApplyMetrics::record_stage(StateRootApplyStage::RootHash, elapsed_us(stage_start));
        if !(effective_change_count == 0 && root_before.is_some()) {
            let stage_start = Instant::now();
            trie.commit()?;
            StateRootApplyMetrics::record_stage(
                StateRootApplyStage::TrieCommit,
                elapsed_us(stage_start),
            );
        }
        let mutation_stats = trie.take_mutation_stats();
        Self::record_mutation_stats(mutation_stats, batch.overlay.lock().len());
        drop(trie);

        let stage_start = Instant::now();
        let overlay = {
            let mut overlay = batch.overlay.lock();
            Self::insert_local_root_records(&mut overlay, block_index, new_root);
            Self::ensure_empty_root_record(&mut overlay, new_root)?;
            std::mem::take(&mut *overlay)
        };
        StateRootApplyMetrics::record_stage(
            StateRootApplyStage::OverlayPrepare,
            elapsed_us(stage_start),
        );
        drop(batch);

        self.publish_overlay(block_index, new_root, overlay)
    }

    pub(crate) fn apply_block_changes_batch(
        &self,
        root_before: Option<UInt256>,
        blocks: &[MptBlockChanges<'_>],
    ) -> MptResult<Vec<UInt256>> {
        if blocks.is_empty() {
            return Ok(Vec::new());
        }

        let _writer = self.write_gate.lock();
        self.validate_ordered_batch(root_before, blocks)?;
        if let Some(root_before) = root_before
            && blocks.iter().all(|block| block.changes.is_empty())
        {
            let samples = blocks.len() as u64;
            Self::record_count_samples(StateRootApplyCountKind::Changes, 0, samples);
            Self::record_stage_samples(StateRootApplyStage::MutateChanges, 0, samples);
            Self::record_stage_samples(StateRootApplyStage::RootHash, 0, samples);
            Self::record_empty_mutation_samples(samples);

            self.publish_empty_root_batch(blocks, root_before)?;
            return Ok(vec![root_before; blocks.len()]);
        }

        let (roots, prepared) = self.prepare_block_changes_batch(root_before, blocks, false)?;
        self.publish_prepared(prepared)?;
        Ok(roots)
    }

    pub(crate) fn apply_block_changes_batch_coordinated<F>(
        &self,
        root_before: Option<UInt256>,
        blocks: &[MptBlockChanges<'_>],
        commit: F,
    ) -> MptResult<Vec<UInt256>>
    where
        F: FnOnce(&S, &mut PreparedMptCommit) -> StorageResult<()>,
    {
        if blocks.is_empty() {
            return Ok(Vec::new());
        }

        let _writer = self.write_gate.lock();
        self.validate_ordered_batch(root_before, blocks)?;
        // Fused cursor resolution is correct only inside this write gate: the
        // gate serializes every StateService writer, and the coordinated
        // commit runs inside it, so the RW transaction used by the commit
        // observes exactly the base the frozen snapshot would have seen while
        // the batch was prepared. Export stays off unless the backing store
        // can resolve overlay entries at its write cursor; every other path
        // keeps the classic resolve-then-write flow.
        let export_deferred_journal = self.defer_full_state_finalization
            && (self.node_snapshots.is_some()
                || self
                    .backing
                    .as_ref()
                    .is_some_and(|backing| backing.supports_raw_overlay_cursor()));
        let (roots, prepared) =
            self.prepare_block_changes_batch(root_before, blocks, export_deferred_journal)?;
        self.publish_prepared_coordinated(prepared, commit)?;
        Ok(roots)
    }

    fn prepare_block_changes_batch(
        &self,
        root_before: Option<UInt256>,
        blocks: &[MptBlockChanges<'_>],
        export_deferred_journal: bool,
    ) -> MptResult<(Vec<UInt256>, PreparedMptCommit)> {
        let overlay_capacity = blocks
            .iter()
            .map(|block| block.changes.len().saturating_mul(2).saturating_add(2))
            .sum();
        let (backing_snapshot, node_snapshot) = self.paired_read_snapshots();
        let batch = Arc::new(MptWriteBatch::<S>::new(
            Arc::clone(&self.kv.read()),
            backing_snapshot,
            node_snapshot,
            overlay_capacity,
        ));
        let mut trie = if self.defer_full_state_finalization {
            let mut trie = Trie::new_batch_deferred_full_state(
                Arc::clone(&batch),
                root_before,
                self.full_state,
            );
            trie.set_deferred_journal_export(export_deferred_journal);
            trie
        } else {
            Trie::new_batch(Arc::clone(&batch), root_before, self.full_state)
        };
        let mut roots = Vec::with_capacity(blocks.len());
        let mut current_root = root_before;
        let mut path_scratch = Vec::new();
        let final_checkpoint = blocks
            .iter()
            .rposition(|block| !block.changes.is_empty())
            .or_else(|| root_before.is_none().then_some(0));

        for (block_offset, block) in blocks.iter().enumerate() {
            let stage_start = Instant::now();
            let mut effective_change_count = 0usize;
            for change in block.changes {
                effective_change_count += 1;
                match change {
                    MptChange::Put { key, value } => {
                        trie.put_with_scratch(key, value, &mut path_scratch)?
                    }
                    MptChange::Delete { key } => {
                        let _ = trie.delete_with_scratch(key, &mut path_scratch)?;
                    }
                }
            }
            StateRootApplyMetrics::record_count(
                StateRootApplyCountKind::Changes,
                effective_change_count as u64,
            );
            StateRootApplyMetrics::record_stage(
                StateRootApplyStage::MutateChanges,
                elapsed_us(stage_start),
            );

            let stage_start = Instant::now();
            let new_root = trie.try_root_hash()?;
            StateRootApplyMetrics::record_stage(
                StateRootApplyStage::RootHash,
                elapsed_us(stage_start),
            );

            if !(effective_change_count == 0 && current_root.is_some()) {
                let stage_start = Instant::now();
                if Some(block_offset) == final_checkpoint {
                    trie.commit()?;
                } else {
                    trie.checkpoint()?;
                }
                StateRootApplyMetrics::record_stage(
                    StateRootApplyStage::TrieCommit,
                    elapsed_us(stage_start),
                );
            }
            let mutation_stats = trie.take_mutation_stats();
            Self::record_mutation_stats(mutation_stats, batch.overlay.lock().len());

            let stage_start = Instant::now();
            {
                let mut overlay = batch.overlay.lock();
                Self::insert_local_root_records(&mut overlay, block.block_index, new_root);
                Self::ensure_empty_root_record(&mut overlay, new_root)?;
            }
            StateRootApplyMetrics::record_stage(
                StateRootApplyStage::OverlayPrepare,
                elapsed_us(stage_start),
            );

            roots.push(new_root);
            current_root = Some(new_root);
        }

        let overlay = {
            let mut overlay = batch.overlay.lock();
            std::mem::take(&mut *overlay)
        };
        // Drain the unresolved deferred journal exported at the final
        // checkpoint commit; empty unless this batch ran with deferred
        // full-state journal export against a cursor-resolving backing store.
        let deferred_journal = std::mem::take(&mut *batch.deferred_journal.lock());
        let deferred_node_snapshot = (!deferred_journal.is_empty())
            .then(|| batch.pinned_node_snapshot())
            .flatten();
        drop(trie);
        drop(batch);

        let last_block_index = Self::last_block_index(blocks, "state-service MPT batch publish")?;
        let Some(last_root) = roots.last().copied() else {
            return Err(MptError::invalid(
                "state-service MPT batch produced no roots for a non-empty block batch",
            ));
        };
        let prepared = PreparedMptCommit::new(last_block_index, last_root, overlay, blocks.len())
            .with_deferred_journal(deferred_journal, deferred_node_snapshot);
        Ok((roots, prepared))
    }

    fn validate_ordered_batch(
        &self,
        root_before: Option<UInt256>,
        blocks: &[MptBlockChanges<'_>],
    ) -> MptResult<()> {
        let Some(first) = blocks.first() else {
            return Err(MptError::invalid(
                "state-service MPT batch validation requires at least one block",
            ));
        };
        match (self.current_local_root(), root_before) {
            (None, None) if first.block_index == 0 => {}
            (None, None) => {
                return Err(MptError::invalid(format!(
                    "non-contiguous state-service MPT batch: no local root exists before block {}",
                    first.block_index
                )));
            }
            (Some((previous_index, current_root)), Some(root_before))
                if previous_index.checked_add(1) == Some(first.block_index)
                    && current_root == root_before => {}
            (Some((previous_index, current_root)), Some(root_before)) => {
                return Err(MptError::invalid(format!(
                    "non-contiguous state-service MPT batch: current local root is ({previous_index}, {current_root}), requested previous root is {root_before} before block {}",
                    first.block_index
                )));
            }
            (None, Some(root_before)) => {
                return Err(MptError::invalid(format!(
                    "non-contiguous state-service MPT batch: requested previous root {root_before} but no local root exists before block {}",
                    first.block_index
                )));
            }
            (Some((previous_index, current_root)), None) => {
                return Err(MptError::invalid(format!(
                    "non-contiguous state-service MPT batch: current local root is ({previous_index}, {current_root}) but no previous root was supplied before block {}",
                    first.block_index
                )));
            }
        }

        for pair in blocks.windows(2) {
            if pair[0].block_index.checked_add(1) != Some(pair[1].block_index) {
                return Err(MptError::invalid(format!(
                    "non-contiguous state-service MPT batch: block {} followed by {}",
                    pair[0].block_index, pair[1].block_index
                )));
            }
        }
        Ok(())
    }

    fn last_block_index(blocks: &[MptBlockChanges<'_>], context: &'static str) -> MptResult<u32> {
        blocks
            .last()
            .map(|block| block.block_index)
            .ok_or_else(|| MptError::invalid(format!("{context} requires at least one block")))
    }

    fn publish_overlay(
        &self,
        block_index: u32,
        new_root: UInt256,
        overlay: MptOverlay,
    ) -> MptResult<UInt256> {
        self.publish_prepared(PreparedMptCommit::new(block_index, new_root, overlay, 1))
    }

    fn publish_prepared(&self, prepared: PreparedMptCommit) -> MptResult<UInt256> {
        let PreparedMptCommit {
            block_index,
            root_hash,
            node_overlay,
            metadata_overlay,
            samples,
            deferred_journal,
            ..
        } = prepared;
        if !deferred_journal.is_empty() {
            return Err(MptError::storage(
                "non-coordinated StateService publish cannot resolve an unresolved deferred journal",
            ));
        }
        let overlay = join_mpt_overlay(node_overlay, metadata_overlay);
        self.publish_overlay_with_samples(block_index, root_hash, overlay, samples)
    }

    fn publish_prepared_coordinated<F>(
        &self,
        mut prepared: PreparedMptCommit,
        commit: F,
    ) -> MptResult<UInt256>
    where
        F: FnOnce(&S, &mut PreparedMptCommit) -> StorageResult<()>,
    {
        let Some(backing) = self.backing.as_ref() else {
            return Err(MptError::storage(
                "coordinated StateService commit requires a durable backing store",
            ));
        };

        let stage_start = Instant::now();
        commit(backing, &mut prepared).map_err(|error| {
            MptError::storage(format!("coordinated StateService commit failed: {error}"))
        })?;
        if !prepared.nodes_visited || !prepared.metadata_visited {
            return Err(MptError::storage(
                "coordinated StateService commit did not consume both prepared node and metadata overlays",
            ));
        }
        if !prepared.deferred_journal.is_empty() && !prepared.journal_committed_at_cursor {
            return Err(MptError::storage(
                "coordinated StateService commit did not resolve the deferred journal at its write cursor",
            ));
        }
        Self::record_stage_samples(
            StateRootApplyStage::BackingCommit,
            elapsed_us(stage_start),
            prepared.samples as u64,
        );

        let PreparedMptCommit {
            block_index,
            root_hash,
            node_overlay,
            metadata_overlay,
            samples,
            counts,
            ..
        } = prepared;
        let overlay = join_mpt_overlay(node_overlay, metadata_overlay);
        self.publish_committed_overlay(block_index, root_hash, overlay, counts, samples)
    }

    fn publish_overlay_with_samples(
        &self,
        block_index: u32,
        new_root: UInt256,
        overlay: MptOverlay,
        samples: usize,
    ) -> MptResult<UInt256> {
        let sample_count = samples.max(1) as u64;
        let stage_start = Instant::now();
        let backing_result = self.commit_overlay_to_backing(&overlay);
        Self::record_stage_samples(
            StateRootApplyStage::BackingCommit,
            elapsed_us(stage_start),
            sample_count,
        );
        let backing_counts = backing_result?;

        self.publish_committed_overlay(block_index, new_root, overlay, backing_counts, samples)
    }

    fn publish_committed_overlay(
        &self,
        block_index: u32,
        new_root: UInt256,
        overlay: MptOverlay,
        backing_counts: OverlayCounts,
        samples: usize,
    ) -> MptResult<UInt256> {
        let samples = samples.max(1) as u64;

        let stage_start = Instant::now();
        let mut overlay_counts = OverlayCounts::default();
        if self.should_publish_live_overlay() {
            let mut kv = self.kv.write();
            let map = Arc::make_mut(&mut *kv);
            for (key, value) in overlay {
                overlay_counts.record(&key, value.as_deref());
                map.insert(key, value);
            }
        } else {
            overlay_counts = backing_counts;
            self.clear_live_overlay_if_needed();
        }
        Self::record_overlay_counts(overlay_counts, samples);
        *self.latest_local_root.write() = Some((block_index, new_root));
        Self::record_stage_samples(
            StateRootApplyStage::PublishGeneration,
            elapsed_us(stage_start),
            samples,
        );
        Ok(new_root)
    }

    fn publish_empty_root_batch(
        &self,
        blocks: &[MptBlockChanges<'_>],
        root_hash: UInt256,
    ) -> MptResult<UInt256> {
        let last_index = Self::last_block_index(blocks, "state-service empty-root batch")?;
        let samples = blocks.len().max(1) as u64;

        let stage_start = Instant::now();
        let empty_root = Node::new();
        let empty_root_record = if root_hash == empty_root.try_hash()? {
            Some((
                Self::mpt_node_key_bytes(root_hash),
                empty_root.to_array().map_err(MptError::from)?,
            ))
        } else {
            None
        };
        Self::record_stage_samples(
            StateRootApplyStage::OverlayPrepare,
            elapsed_us(stage_start),
            samples,
        );

        let stage_start = Instant::now();
        let backing_counts =
            self.commit_empty_root_batch_to_backing(blocks, root_hash, empty_root_record.as_ref())?;
        Self::record_stage_samples(
            StateRootApplyStage::BackingCommit,
            elapsed_us(stage_start),
            samples,
        );

        let stage_start = Instant::now();
        let overlay_counts = if self.should_publish_live_overlay() {
            let mut counts = OverlayCounts::default();
            let mut kv = self.kv.write();
            let map = Arc::make_mut(&mut *kv);
            for block in blocks {
                map.insert(
                    Keys::state_root(block.block_index),
                    Some(Self::encode_state_root_fields(block.block_index, root_hash)),
                );
                counts.record_put();
            }
            map.insert(
                Keys::CURRENT_LOCAL_ROOT_INDEX.to_vec(),
                Some(last_index.to_le_bytes().to_vec()),
            );
            counts.record_put();
            if let Some((key, value)) = empty_root_record {
                counts.record_node_put(value.len());
                map.insert(key.to_vec(), Some(value));
            }
            counts
        } else {
            self.clear_live_overlay_if_needed();
            backing_counts
        };
        Self::record_overlay_counts(overlay_counts, samples);
        *self.latest_local_root.write() = Some((last_index, root_hash));
        Self::record_stage_samples(
            StateRootApplyStage::PublishGeneration,
            elapsed_us(stage_start),
            samples,
        );
        Ok(root_hash)
    }

    fn commit_empty_root_batch_to_backing(
        &self,
        blocks: &[MptBlockChanges<'_>],
        root_hash: UInt256,
        empty_root_record: Option<&([u8; 1 + UINT256_SIZE], Vec<u8>)>,
    ) -> MptResult<OverlayCounts> {
        match self.backing.as_ref() {
            None => Ok(OverlayCounts::default()),
            Some(backing) => {
                let current_index =
                    Self::last_block_index(blocks, "state-service empty-root backing commit")?;
                let current_index_value = current_index.to_le_bytes();
                let mut source = EmptyRootBatchOverlaySource::new(
                    blocks,
                    root_hash,
                    empty_root_record,
                    current_index,
                );
                let committed = backing
                    .try_commit_borrowed_raw_overlay(&mut source)
                    .map_err(|err| {
                        MptError::storage(format!(
                            "state-service empty-batch backing commit failed: {err}"
                        ))
                    })?;
                if committed {
                    return Ok(source.counts);
                }

                let mut counts = OverlayCounts::default();
                let mut snapshot = backing.snapshot();
                let writer = Arc::get_mut(&mut snapshot).ok_or_else(|| {
                    MptError::storage(
                        "unable to obtain mutable state-service backing snapshot for empty-batch commit",
                    )
                })?;
                for block in blocks {
                    counts.record_put();
                    writer
                        .put(
                            Keys::state_root(block.block_index),
                            Self::encode_state_root_fields(block.block_index, root_hash),
                        )
                        .map_err(|err| {
                            MptError::storage(format!(
                                "state-service empty-batch backing write failed: {err}"
                            ))
                        })?;
                }
                counts.record_put();
                writer
                    .put(
                        Keys::CURRENT_LOCAL_ROOT_INDEX.to_vec(),
                        current_index_value.to_vec(),
                    )
                    .map_err(|err| {
                        MptError::storage(format!(
                            "state-service empty-batch backing write failed: {err}"
                        ))
                    })?;
                if let Some((key, value)) = empty_root_record {
                    counts.record_node_put(value.len());
                    writer.put(key.to_vec(), value.clone()).map_err(|err| {
                        MptError::storage(format!(
                            "state-service empty-batch backing write failed: {err}"
                        ))
                    })?;
                }
                writer.try_commit().map_err(|err| {
                    MptError::storage(format!(
                        "state-service empty-batch backing commit failed: {err}"
                    ))
                })?;
                Ok(counts)
            }
        }
    }

    fn clear_live_overlay_if_needed(&self) {
        if self.kv.read().is_empty() {
            return;
        }

        let mut kv = self.kv.write();
        if !kv.is_empty() {
            *kv = Arc::new(HashMap::new());
        }
    }

    fn local_root_overlay(block_index: u32, root_hash: UInt256) -> MptOverlay {
        let mut overlay = MptOverlay::with_capacity_and_hasher(2, Default::default());
        Self::insert_local_root_records(&mut overlay, block_index, root_hash);
        StateRootApplyMetrics::record_stage(StateRootApplyStage::OverlayPrepare, 0);
        overlay
    }

    fn should_publish_live_overlay(&self) -> bool {
        self.backing.is_none()
    }

    fn insert_local_root_records(overlay: &mut MptOverlay, block_index: u32, root_hash: UInt256) {
        overlay.insert(
            Keys::state_root(block_index),
            Some(Self::encode_state_root_fields(block_index, root_hash)),
        );
        overlay.insert(
            Keys::CURRENT_LOCAL_ROOT_INDEX.to_vec(),
            Some(block_index.to_le_bytes().to_vec()),
        );
    }

    fn ensure_empty_root_record(overlay: &mut MptOverlay, root_hash: UInt256) -> MptResult<()> {
        let empty = Node::new();
        if root_hash != empty.try_hash()? {
            return Ok(());
        }
        overlay.insert(
            Self::mpt_node_key(root_hash),
            Some(empty.to_array().map_err(MptError::from)?),
        );
        Ok(())
    }

    fn mpt_node_key(root_hash: UInt256) -> Vec<u8> {
        Self::mpt_node_key_bytes(root_hash).to_vec()
    }

    fn mpt_node_key_bytes(root_hash: UInt256) -> [u8; 1 + UINT256_SIZE] {
        let mut key = [0u8; 1 + UINT256_SIZE];
        key[0] = MPT_NODE_PREFIX;
        key[1..].copy_from_slice(&root_hash.to_array());
        key
    }

    fn state_root_key_bytes(index: u32) -> [u8; 5] {
        let mut key = [0u8; 5];
        key[0] = 0x01;
        key[1..].copy_from_slice(&index.to_be_bytes());
        key
    }

    fn state_root_record_len() -> usize {
        STATE_ROOT_UNSIGNED_LEN + 1
    }

    fn encode_state_root_fields(index: u32, root_hash: UInt256) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(Self::state_root_record_len());
        bytes.push(CURRENT_VERSION);
        bytes.extend_from_slice(&index.to_le_bytes());
        bytes.extend_from_slice(&root_hash.to_bytes());
        bytes.push(0x00);
        bytes
    }

    fn record_overlay_counts(counts: OverlayCounts, samples: u64) {
        let samples = samples.max(1);
        Self::record_count_samples(
            StateRootApplyCountKind::OverlayEntries,
            counts.entries(),
            samples,
        );
        Self::record_count_samples(StateRootApplyCountKind::OverlayPuts, counts.puts, samples);
        Self::record_count_samples(
            StateRootApplyCountKind::OverlayDeletes,
            counts.deletes,
            samples,
        );
        Self::record_count_samples(StateRootApplyCountKind::NodePuts, counts.node_puts, samples);
        Self::record_count_samples(
            StateRootApplyCountKind::NodeDeletes,
            counts.node_deletes,
            samples,
        );
        for (kind, count) in [
            (
                StateRootApplyCountKind::NodeValueSize0To64,
                counts.node_value_sizes[0],
            ),
            (
                StateRootApplyCountKind::NodeValueSize65To128,
                counts.node_value_sizes[1],
            ),
            (
                StateRootApplyCountKind::NodeValueSize129To256,
                counts.node_value_sizes[2],
            ),
            (
                StateRootApplyCountKind::NodeValueSize257To512,
                counts.node_value_sizes[3],
            ),
            (
                StateRootApplyCountKind::NodeValueSize513To1024,
                counts.node_value_sizes[4],
            ),
            (
                StateRootApplyCountKind::NodeValueSize1025To4096,
                counts.node_value_sizes[5],
            ),
            (
                StateRootApplyCountKind::NodeValueSize4097To16384,
                counts.node_value_sizes[6],
            ),
            (
                StateRootApplyCountKind::NodeValueSizeOver16384,
                counts.node_value_sizes[7],
            ),
        ] {
            Self::record_count_samples(kind, count, samples);
        }
        for (kind, bytes) in [
            (
                StateRootApplyCountKind::NodeValueBytes0To64,
                counts.node_value_bytes[0],
            ),
            (
                StateRootApplyCountKind::NodeValueBytes65To128,
                counts.node_value_bytes[1],
            ),
            (
                StateRootApplyCountKind::NodeValueBytes129To256,
                counts.node_value_bytes[2],
            ),
            (
                StateRootApplyCountKind::NodeValueBytes257To512,
                counts.node_value_bytes[3],
            ),
            (
                StateRootApplyCountKind::NodeValueBytes513To1024,
                counts.node_value_bytes[4],
            ),
            (
                StateRootApplyCountKind::NodeValueBytes1025To4096,
                counts.node_value_bytes[5],
            ),
            (
                StateRootApplyCountKind::NodeValueBytes4097To16384,
                counts.node_value_bytes[6],
            ),
            (
                StateRootApplyCountKind::NodeValueBytesOver16384,
                counts.node_value_bytes[7],
            ),
        ] {
            Self::record_count_samples(kind, bytes, samples);
        }
    }

    fn record_stage_samples(stage: StateRootApplyStage, elapsed_us: u64, samples: u64) {
        let samples = samples.max(1);
        let base = elapsed_us / samples;
        let remainder = elapsed_us % samples;
        for index in 0..samples {
            let sample = base + u64::from(index < remainder);
            StateRootApplyMetrics::record_stage(stage, sample);
        }
    }

    fn record_count_samples(kind: StateRootApplyCountKind, count: u64, samples: u64) {
        let samples = samples.max(1);
        let base = count / samples;
        let remainder = count % samples;
        for index in 0..samples {
            let sample = base + u64::from(index < remainder);
            StateRootApplyMetrics::record_count(kind, sample);
        }
    }

    /// Rewinds local state-root records for an inclusive reverted block range.
    ///
    /// Full-state mode retains historical trie nodes, so rewinding the current
    /// local root pointer to the block before `from_index` leaves the trie
    /// resolvable. Pruning mode may have deleted those historical nodes while
    /// applying later blocks, so it refuses rewinds that would move the current
    /// root backward.
    pub fn revert_local_roots(&self, from_index: u32, to_index: u32) -> MptResult<()> {
        if from_index > to_index {
            return Ok(());
        }
        if self.node_snapshots.is_some() {
            return Err(MptError::invalid(
                "split-authority StateService reverts require the coordinated marker path",
            ));
        }

        let _writer = self.write_gate.lock();
        let (overlay, rewound_latest_root) =
            self.prepare_local_root_revert(from_index, to_index)?;
        self.commit_overlay_to_backing(&overlay)?;

        if self.should_publish_live_overlay() {
            let mut kv = self.kv.write();
            let map = Arc::make_mut(&mut *kv);
            map.extend(overlay);
        } else {
            self.clear_live_overlay_if_needed();
        }
        *self.latest_local_root.write() = rewound_latest_root;
        Ok(())
    }

    /// Rewinds local-root metadata through an externally coordinated commit.
    /// Split node authority uses this path so the unchanged pack horizon and
    /// rewound StateService tip cross the mandatory MDBX marker transaction
    /// together.
    pub fn revert_local_roots_coordinated<F>(
        &self,
        from_index: u32,
        to_index: u32,
        commit: F,
    ) -> MptResult<()>
    where
        F: FnOnce(&S, &mut PreparedMptCommit) -> StorageResult<()>,
    {
        if from_index > to_index {
            return Ok(());
        }
        let _writer = self.write_gate.lock();
        let (overlay, rewound_latest_root) =
            self.prepare_local_root_revert(from_index, to_index)?;
        let (block_index, root_hash) = rewound_latest_root.ok_or_else(|| {
            MptError::invalid("coordinated StateService authority cannot rewind before genesis")
        })?;
        self.publish_prepared_coordinated(
            PreparedMptCommit::new(block_index, root_hash, overlay, 1),
            commit,
        )?;
        Ok(())
    }

    fn prepare_local_root_revert(
        &self,
        from_index: u32,
        to_index: u32,
    ) -> MptResult<(MptOverlay, Option<(u32, UInt256)>)> {
        let current_index = self.current_local_root_index();
        let rewinds_current = current_index.is_some_and(|index| index >= from_index);
        if rewinds_current && !self.full_state {
            return Err(MptError::invalid(
                "cannot rewind pruning-mode StateService MPT local roots because historical trie nodes may have been pruned",
            ));
        }

        let mut overlay = MptOverlay::with_capacity_and_hasher(
            (to_index - from_index + 1) as usize + 1,
            Default::default(),
        );
        for index in from_index..=to_index {
            overlay.insert(Keys::state_root(index), None);
        }

        let mut rewound_latest_root = *self.latest_local_root.read();
        if rewinds_current {
            match from_index.checked_sub(1) {
                Some(previous_index) => {
                    let map = Arc::clone(&self.kv.read());
                    let backing_snapshot = self.backing_snapshot();
                    let previous_root = Self::read_state_root(
                        &map,
                        backing_snapshot.as_deref(),
                        previous_index,
                    )?
                    .ok_or_else(|| {
                        MptError::invalid(format!(
                            "cannot rewind StateService MPT local root to missing block {previous_index}"
                        ))
                    })?;
                    rewound_latest_root = Some((previous_index, *previous_root.root_hash()));
                    overlay.insert(
                        Keys::CURRENT_LOCAL_ROOT_INDEX.to_vec(),
                        Some(previous_index.to_le_bytes().to_vec()),
                    );
                }
                None => {
                    rewound_latest_root = None;
                    overlay.insert(Keys::CURRENT_LOCAL_ROOT_INDEX.to_vec(), None);
                }
            }
        }

        Ok((overlay, rewound_latest_root))
    }

    fn commit_overlay_to_backing(&self, overlay: &MptOverlay) -> MptResult<OverlayCounts> {
        match self.backing.as_ref() {
            None => Ok(OverlayCounts::default()),
            Some(backing) => {
                let mut entries = overlay.iter().collect::<Vec<_>>();
                entries.sort_unstable_by(|left, right| left.0.cmp(right.0));
                let mut source = SortedOverlaySource {
                    entries: &entries,
                    counts: OverlayCounts::default(),
                };
                let committed = backing
                    .try_commit_borrowed_raw_overlay(&mut source)
                    .map_err(|err| {
                        MptError::storage(format!("state-service backing commit failed: {err}"))
                    })?;
                if committed {
                    return Ok(source.counts);
                }

                let mut counts = OverlayCounts::default();
                let mut snapshot = backing.snapshot();
                let writer = Arc::get_mut(&mut snapshot).ok_or_else(|| {
                    MptError::storage(
                        "unable to obtain mutable state-service backing snapshot for commit",
                    )
                })?;
                for (key, value) in entries {
                    counts.record(key, value.as_deref());
                    match value {
                        Some(value) => writer.put(key.clone(), value.clone()),
                        None => writer.delete(key.clone()),
                    }
                    .map_err(|err| {
                        MptError::storage(format!("state-service backing write failed: {err}"))
                    })?;
                }
                writer.try_commit().map_err(|err| {
                    MptError::storage(format!("state-service backing commit failed: {err}"))
                })?;
                Ok(counts)
            }
        }
    }

    /// Fallible state-root lookup for the live store generation.
    pub fn try_get_state_root(&self, index: u32) -> MptResult<Option<StateRoot>> {
        let map = Arc::clone(&self.kv.read());
        let backing_snapshot = self.backing_snapshot();
        Self::read_state_root(&map, backing_snapshot.as_deref(), index)
    }

    /// Returns the state-root record persisted for `index`, if any.
    ///
    /// Prefer [`Self::try_get_state_root`] when backend failures must abort.
    pub fn get_state_root(&self, index: u32) -> Option<StateRoot> {
        match self.try_get_state_root(index) {
            Ok(root) => root,
            Err(error) => {
                tracing::error!(
                    target: "neo.state_service",
                    index,
                    error = %error,
                    "MPT state-root read failed; treating as absent for legacy Option API"
                );
                None
            }
        }
    }

    /// Returns the current local root index, if a block has been
    /// applied (C# `StateSnapshot.CurrentLocalRootIndex`).
    pub fn current_local_root_index(&self) -> Option<u32> {
        self.current_local_root().map(|(index, _)| index)
    }

    /// Returns the current local root hash, if a block has been
    /// applied (C# `StateSnapshot.CurrentLocalRootHash`).
    ///
    /// Callers that also walk the trie at the returned root must use
    /// [`MptStore::snapshot`] and read both from the same frozen view.
    pub fn current_local_root_hash(&self) -> Option<UInt256> {
        self.current_local_root().map(|(_, root)| root)
    }

    /// Returns the current local root index and hash from one generation.
    ///
    /// This keeps hot-path contiguity checks from taking separate snapshots for
    /// the index and hash.
    pub fn current_local_root(&self) -> Option<(u32, UInt256)> {
        *self.latest_local_root.read()
    }

    /// Decodes the state-root record for `index` out of a generation
    /// map (shared by the live accessors and [`MptReadSnapshot`]).
    ///
    /// Returns `Ok(None)` when the record is absent and `Err` when the durable
    /// backing read fails.
    fn read_state_root<B>(
        map: &HashMap<Vec<u8>, Option<Vec<u8>>>,
        backing_snapshot: Option<&B>,
        index: u32,
    ) -> MptResult<Option<StateRoot>>
    where
        B: RawReadOnlyStore + ?Sized,
    {
        let key = Keys::state_root(index);
        let bytes = match map.get(&key) {
            Some(Some(bytes)) => bytes.clone(),
            Some(None) => return Ok(None),
            None => {
                let Some(backing) = backing_snapshot else {
                    return Ok(None);
                };
                match backing.try_get_bytes_result(&key).map_err(|error| {
                    MptError::storage(format!("MPT state-root backing get failed: {error}"))
                })? {
                    Some(bytes) => bytes,
                    None => return Ok(None),
                }
            }
        };
        match Self::decode_state_root(&bytes) {
            Some(root) => Ok(Some(root)),
            None => {
                tracing::warn!(
                    target: "neo.state_service",
                    index,
                    "malformed state-root record in MPT store"
                );
                Ok(None)
            }
        }
    }

    /// Reads the current local root index out of a generation map.
    ///
    /// Returns `Ok(None)` when no index is published and `Err` when the durable
    /// backing read fails.
    fn read_current_local_root_index<B>(
        map: &HashMap<Vec<u8>, Option<Vec<u8>>>,
        backing_snapshot: Option<&B>,
    ) -> MptResult<Option<u32>>
    where
        B: RawReadOnlyStore + ?Sized,
    {
        let key = Keys::CURRENT_LOCAL_ROOT_INDEX.to_vec();
        let bytes = match map.get(&key) {
            Some(Some(bytes)) => bytes.clone(),
            Some(None) => return Ok(None),
            None => {
                let Some(backing) = backing_snapshot else {
                    return Ok(None);
                };
                match backing.try_get_bytes_result(&key).map_err(|error| {
                    MptError::storage(format!(
                        "MPT current local root index backing get failed: {error}"
                    ))
                })? {
                    Some(bytes) => bytes,
                    None => return Ok(None),
                }
            }
        };
        let arr: [u8; 4] = bytes
            .as_slice()
            .try_into()
            .map_err(|_| MptError::invalid("current local root index record has invalid length"))?;
        Ok(Some(u32::from_le_bytes(arr)))
    }

    /// Serializes a state root in the C# `StateRoot` wire format:
    /// unsigned fields (`version u8 + index u32 LE + root_hash`)
    /// followed by a var-int witness count of `0` (local roots carry
    /// no witness; C# writes `WriteVarInt(0)` for a null witness).
    /// Decodes the unsigned prefix of a C#-format `StateRoot` record;
    /// the trailing witness array (if any) is ignored because the
    /// state service only needs `(version, index, root_hash)` here.
    fn decode_state_root(bytes: &[u8]) -> Option<StateRoot> {
        if bytes.len() < STATE_ROOT_UNSIGNED_LEN {
            return None;
        }
        let version = bytes[0];
        let index = u32::from_le_bytes(bytes[1..5].try_into().ok()?);
        let root_hash = UInt256::from_bytes(&bytes[5..5 + UINT256_SIZE]).ok()?;
        Some(StateRoot::new(version, index, root_hash))
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct ProcessResourceSnapshot {
    read_bytes: u64,
    minor_faults: u64,
    major_faults: u64,
}

impl ProcessResourceSnapshot {
    fn delta_since(self, before: Self) -> Self {
        Self {
            read_bytes: self.read_bytes.saturating_sub(before.read_bytes),
            minor_faults: self.minor_faults.saturating_sub(before.minor_faults),
            major_faults: self.major_faults.saturating_sub(before.major_faults),
        }
    }
}

fn record_deferred_resource_delta(
    before: Option<ProcessResourceSnapshot>,
    after: Option<ProcessResourceSnapshot>,
) {
    let (Some(before), Some(after)) = (before, after) else {
        return;
    };
    let delta = after.delta_since(before);
    StateRootApplyMetrics::record_count(
        StateRootApplyCountKind::DeferredFinalizationReadBytes,
        delta.read_bytes,
    );
    StateRootApplyMetrics::record_count(
        StateRootApplyCountKind::DeferredFinalizationMinorFaults,
        delta.minor_faults,
    );
    StateRootApplyMetrics::record_count(
        StateRootApplyCountKind::DeferredFinalizationMajorFaults,
        delta.major_faults,
    );
}

/// Best-effort Linux process resource counters. Restricted containers and
/// non-Linux platforms simply omit the evidence without affecting correctness.
fn process_resource_snapshot() -> Option<ProcessResourceSnapshot> {
    let io = fs::read_to_string("/proc/self/io").ok()?;
    let stat = fs::read_to_string("/proc/self/stat").ok()?;
    let (minor_faults, major_faults) = proc_stat_faults(&stat)?;
    Some(ProcessResourceSnapshot {
        read_bytes: proc_io_counter(&io, "read_bytes")?,
        minor_faults,
        major_faults,
    })
}

fn proc_io_counter(input: &str, name: &str) -> Option<u64> {
    input.lines().find_map(|line| {
        let (field, value) = line.split_once(':')?;
        (field == name).then(|| value.trim().parse().ok()).flatten()
    })
}

fn proc_stat_faults(input: &str) -> Option<(u64, u64)> {
    let mut fields = input.get(input.rfind(')')? + 1..)?.split_whitespace();
    // After the parenthesized process name, state is field 3 (index 0).
    let minor_faults = fields.nth(7)?.parse().ok()?;
    let major_faults = fields.nth(1)?.parse().ok()?;
    Some((minor_faults, major_faults))
}

fn elapsed_us(start: Instant) -> u64 {
    start.elapsed().as_micros().min(u64::MAX as u128) as u64
}

impl MptStore<MemoryStore> {
    /// Constructs an empty in-memory store.
    ///
    /// `full_state` mirrors the C# `FullState` setting: `true` keeps every
    /// historical trie version resolvable, `false` prunes superseded nodes on
    /// each block.
    pub fn new(full_state: bool) -> Self {
        Self::new_with_options(full_state, false)
    }

    /// Constructs an empty store with an explicit full-state finalization policy.
    pub fn new_with_options(full_state: bool, defer_full_state_finalization: bool) -> Self {
        Self {
            kv: RwLock::new(Arc::new(HashMap::new())),
            write_gate: Mutex::new(()),
            full_state,
            defer_full_state_finalization: full_state && defer_full_state_finalization,
            latest_local_root: RwLock::new(None),
            backing: None,
            node_snapshots: None,
        }
    }

    /// Opens a store over a concrete in-memory backend.
    ///
    /// This is primarily for tests and ephemeral nodes that need persistence
    /// across `MptStore` instances without erasing the backend.
    pub fn from_memory_store(backing: Arc<MemoryStore>, full_state: bool) -> MptResult<Self> {
        Self::from_store(backing, full_state)
    }

    /// Opens an in-memory backend with an explicit full-state finalization policy.
    pub fn from_memory_store_with_options(
        backing: Arc<MemoryStore>,
        full_state: bool,
        defer_full_state_finalization: bool,
    ) -> MptResult<Self> {
        Self::from_store_with_options(backing, full_state, defer_full_state_finalization)
    }
}

impl<S> std::fmt::Debug for MptStore<S>
where
    S: Store,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Clone the generation pointer once so no lock guard is held
        // across the field reads (parking_lot read locks are not
        // recursive once a writer queues up).
        let map = Arc::clone(&self.kv.read());
        f.debug_struct("MptStore")
            .field("entries", &map.len())
            .field("full_state", &self.full_state)
            .field(
                "defer_full_state_finalization",
                &self.defer_full_state_finalization,
            )
            .field("local_root_index", &self.current_local_root_index())
            .finish()
    }
}

#[cfg(test)]
#[path = "../tests/storage/mpt_store.rs"]
mod tests;

#[cfg(test)]
#[path = "../tests/storage/mpt_store_transient_overlay.rs"]
mod transient_overlay_tests;
