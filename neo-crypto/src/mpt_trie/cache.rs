use super::error::{MptError, MptResult};
use super::node::Node;
use super::node_type::NodeType;
use neo_io::{MemoryReader, Serializable};
use neo_primitives::UINT256_SIZE;
use neo_primitives::UInt256;
use rustc_hash::{FxHashMap, FxHashSet};
use std::collections::hash_map::Entry;
use std::fs;
use std::sync::Arc;
use std::time::Instant;

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

    /// Resolves the node identified by the supplied hash if present either in the
    /// in-memory cache or the underlying store.
    pub fn resolve(&mut self, hash: &UInt256) -> MptResult<Option<Node>> {
        let entry = self.resolve_internal(hash)?;
        let mut node = entry.resolve_clone()?;
        if let Some(node) = node.as_mut() {
            // `Node::clone` intentionally clears the materialized node's own
            // memoized hash like C#. Keep the cache lookup key separately as
            // pruning provenance without changing subsequent `hash()` calls.
            node.set_accounted_hash(*hash);
        }
        Ok(node)
    }

    /// Adds or updates the supplied node inside the cache.
    pub fn put_node(&mut self, mut node: Node) -> MptResult<()> {
        let payload_without_reference = node.to_array_without_reference()?;
        super::metrics::record_hash_computation();
        let hash_bytes = crate::Crypto::hash256(&payload_without_reference);
        let hash = UInt256::from_bytes(&hash_bytes).map_err(MptError::from)?;
        node.set_finalized_hash(hash);
        let node_type = node.node_type;
        self.put_node_with_payload(Some(node), node_type, hash, payload_without_reference)
    }

    /// Adds or updates the supplied node inside the cache while keeping the
    /// caller's node hash cached.
    pub(crate) fn put_node_cached(&mut self, node: &mut Node) -> MptResult<()> {
        let repeated_ancestor = matches!(
            node.node_type,
            NodeType::BranchNode | NodeType::ExtensionNode
        ) && node.cached_hash().is_some_and(|hash| {
            self.entries
                .get(&hash)
                .is_some_and(|entry| entry.produced_in_current_commit)
        });
        node.set_dirty();
        self.put_node_cached_inner(node, repeated_ancestor)
    }

    pub(crate) fn prepare_node_finalization(
        &mut self,
        node: &mut Node,
    ) -> MptResult<PendingNodeFinalization> {
        let payload_without_reference = node.to_array_without_reference()?;
        self.mutation_stats.put_node_cached_calls =
            self.mutation_stats.put_node_cached_calls.saturating_add(1);
        self.mutation_stats.serialized_payload_bytes = self
            .mutation_stats
            .serialized_payload_bytes
            .saturating_add(payload_without_reference.len() as u64);
        let hash = if let Some(hash) = node.cached_hash() {
            hash
        } else {
            super::metrics::record_hash_computation();
            let hash_bytes = crate::Crypto::hash256(&payload_without_reference);
            UInt256::from_bytes(&hash_bytes).map_err(MptError::from)?
        };
        node.set_pending_hash(hash);
        Ok(PendingNodeFinalization {
            node_type: node.node_type,
            hash,
            payload_without_reference,
        })
    }

    pub(crate) fn finalize_prepared_nodes(
        &mut self,
        pending: Vec<PendingNodeFinalization>,
    ) -> MptResult<()> {
        if self.defer_reference_resolution {
            return self.defer_prepared_nodes(pending);
        }

        let mut new_hashes = FxHashSet::default();
        new_hashes.reserve(pending.len());
        let mut missing_hashes = Vec::new();
        let mut cache_hits = 0u64;
        for node in &pending {
            if self.entries.contains_key(&node.hash) || !new_hashes.insert(node.hash) {
                cache_hits = cache_hits.saturating_add(1);
            } else {
                missing_hashes.push(node.hash);
            }
        }
        self.mutation_stats.finalization_cache_hits = self
            .mutation_stats
            .finalization_cache_hits
            .saturating_add(cache_hits);

        missing_hashes.sort_unstable_by_key(UInt256::to_array);
        let keys = missing_hashes
            .iter()
            .map(|hash| node_key_bytes(self.prefix, hash).to_vec())
            .collect::<Vec<_>>();
        let lookups = match self.store.try_get_nodes_with_source(&keys) {
            Ok(lookups) => lookups,
            Err(error) => {
                self.mutation_stats.finalization_lookup_errors = self
                    .mutation_stats
                    .finalization_lookup_errors
                    .saturating_add((pending.len() as u64).saturating_sub(cache_hits));
                return Err(error);
            }
        };
        if lookups.len() != missing_hashes.len() {
            self.mutation_stats.finalization_lookup_errors = self
                .mutation_stats
                .finalization_lookup_errors
                .saturating_add((pending.len() as u64).saturating_sub(cache_hits));
            return Err(MptError::storage(format!(
                "MPT batch lookup returned {} results for {} hashes",
                lookups.len(),
                missing_hashes.len()
            )));
        }

        let mut loaded = Vec::with_capacity(lookups.len());
        for (hash, lookup) in missing_hashes.into_iter().zip(lookups) {
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
            loaded.push((hash, MptTrackable::new(node)));
        }
        for (hash, entry) in loaded {
            self.entries.insert(hash, entry);
        }

        for node in pending {
            let entry = self
                .entries
                .get_mut(&node.hash)
                .ok_or_else(|| MptError::invalid("prefetched MPT finalization entry is missing"))?;
            Self::stage_payload(entry, node.node_type, node.payload_without_reference);
        }
        Ok(())
    }

    pub(crate) fn defer_intermediate_node(&mut self, node: &mut Node) -> MptResult<()> {
        let pending = self.prepare_node_finalization(node)?;
        let cache_hit = self.defer_prepared_node(pending);
        self.mutation_stats.finalization_cache_hits = self
            .mutation_stats
            .finalization_cache_hits
            .saturating_add(u64::from(cache_hit));
        Ok(())
    }

    pub(crate) const fn defers_intermediate_nodes(&self) -> bool {
        self.defer_intermediate_nodes
    }

    fn defer_prepared_nodes(&mut self, pending: Vec<PendingNodeFinalization>) -> MptResult<()> {
        let mut cache_hits = 0u64;
        for node in pending {
            cache_hits = cache_hits.saturating_add(u64::from(self.defer_prepared_node(node)));
        }
        self.mutation_stats.finalization_cache_hits = self
            .mutation_stats
            .finalization_cache_hits
            .saturating_add(cache_hits);
        Ok(())
    }

    fn defer_prepared_node(&mut self, node: PendingNodeFinalization) -> bool {
        if let Some(entry) = self.entries.get_mut(&node.hash) {
            Self::stage_payload(entry, node.node_type, node.payload_without_reference);
            return true;
        }

        self.append_deferred_operation(
            node.hash,
            DeferredReferenceOperation::Put {
                node_type: node.node_type,
                payload_without_reference: node.payload_without_reference,
            },
        )
    }

    fn put_node_cached_inner(&mut self, node: &mut Node, repeated_ancestor: bool) -> MptResult<()> {
        let payload_without_reference = node.to_array_without_reference()?;
        self.mutation_stats.put_node_cached_calls =
            self.mutation_stats.put_node_cached_calls.saturating_add(1);
        self.mutation_stats.serialized_payload_bytes = self
            .mutation_stats
            .serialized_payload_bytes
            .saturating_add(payload_without_reference.len() as u64);
        if repeated_ancestor {
            self.mutation_stats.repeated_ancestor_finalizations = self
                .mutation_stats
                .repeated_ancestor_finalizations
                .saturating_add(1);
        }
        let hash = if let Some(hash) = node.cached_hash() {
            hash
        } else {
            super::metrics::record_hash_computation();
            let hash_bytes = crate::Crypto::hash256(&payload_without_reference);
            UInt256::from_bytes(&hash_bytes).map_err(MptError::from)?
        };
        self.put_node_with_payload(None, node.node_type, hash, payload_without_reference)?;
        node.set_finalized_hash(hash);
        Ok(())
    }

    fn put_node_with_payload(
        &mut self,
        node: Option<Node>,
        node_type: NodeType,
        hash: UInt256,
        payload_without_reference: Vec<u8>,
    ) -> MptResult<()> {
        let entry = self.resolve_finalized(&hash)?;

        Self::stage_payload(entry, node_type, payload_without_reference);
        if let Some(mut node) = node {
            node.reference = entry.reference;
            entry.node = Some(node);
        }
        Ok(())
    }

    fn stage_payload(
        entry: &mut MptTrackable,
        node_type: NodeType,
        payload_without_reference: Vec<u8>,
    ) {
        if entry.node_type.is_some() {
            entry.reference = entry.reference.wrapping_add(1);
            entry.state = TrackState::Changed;
        } else {
            entry.reference = 1;
            entry.state = TrackState::Added;
        }
        entry.node_type = Some(node_type);
        if let Some(existing) = entry.node.as_mut() {
            existing.reference = entry.reference;
        }
        entry.payload_without_reference = Some(payload_without_reference);
        entry.produced_in_current_commit = true;
    }

    pub(crate) fn record_hash_computations(&mut self, count: u64) {
        self.mutation_stats.hash_computations =
            self.mutation_stats.hash_computations.saturating_add(count);
    }

    pub(crate) fn record_mutation_depth(&mut self, depth: usize) {
        self.mutation_stats.max_recursion_depth =
            self.mutation_stats.max_recursion_depth.max(depth as u64);
    }

    pub(crate) const fn mutation_stats(&self) -> MptMutationStats {
        self.mutation_stats
    }

    pub(crate) fn take_mutation_stats(&mut self) -> MptMutationStats {
        std::mem::take(&mut self.mutation_stats)
    }

    /// Decrements the reference count for the node or marks it for deletion when it
    /// is no longer referenced.
    pub fn delete_node(&mut self, hash: UInt256) -> MptResult<()> {
        if self.defer_reference_resolution && self.deferred_entries.contains_key(&hash) {
            self.append_deferred_operation(hash, DeferredReferenceOperation::Delete);
            return Ok(());
        }

        let entry = self.resolve_internal(&hash)?;
        if entry.node_type.is_none() {
            return Ok(());
        }
        if entry.reference > 1 {
            entry.reference -= 1;
            if entry.payload_without_reference.is_none() {
                let node = entry.node.as_ref().ok_or_else(|| {
                    MptError::invalid("cache entry cannot serialize without a materialized node")
                })?;
                entry.payload_without_reference = Some(node.to_array_without_reference()?);
            }
            if let Some(node) = entry.node.as_mut() {
                node.reference = entry.reference;
            }
            entry.state = TrackState::Changed;
        } else {
            entry.node = None;
            entry.node_type = None;
            entry.reference = 0;
            entry.payload_without_reference = None;
            entry.state = TrackState::Deleted;
        }
        Ok(())
    }

    pub(crate) fn checkpoint(&mut self) {
        for entry in self.entries.values_mut() {
            entry.produced_in_current_commit = false;
        }
    }

    /// Enables or disables unresolved deferred-journal export at commit time.
    ///
    /// Only meaningful for deferred full-state batch tries whose store stages
    /// unresolved journals (see
    /// [`MptStoreSnapshot::stage_unresolved_deferred_journal`]); every other
    /// configuration keeps the classic resolve-then-write flow.
    pub fn set_deferred_journal_export(&mut self, enabled: bool) {
        self.export_deferred_journal = enabled;
    }

    /// Flushes the pending changes to the underlying store.
    pub fn commit(&mut self) -> MptResult<()> {
        let mut overlay = Vec::with_capacity(
            self.entries
                .len()
                .saturating_add(self.deferred_entries.len()),
        );
        for (hash, entry) in &self.entries {
            match entry.state {
                TrackState::None => {}
                TrackState::Added | TrackState::Changed => {
                    let node_type = entry
                        .node_type
                        .ok_or_else(|| MptError::invalid("cache entry missing node type"))?;
                    let payload_without_reference =
                        entry.payload_without_reference.as_ref().ok_or_else(|| {
                            MptError::invalid("cache entry missing serialized node payload")
                        })?;
                    let data = Node::array_from_payload_parts(
                        node_type,
                        entry.reference,
                        payload_without_reference,
                    )?;
                    overlay.push((self.key(hash), Some(data)));
                }
                TrackState::Deleted => {
                    overlay.push((self.key(hash), None));
                }
            }
        }

        // Fused commit: when requested, hand the deferred full-state journal
        // to the store unresolved so the backing commit can resolve reference
        // counts at its write cursor. The materialized overlay above carries
        // no deferred entries, and its keys are disjoint from the journaled
        // hashes (a hash lives in either `entries` or `deferred_entries`,
        // never both), so cursor resolution observes exactly the base the
        // classic snapshot probe would have seen.
        let exported_journal = if self.export_deferred_journal {
            self.summarize_deferred_journal()
        } else {
            None
        };
        match exported_journal {
            Some(journal) if !journal.is_empty() => {
                // Byte parity rests on journal keys being disjoint from the
                // materialized overlay: the fused cursor resolves a journaled
                // hash against the pre-overlay base exactly because no overlay
                // entry can carry the same key.
                debug_assert!(
                    journal
                        .iter()
                        .all(|entry| !overlay.iter().any(|(key, _)| key == &entry.key)),
                    "deferred journal keys must be disjoint from the materialized overlay"
                );
                if self.store.stage_unresolved_deferred_journal(journal)? {
                    self.store.apply_overlay(overlay)?;
                } else {
                    // The store cannot carry an unresolved journal; resolve
                    // against the backing snapshot exactly like the classic
                    // path. The deferred journal is untouched, so the replay
                    // below sees the same operations it always would.
                    overlay.extend(self.deferred_overlay()?);
                    self.store.apply_overlay(overlay)?;
                }
            }
            _ => {
                overlay.extend(self.deferred_overlay()?);
                self.store.apply_overlay(overlay)?;
            }
        }
        self.entries.clear();
        self.deferred_entries.clear();
        self.deferred_operations.clear();
        Ok(())
    }

    /// Summarizes the deferred journal into per-hash put counts and first-put
    /// payloads, ordered by storage key, without consulting the backing store.
    ///
    /// Returns `None` when the journal contains a delete, which a put-count
    /// summary cannot represent. Full-state tries never record deletes
    /// (`Trie::previous_hash` yields `None` in full-state mode), so `None`
    /// signals an unexpected journal whose caller must fall back to the
    /// classic resolve-then-write path, which handles deletes.
    fn summarize_deferred_journal(&mut self) -> Option<Vec<UnresolvedDeferredNode>> {
        if self.deferred_entries.is_empty() {
            return Some(Vec::new());
        }

        let stage_start = Instant::now();
        let mut journal = Vec::with_capacity(self.deferred_entries.len());
        for (hash, entry) in &self.deferred_entries {
            let mut summary: Option<UnresolvedDeferredNode> = None;
            let mut current = Some(entry.first);
            while let Some(index) = current {
                let record = self.deferred_operations.get(index)?;
                match &record.operation {
                    DeferredReferenceOperation::Put {
                        node_type,
                        payload_without_reference,
                    } => match summary.as_mut() {
                        Some(summary) => {
                            summary.puts = summary.puts.checked_add(1)?;
                        }
                        None => {
                            summary = Some(UnresolvedDeferredNode {
                                key: Self::key_for(self.prefix, hash),
                                node_type: *node_type,
                                payload_without_reference: payload_without_reference.clone(),
                                puts: 1,
                            });
                        }
                    },
                    DeferredReferenceOperation::Delete => return None,
                }
                current = record.next;
            }
            journal.push(summary?);
        }
        journal.sort_unstable_by(|left, right| left.key.cmp(&right.key));
        self.mutation_stats.deferred_finalization_prepare_us = self
            .mutation_stats
            .deferred_finalization_prepare_us
            .saturating_add(elapsed_us(stage_start));
        Some(journal)
    }

    fn deferred_overlay(&mut self) -> MptResult<Vec<(Vec<u8>, Option<Vec<u8>>)>> {
        if self.deferred_entries.is_empty() {
            return Ok(Vec::new());
        }

        let stage_start = Instant::now();
        let mut pending = self
            .deferred_entries
            .iter()
            .map(|(hash, entry)| (*hash, *entry))
            .collect::<Vec<_>>();
        pending.sort_unstable_by_key(|(hash, _)| hash.to_array());
        let keys = pending
            .iter()
            .map(|(hash, _)| node_key_bytes(self.prefix, hash))
            .collect::<Vec<_>>();
        self.mutation_stats.deferred_finalization_prepare_us = self
            .mutation_stats
            .deferred_finalization_prepare_us
            .saturating_add(elapsed_us(stage_start));

        let stage_start = Instant::now();
        // The deferred path issues only a handful of sorted batches per
        // commit. Capture process I/O around the provider call itself so the
        // evidence does not add work to ordinary point resolution.
        let resources_before = process_resource_snapshot();
        let lookup_result = self.store.try_get_nodes_with_source_raw_borrowed(&keys);
        let resources_after = process_resource_snapshot();
        self.mutation_stats
            .record_deferred_resource_delta(resources_before, resources_after);
        let lookups = match lookup_result {
            Ok(lookups) => lookups,
            Err(error) => {
                self.mutation_stats.deferred_finalization_lookup_us = self
                    .mutation_stats
                    .deferred_finalization_lookup_us
                    .saturating_add(elapsed_us(stage_start));
                self.mutation_stats.finalization_lookup_errors = self
                    .mutation_stats
                    .finalization_lookup_errors
                    .saturating_add(pending.len() as u64);
                return Err(error);
            }
        };
        self.mutation_stats.deferred_finalization_lookup_us = self
            .mutation_stats
            .deferred_finalization_lookup_us
            .saturating_add(elapsed_us(stage_start));
        if lookups.len() != pending.len() {
            self.mutation_stats.finalization_lookup_errors = self
                .mutation_stats
                .finalization_lookup_errors
                .saturating_add(pending.len() as u64);
            return Err(MptError::storage(format!(
                "MPT deferred batch lookup returned {} results for {} hashes",
                lookups.len(),
                pending.len()
            )));
        }

        let mut overlay = Vec::with_capacity(pending.len());
        for ((hash, entry), lookup) in pending.into_iter().zip(lookups) {
            let stage_start = Instant::now();
            let base = self.classify_finalization_raw_lookup(lookup);
            self.mutation_stats.deferred_finalization_parse_us = self
                .mutation_stats
                .deferred_finalization_parse_us
                .saturating_add(elapsed_us(stage_start));
            let base = base?;

            let stage_start = Instant::now();
            let state = self.replay_deferred_operations(entry, base);
            self.mutation_stats.deferred_finalization_replay_us = self
                .mutation_stats
                .deferred_finalization_replay_us
                .saturating_add(elapsed_us(stage_start));
            let state = state?;

            let stage_start = Instant::now();
            let value = state.map(DeferredNodeState::into_bytes).transpose();
            self.mutation_stats.deferred_finalization_encode_us = self
                .mutation_stats
                .deferred_finalization_encode_us
                .saturating_add(elapsed_us(stage_start));
            let value = value?;
            overlay.push((Self::key_for(self.prefix, &hash), value));
        }
        Ok(overlay)
    }

    fn promote_deferred_entry(&mut self, hash: &UInt256) -> MptResult<()> {
        let Some(entry) = self.deferred_entries.get(hash).copied() else {
            return Ok(());
        };
        let key = node_key_bytes(self.prefix, hash);
        let lookup_started = Instant::now();
        let lookup = match self.store.try_get_node_with_source(&key) {
            Ok(lookup) => lookup,
            Err(error) => {
                self.mutation_stats.trie_resolve_store_ns = self
                    .mutation_stats
                    .trie_resolve_store_ns
                    .saturating_add(elapsed_ns(lookup_started));
                self.mutation_stats.finalization_lookup_errors = self
                    .mutation_stats
                    .finalization_lookup_errors
                    .saturating_add(1);
                return Err(error);
            }
        };
        let base = self.classify_finalization_lookup(*hash, lookup);
        self.mutation_stats.trie_resolve_store_ns = self
            .mutation_stats
            .trie_resolve_store_ns
            .saturating_add(elapsed_ns(lookup_started));
        if base.is_some() {
            self.mutation_stats.trie_resolve_store_hits = self
                .mutation_stats
                .trie_resolve_store_hits
                .saturating_add(1);
        } else {
            self.mutation_stats.trie_resolve_store_misses = self
                .mutation_stats
                .trie_resolve_store_misses
                .saturating_add(1);
        }
        let base_present = base.is_some();
        let base = base
            .map(|node| DeferredNodeState::from_node(&node))
            .transpose()?;
        let state = self.replay_deferred_operations(entry, base)?;
        let trackable = match state {
            Some(state) => {
                let node_type = state.node_type;
                let reference = state.reference;
                let payload_without_reference = state.payload_without_reference.clone();
                let node = state.into_node(*hash)?;
                MptTrackable {
                    node: Some(node),
                    node_type: Some(node_type),
                    reference,
                    payload_without_reference: Some(payload_without_reference),
                    state: if base_present {
                        TrackState::Changed
                    } else {
                        TrackState::Added
                    },
                    produced_in_current_commit: true,
                }
            }
            None => MptTrackable {
                node: None,
                node_type: None,
                reference: 0,
                payload_without_reference: None,
                state: if base_present {
                    TrackState::Deleted
                } else {
                    TrackState::None
                },
                produced_in_current_commit: true,
            },
        };
        self.deferred_entries.remove(hash);
        self.entries.insert(*hash, trackable);
        Ok(())
    }

    fn resolve_internal(&mut self, hash: &UInt256) -> MptResult<&mut MptTrackable> {
        if self.deferred_entries.contains_key(hash) {
            self.promote_deferred_entry(hash)?;
            return self.entries.get_mut(hash).ok_or_else(|| {
                MptError::invalid("promoted deferred MPT entry is missing from the cache")
            });
        }
        let store = self.store.as_ref();
        let prefix = self.prefix;
        let stats = &mut self.mutation_stats;

        match self.entries.entry(*hash) {
            Entry::Occupied(entry) => {
                stats.trie_resolve_cache_hits = stats.trie_resolve_cache_hits.saturating_add(1);
                Ok(entry.into_mut())
            }
            Entry::Vacant(entry) => {
                let lookup_started = Instant::now();
                let node = match Self::load_from_store_snapshot(store, prefix, hash) {
                    Ok(node) => node,
                    Err(error) => {
                        stats.trie_resolve_store_ns = stats
                            .trie_resolve_store_ns
                            .saturating_add(elapsed_ns(lookup_started));
                        return Err(error);
                    }
                };
                stats.trie_resolve_store_ns = stats
                    .trie_resolve_store_ns
                    .saturating_add(elapsed_ns(lookup_started));
                if node.is_some() {
                    stats.trie_resolve_store_hits = stats.trie_resolve_store_hits.saturating_add(1);
                } else {
                    stats.trie_resolve_store_misses =
                        stats.trie_resolve_store_misses.saturating_add(1);
                }
                Ok(entry.insert(MptTrackable::new(node)))
            }
        }
    }

    fn resolve_finalized(&mut self, hash: &UInt256) -> MptResult<&mut MptTrackable> {
        let store = self.store.as_ref();
        let prefix = self.prefix;
        let stats = &mut self.mutation_stats;

        match self.entries.entry(*hash) {
            Entry::Occupied(entry) => {
                stats.finalization_cache_hits = stats.finalization_cache_hits.saturating_add(1);
                Ok(entry.into_mut())
            }
            Entry::Vacant(entry) => {
                let key = node_key_bytes(prefix, hash);
                let lookup = match store.try_get_node_with_source(&key) {
                    Ok(lookup) => lookup,
                    Err(error) => {
                        stats.finalization_lookup_errors =
                            stats.finalization_lookup_errors.saturating_add(1);
                        return Err(error);
                    }
                };
                let mut node = match lookup {
                    MptStoreLookup::InMemory(node) => {
                        if node.is_some() {
                            stats.finalization_memory_hits =
                                stats.finalization_memory_hits.saturating_add(1);
                        } else {
                            stats.finalization_memory_misses =
                                stats.finalization_memory_misses.saturating_add(1);
                        }
                        node
                    }
                    MptStoreLookup::Backing(node) => {
                        if node.is_some() {
                            stats.finalization_backing_hits =
                                stats.finalization_backing_hits.saturating_add(1);
                        } else {
                            stats.finalization_backing_misses =
                                stats.finalization_backing_misses.saturating_add(1);
                        }
                        node
                    }
                };
                if let Some(node) = node.as_mut() {
                    node.set_accounted_hash(*hash);
                }
                Ok(entry.insert(MptTrackable::new(node)))
            }
        }
    }

    fn load_from_store_snapshot(store: &S, prefix: u8, hash: &UInt256) -> MptResult<Option<Node>> {
        let key = node_key_bytes(prefix, hash);
        let mut node = store.try_get_node(&key)?;
        if let Some(node) = node.as_mut() {
            node.set_accounted_hash(*hash);
        }
        Ok(node)
    }

    fn key(&self, hash: &UInt256) -> Vec<u8> {
        Self::key_for(self.prefix, hash)
    }

    fn key_for(prefix: u8, hash: &UInt256) -> Vec<u8> {
        node_key_bytes(prefix, hash).to_vec()
    }
}

