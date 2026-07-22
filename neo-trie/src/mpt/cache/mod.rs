//! # MPT Mutation Cache
//!
//! ## Boundary
//!
//! This module tracks deterministic node mutations and delegates byte storage
//! through [`MptStoreSnapshot`]. Durable transactions and StateService policy
//! remain outside this crate.
//!
//! ## Contents
//!
//! Store capabilities, cache state, mutation/finalization operations, and
//! request-local finalization telemetry.

mod operations;
mod telemetry;

#[cfg(test)]
pub(crate) use telemetry::{ProcessResourceSnapshot, proc_io_counter, proc_stat_faults};

use super::error::{MptError, MptResult};
use super::node::Node;
use super::node_type::NodeType;
use neo_io::{MemoryReader, Serializable};
use neo_primitives::UINT256_SIZE;
use neo_primitives::UInt256;
use rustc_hash::FxHashMap;
use std::collections::hash_map::Entry;
use std::sync::Arc;

pub(crate) fn node_key_bytes(prefix: u8, hash: &UInt256) -> [u8; 1 + UINT256_SIZE] {
    let mut key = [0u8; 1 + UINT256_SIZE];
    key[0] = prefix;
    key[1..].copy_from_slice(&hash.to_array());
    key
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TrackState {
    None,
    Added,
    Changed,
    Deleted,
}

/// Abstraction over the persistence snapshot used by the trie cache.
pub trait MptStoreSnapshot: Send + Sync {
    /// Retrieves the serialized node associated with the specified key.
    fn try_get(&self, key: &[u8]) -> MptResult<Option<Vec<u8>>>;

    /// Retrieves and decodes a trie node associated with the specified key.
    ///
    /// Backends that retain decoded immutable nodes can override this to avoid
    /// repeating serialization parsing on adjacent trie updates. The default
    /// preserves the original byte-oriented storage contract.
    fn try_get_node(&self, key: &[u8]) -> MptResult<Option<Node>> {
        decode_node(self.try_get(key)?)
    }

    /// Retrieves and decodes a node while retaining its storage provenance.
    ///
    /// The default treats the snapshot itself as the backing authority and
    /// deliberately reuses `try_get_node`, including backend decode caches.
    /// Layered stores override this to distinguish staged reuse from durable
    /// reads without adding global hot-path counters.
    fn try_get_node_with_source(&self, key: &[u8]) -> MptResult<MptStoreLookup<Node>> {
        Ok(MptStoreLookup::Backing(self.try_get_node(key)?))
    }

    /// Resolves an ordered set of nodes with all-or-nothing error semantics.
    ///
    /// Backends with a native batch or cursor path override this method. The
    /// default preserves exact input order and duplicate keys.
    fn try_get_nodes_with_source(&self, keys: &[Vec<u8>]) -> MptResult<Vec<MptStoreLookup<Node>>> {
        keys.iter()
            .map(|key| self.try_get_node_with_source(key))
            .collect()
    }

    /// Resolves a sorted set of borrowed node keys without requiring callers to
    /// allocate one owned `Vec<u8>` per key. Keys must be non-decreasing in raw
    /// byte order. The default keeps the same scalar semantics for stores that
    /// do not provide a batch implementation.
    fn try_get_nodes_with_source_borrowed<K>(
        &self,
        keys: &[K],
    ) -> MptResult<Vec<MptStoreLookup<Node>>>
    where
        K: AsRef<[u8]>,
    {
        // Preserve existing custom bulk implementations (including their
        // error and retry behavior). Only specialized stores need to override
        // this surface to avoid the compatibility allocation.
        let owned = keys
            .iter()
            .map(|key| key.as_ref().to_vec())
            .collect::<Vec<_>>();
        self.try_get_nodes_with_source(&owned)
    }

    /// Resolves sorted node bytes while retaining whether each value came from
    /// the mutable overlay or the frozen backing snapshot.
    ///
    /// Backends with a raw batch path override this method. The default keeps
    /// custom snapshots correct by serializing the decoded node representation.
    fn try_get_nodes_with_source_raw_borrowed<K>(
        &self,
        keys: &[K],
    ) -> MptResult<Vec<MptStoreLookup<Vec<u8>>>>
    where
        K: AsRef<[u8]>,
    {
        self.try_get_nodes_with_source_borrowed(keys)?
            .into_iter()
            .map(|lookup| {
                let encode = |node: Option<Node>| {
                    node.map(|node| {
                        let payload = node.to_array_without_reference()?;
                        Node::array_from_payload_parts(node.node_type, node.reference, &payload)
                    })
                    .transpose()
                };
                match lookup {
                    MptStoreLookup::InMemory(node) => Ok(MptStoreLookup::InMemory(encode(node)?)),
                    MptStoreLookup::Backing(node) => Ok(MptStoreLookup::Backing(encode(node)?)),
                }
            })
            .collect()
    }

    /// Persists the serialized node for the supplied key.
    fn put(&self, key: Vec<u8>, value: Vec<u8>) -> MptResult<()>;

    /// Stages an unresolved deferred full-state journal so the backing commit
    /// can resolve each entry at its write cursor.
    ///
    /// Returns `true` when the store carries the journal to its commit path.
    /// Stores that cannot (including every default implementation) return
    /// `false`, and the caller resolves the journal against the backing
    /// snapshot itself, preserving the classic resolve-then-write flow.
    fn stage_unresolved_deferred_journal(
        &self,
        journal: Vec<UnresolvedDeferredNode>,
    ) -> MptResult<bool> {
        let _ = journal;
        Ok(false)
    }

    /// Removes the value associated with the supplied key.
    fn delete(&self, key: Vec<u8>) -> MptResult<()>;

    /// Applies a batch of serialized-node mutations.
    ///
    /// The default preserves the original per-entry semantics. Hot write-batch
    /// stores can override this to merge bookkeeping/lock acquisition.
    fn apply_overlay(&self, overlay: Vec<(Vec<u8>, Option<Vec<u8>>)>) -> MptResult<()> {
        for (key, value) in overlay {
            match value {
                Some(value) => self.put(key, value)?,
                None => self.delete(key)?,
            }
        }
        Ok(())
    }
}

fn decode_node(bytes: Option<Vec<u8>>) -> MptResult<Option<Node>> {
    let Some(bytes) = bytes else {
        return Ok(None);
    };
    let mut reader = MemoryReader::new(&bytes);
    Node::deserialize(&mut reader)
        .map(Some)
        .map_err(MptError::from)
}

/// Result of a layered MPT snapshot lookup with storage provenance retained.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MptStoreLookup<T> {
    /// The key was resolved by the current mutable generation or write overlay.
    InMemory(Option<T>),
    /// The key required consulting the frozen backing snapshot.
    Backing(Option<T>),
}

/// One unresolved deferred full-state journal entry: the journaled put count
/// and the serialized payload without its persisted reference count.
///
/// Produced by [`MptCache::commit`] when deferred journal export is enabled.
/// The backing store carries these entries to its coordinated commit and
/// resolves them at the write cursor via [`UnresolvedDeferredNode::resolve_bytes`],
/// which replaces the classic resolve-then-encode sweep against the frozen
/// backing snapshot with a read-modify-write at the commit cursor.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnresolvedDeferredNode {
    /// Full storage key (prefix byte + node hash).
    pub key: Vec<u8>,
    /// Node kind of the first journaled put.
    pub node_type: NodeType,
    /// Serialized node payload without the reference count (the hash preimage).
    pub payload_without_reference: Vec<u8>,
    /// Number of journaled put operations for this node hash.
    pub puts: u32,
}

impl UnresolvedDeferredNode {
    /// Computes the final stored bytes for this journal entry given the value
    /// currently persisted under the same key.
    ///
    /// This reproduces `MptCache::deferred_overlay`'s per-entry result for a
    /// put-only journal exactly: the persisted reference (zero when the key is
    /// absent) plus the journaled put count, re-encoded with the payload and
    /// node type carried by the stored value when one exists. Stored bytes are
    /// validated with the same structural parser as the classic path, so
    /// corrupt rows fail the commit in both flows. Full-state journals never
    /// contain deletes (`Trie::previous_hash` yields `None` in full-state
    /// mode), which is what makes a put-count summary lossless.
    pub fn resolve_bytes(&self, stored: Option<Vec<u8>>) -> MptResult<Vec<u8>> {
        let puts = i32::try_from(self.puts).map_err(|_| {
            MptError::invalid("deferred MPT journal put count overflows the reference count")
        })?;
        match stored {
            Some(stored) => {
                // The base payload and type win over the journaled ones, just
                // like `replay_deferred_operations` keeps the base state and
                // only accumulates the reference for existing nodes.
                let (node_type, reference, payload_without_reference) =
                    Node::split_serialized_reference_owned(stored)?;
                Node::array_from_payload_parts_owned(
                    node_type,
                    reference.wrapping_add(puts),
                    payload_without_reference,
                )
            }
            None => Node::array_from_payload_parts(
                self.node_type,
                puts,
                &self.payload_without_reference,
            ),
        }
    }
}