fn elapsed_us(start: Instant) -> u64 {
    start.elapsed().as_micros().min(u64::MAX as u128) as u64
}

fn elapsed_ns(start: Instant) -> u64 {
    start.elapsed().as_nanos().min(u64::MAX as u128) as u64
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct ProcessResourceSnapshot {
    pub(crate) read_bytes: u64,
    pub(crate) minor_faults: u64,
    pub(crate) major_faults: u64,
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

impl MptMutationStats {
    pub(crate) fn record_deferred_resource_delta(
        &mut self,
        before: Option<ProcessResourceSnapshot>,
        after: Option<ProcessResourceSnapshot>,
    ) {
        let (Some(before), Some(after)) = (before, after) else {
            return;
        };
        let delta = after.delta_since(before);
        self.deferred_finalization_read_bytes = self
            .deferred_finalization_read_bytes
            .saturating_add(delta.read_bytes);
        self.deferred_finalization_minor_faults = self
            .deferred_finalization_minor_faults
            .saturating_add(delta.minor_faults);
        self.deferred_finalization_major_faults = self
            .deferred_finalization_major_faults
            .saturating_add(delta.major_faults);
    }
}

/// Best-effort Linux process resource counters. Other platforms and restricted
/// containers return `None`; callers must treat that as missing telemetry, not
/// as a storage or execution failure.
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

pub(crate) fn proc_io_counter(input: &str, name: &str) -> Option<u64> {
    input.lines().find_map(|line| {
        let (field, value) = line.split_once(':')?;
        (field == name).then(|| value.trim().parse().ok()).flatten()
    })
}

pub(crate) fn proc_stat_faults(input: &str) -> Option<(u64, u64)> {
    let mut fields = input.get(input.rfind(')')? + 1..)?.split_whitespace();
    // After the parenthesized process name, state is field 3 (index 0).
    let minor_faults = fields.nth(7)?.parse().ok()?;
    let major_faults = fields.nth(1)?.parse().ok()?;
    Some((minor_faults, major_faults))
}