pub(crate) struct PendingNodeFinalization {
    node_type: NodeType,
    hash: UInt256,
    payload_without_reference: Vec<u8>,
}

struct MptTrackable {
    /// Materialized only for store-loaded nodes or an in-cache resolve.
    node: Option<Node>,
    node_type: Option<NodeType>,
    reference: i32,
    payload_without_reference: Option<Vec<u8>>,
    state: TrackState,
    produced_in_current_commit: bool,
}

enum DeferredReferenceOperation {
    Put {
        node_type: NodeType,
        payload_without_reference: Vec<u8>,
    },
    Delete,
}

struct DeferredReferenceOperationRecord {
    operation: DeferredReferenceOperation,
    next: Option<usize>,
}

#[derive(Clone, Copy)]
struct DeferredReferenceEntry {
    first: usize,
    last: usize,
}

struct DeferredNodeState {
    node_type: NodeType,
    reference: i32,
    payload_without_reference: Vec<u8>,
}

impl DeferredNodeState {
    fn from_node(node: &Node) -> MptResult<Self> {
        Ok(Self {
            node_type: node.node_type,
            reference: node.reference,
            payload_without_reference: node.to_array_without_reference()?,
        })
    }

    fn to_bytes(&self) -> MptResult<Vec<u8>> {
        Node::array_from_payload_parts(
            self.node_type,
            self.reference,
            &self.payload_without_reference,
        )
    }

    fn into_bytes(self) -> MptResult<Vec<u8>> {
        Node::array_from_payload_parts_owned(
            self.node_type,
            self.reference,
            self.payload_without_reference,
        )
    }

    fn from_serialized_owned(bytes: Vec<u8>) -> MptResult<Self> {
        let (node_type, reference, payload_without_reference) =
            Node::split_serialized_reference_owned(bytes)?;
        Ok(Self {
            node_type,
            reference,
            payload_without_reference,
        })
    }

    fn into_node(self, hash: UInt256) -> MptResult<Node> {
        let bytes = self.to_bytes()?;
        let mut reader = MemoryReader::new(&bytes);
        let mut node = Node::deserialize(&mut reader).map_err(MptError::from)?;
        node.set_accounted_hash(hash);
        Ok(node)
    }
}

impl MptTrackable {
    fn new(node: Option<Node>) -> Self {
        let node_type = node.as_ref().map(|node| node.node_type);
        let reference = node.as_ref().map_or(0, |node| node.reference);
        Self {
            node,
            node_type,
            reference,
            payload_without_reference: None,
            state: TrackState::None,
            produced_in_current_commit: false,
        }
    }

    fn resolve_clone(&mut self) -> MptResult<Option<Node>> {
        let Some(node_type) = self.node_type else {
            return Ok(None);
        };
        if self.node.is_none() {
            let payload_without_reference =
                self.payload_without_reference.as_ref().ok_or_else(|| {
                    MptError::invalid(
                        "cache entry cannot materialize without serialized node payload",
                    )
                })?;
            let bytes = Node::array_from_payload_parts(
                node_type,
                self.reference,
                payload_without_reference,
            )?;
            let mut reader = MemoryReader::new(&bytes);
            self.node = Some(Node::deserialize(&mut reader).map_err(MptError::from)?);
        }
        let node = self
            .node
            .as_mut()
            .ok_or_else(|| MptError::invalid("cache entry lost its materialized node"))?;
        node.reference = self.reference;
        Ok(Some(node.clone()))
    }
}

/// Mutation work observed by one trie since the previous snapshot reset.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MptMutationStats {
    /// Nodes serialized and finalized through the mutation cache.
    pub put_node_cached_calls: u64,
    /// Bytes serialized as hash preimages by cached node finalization.
    pub serialized_payload_bytes: u64,
    /// Actual SHA-256 computations performed while mutating or hashing the root.
    pub hash_computations: u64,
    /// Deepest recursive put/delete frame reached by one mutation.
    pub max_recursion_depth: u64,
    /// Trie hash resolutions served by this mutation cache without a store read.
    pub trie_resolve_cache_hits: u64,
    /// Trie hash resolutions found in the underlying store snapshot.
    pub trie_resolve_store_hits: u64,
    /// Trie hash resolutions absent from the underlying store snapshot.
    pub trie_resolve_store_misses: u64,
    /// Nanoseconds spent loading and decoding trie nodes from the store snapshot.
    ///
    /// Resolution is timed per store miss, so retaining nanoseconds avoids
    /// dropping every sub-microsecond lookup before block-level aggregation.
    pub trie_resolve_store_ns: u64,
    /// Branch/extension nodes finalized more than once in one cache epoch.
    pub repeated_ancestor_finalizations: u64,
    /// Finalized hashes already resolved in the current trie cache epoch.
    pub finalization_cache_hits: u64,
    /// Finalized hashes found in the mutable generation or write overlay.
    pub finalization_memory_hits: u64,
    /// Finalized hashes hidden or absent in the mutable generation.
    pub finalization_memory_misses: u64,
    /// Finalized hashes found in the frozen backing snapshot.
    pub finalization_backing_hits: u64,
    /// Finalized hashes absent from the frozen backing snapshot.
    pub finalization_backing_misses: u64,
    /// Finalized hashes whose backing lookup returned an error.
    pub finalization_lookup_errors: u64,
    /// Time spent collecting, sorting, and keying deferred full-state entries.
    pub deferred_finalization_prepare_us: u64,
    /// Time spent resolving deferred full-state entries from the backing snapshot.
    pub deferred_finalization_lookup_us: u64,
    /// Time spent validating and splitting serialized deferred full-state nodes.
    pub deferred_finalization_parse_us: u64,
    /// Time spent replaying deferred full-state reference operations.
    pub deferred_finalization_replay_us: u64,
    /// Time spent encoding deferred full-state nodes with final references.
    pub deferred_finalization_encode_us: u64,
    /// Process-attributed physical read bytes observed around deferred batch
    /// lookups. This is best-effort on Linux and remains zero when `/proc` is
    /// unavailable or cannot be parsed.
    pub deferred_finalization_read_bytes: u64,
    /// Process minor page faults observed around deferred batch lookups.
    pub deferred_finalization_minor_faults: u64,
    /// Process major page faults observed around deferred batch lookups.
    pub deferred_finalization_major_faults: u64,
}

/// Write-through cache mirroring the behaviour of the C# implementation.
///
/// Nodes are addressed by their hash and reference counted so that multiple
/// parents can point to the same subtree while it lives inside the cache.
pub struct MptCache<S>
where
    S: MptStoreSnapshot + 'static,
{
    store: Arc<S>,
    prefix: u8,
    // Keys are SHA-256 node identifiers, so they are already uniformly
    // distributed and cannot be chosen to create an FxHash collision set.
    entries: FxHashMap<UInt256, MptTrackable>,
    defer_reference_resolution: bool,
    defer_intermediate_nodes: bool,
    /// When set, [`MptCache::commit`] exports the deferred full-state journal
    /// unresolved (payload + journaled put count per hash) for the backing
    /// store to resolve at its write cursor, instead of probing the frozen
    /// backing snapshot for every journaled hash. Stores that cannot carry an
    /// unresolved journal transparently fall back to the classic flow.
    export_deferred_journal: bool,
    deferred_entries: FxHashMap<UInt256, DeferredReferenceEntry>,
    deferred_operations: Vec<DeferredReferenceOperationRecord>,
    mutation_stats: MptMutationStats,
}

impl<S> MptCache<S>
where
    S: MptStoreSnapshot + 'static,
{
    /// Creates a new cache backed by the given store snapshot with the specified key prefix.
    pub fn new(store: Arc<S>, prefix: u8) -> Self {
        Self::new_with_reference_mode(store, prefix, false, false)
    }

    pub(crate) fn new_deferred(store: Arc<S>, prefix: u8) -> Self {
        Self::new_with_reference_mode(store, prefix, true, false)
    }

    pub(crate) fn new_deferred_with_intermediate_nodes(store: Arc<S>, prefix: u8) -> Self {
        Self::new_with_reference_mode(store, prefix, true, true)
    }

    fn new_with_reference_mode(
        store: Arc<S>,
        prefix: u8,
        defer_reference_resolution: bool,
        defer_intermediate_nodes: bool,
    ) -> Self {
        Self {
            store,
            prefix,
            entries: FxHashMap::default(),
            defer_reference_resolution,
            defer_intermediate_nodes,
            export_deferred_journal: false,
            deferred_entries: FxHashMap::default(),
            deferred_operations: Vec::new(),
            mutation_stats: MptMutationStats::default(),
        }
    }

    #[cfg(test)]
    pub(crate) fn materialized_entry_count(&self) -> usize {
        self.entries
            .values()
            .filter(|entry| entry.node.is_some())
            .count()
    }

    fn append_deferred_operation(
        &mut self,
        hash: UInt256,
        operation: DeferredReferenceOperation,
    ) -> bool {
        let index = self.deferred_operations.len();
        self.deferred_operations
            .push(DeferredReferenceOperationRecord {
                operation,
                next: None,
            });
        match self.deferred_entries.entry(hash) {
            Entry::Occupied(mut entry) => {
                let previous = entry.get().last;
                self.deferred_operations[previous].next = Some(index);
                entry.get_mut().last = index;
                true
            }
            Entry::Vacant(entry) => {
                entry.insert(DeferredReferenceEntry {
                    first: index,
                    last: index,
                });
                false
            }
        }
    }

    fn replay_deferred_operations(
        &self,
        entry: DeferredReferenceEntry,
        base: Option<DeferredNodeState>,
    ) -> MptResult<Option<DeferredNodeState>> {
        let mut state = base;
        let mut current = Some(entry.first);
        while let Some(index) = current {
            let record = self.deferred_operations.get(index).ok_or_else(|| {
                MptError::invalid("deferred MPT reference operation index is out of range")
            })?;
            match &record.operation {
                DeferredReferenceOperation::Put {
                    node_type,
                    payload_without_reference,
                } => match state.as_mut() {
                    Some(state) => {
                        state.reference = state.reference.wrapping_add(1);
                    }
                    None => {
                        state = Some(DeferredNodeState {
                            node_type: *node_type,
                            reference: 1,
                            payload_without_reference: payload_without_reference.clone(),
                        });
                    }
                },
                DeferredReferenceOperation::Delete => match state.as_mut() {
                    Some(state) if state.reference > 1 => state.reference -= 1,
                    Some(_) => state = None,
                    None => {}
                },
            }
            current = record.next;
        }
        Ok(state)
    }

    fn classify_finalization_lookup(
        &mut self,
        hash: UInt256,
        lookup: MptStoreLookup<Node>,
    ) -> Option<Node> {
        let mut node = match lookup {
            MptStoreLookup::InMemory(node) => {
                if node.is_some() {
                    self.mutation_stats.finalization_memory_hits = self
                        .mutation_stats
                        .finalization_memory_hits
                        .saturating_add(1);
                } else {
                    self.mutation_stats.finalization_memory_misses = self
                        .mutation_stats
                        .finalization_memory_misses
                        .saturating_add(1);
                }
                node
            }
            MptStoreLookup::Backing(node) => {
                if node.is_some() {
                    self.mutation_stats.finalization_backing_hits = self
                        .mutation_stats
                        .finalization_backing_hits
                        .saturating_add(1);
                } else {
                    self.mutation_stats.finalization_backing_misses = self
                        .mutation_stats
                        .finalization_backing_misses
                        .saturating_add(1);
                }
                node
            }
        };
        if let Some(node) = node.as_mut() {
            node.set_accounted_hash(hash);
        }
        node
    }

    fn classify_finalization_raw_lookup(
        &mut self,
        lookup: MptStoreLookup<Vec<u8>>,
    ) -> MptResult<Option<DeferredNodeState>> {
        let bytes = match lookup {
            MptStoreLookup::InMemory(bytes) => {
                if bytes.is_some() {
                    self.mutation_stats.finalization_memory_hits = self
                        .mutation_stats
                        .finalization_memory_hits
                        .saturating_add(1);
                } else {
                    self.mutation_stats.finalization_memory_misses = self
                        .mutation_stats
                        .finalization_memory_misses
                        .saturating_add(1);
                }
                bytes
            }
            MptStoreLookup::Backing(bytes) => {
                if bytes.is_some() {
                    self.mutation_stats.finalization_backing_hits = self
                        .mutation_stats
                        .finalization_backing_hits
                        .saturating_add(1);
                } else {
                    self.mutation_stats.finalization_backing_misses = self
                        .mutation_stats
                        .finalization_backing_misses
                        .saturating_add(1);
                }
                bytes
            }
        };
        bytes
            .map(DeferredNodeState::from_serialized_owned)
            .transpose()
    }
}
