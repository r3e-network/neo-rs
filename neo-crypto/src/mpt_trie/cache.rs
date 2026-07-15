use super::error::{MptError, MptResult};
use super::node::Node;
use super::node_type::NodeType;
use neo_io::{MemoryReader, Serializable};
use neo_primitives::UINT256_SIZE;
use neo_primitives::UInt256;
use rustc_hash::{FxHashMap, FxHashSet};
use std::collections::hash_map::Entry;
use std::sync::Arc;

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

    /// Persists the serialized node for the supplied key.
    fn put(&self, key: Vec<u8>, value: Vec<u8>) -> MptResult<()>;

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
        Self::new_with_reference_mode(store, prefix, false)
    }

    pub(crate) fn new_deferred(store: Arc<S>, prefix: u8) -> Self {
        Self::new_with_reference_mode(store, prefix, true)
    }

    fn new_with_reference_mode(
        store: Arc<S>,
        prefix: u8,
        defer_reference_resolution: bool,
    ) -> Self {
        Self {
            store,
            prefix,
            entries: FxHashMap::default(),
            defer_reference_resolution,
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
        base: Option<Node>,
    ) -> MptResult<Option<DeferredNodeState>> {
        let mut state = base
            .as_ref()
            .map(DeferredNodeState::from_node)
            .transpose()?;
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
            .map(|hash| Self::key_bytes(self.prefix, hash).to_vec())
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

    fn defer_prepared_nodes(&mut self, pending: Vec<PendingNodeFinalization>) -> MptResult<()> {
        let mut cache_hits = 0u64;
        for node in pending {
            if let Some(entry) = self.entries.get_mut(&node.hash) {
                cache_hits = cache_hits.saturating_add(1);
                Self::stage_payload(entry, node.node_type, node.payload_without_reference);
                continue;
            }

            let existed = self.append_deferred_operation(
                node.hash,
                DeferredReferenceOperation::Put {
                    node_type: node.node_type,
                    payload_without_reference: node.payload_without_reference,
                },
            );
            if existed {
                cache_hits = cache_hits.saturating_add(1);
            }
        }
        self.mutation_stats.finalization_cache_hits = self
            .mutation_stats
            .finalization_cache_hits
            .saturating_add(cache_hits);
        Ok(())
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
        overlay.extend(self.deferred_overlay()?);
        self.store.apply_overlay(overlay)?;
        self.entries.clear();
        self.deferred_entries.clear();
        self.deferred_operations.clear();
        Ok(())
    }

    fn deferred_overlay(&mut self) -> MptResult<Vec<(Vec<u8>, Option<Vec<u8>>)>> {
        if self.deferred_entries.is_empty() {
            return Ok(Vec::new());
        }

        let mut pending = self
            .deferred_entries
            .iter()
            .map(|(hash, entry)| (*hash, *entry))
            .collect::<Vec<_>>();
        pending.sort_unstable_by_key(|(hash, _)| hash.to_array());
        let keys = pending
            .iter()
            .map(|(hash, _)| Self::key_bytes(self.prefix, hash).to_vec())
            .collect::<Vec<_>>();
        let lookups = match self.store.try_get_nodes_with_source(&keys) {
            Ok(lookups) => lookups,
            Err(error) => {
                self.mutation_stats.finalization_lookup_errors = self
                    .mutation_stats
                    .finalization_lookup_errors
                    .saturating_add(pending.len() as u64);
                return Err(error);
            }
        };
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
            let base = self.classify_finalization_lookup(hash, lookup);
            let state = self.replay_deferred_operations(entry, base)?;
            let value = state.map(|state| state.to_bytes()).transpose()?;
            overlay.push((Self::key_for(self.prefix, &hash), value));
        }
        Ok(overlay)
    }

    fn promote_deferred_entry(&mut self, hash: &UInt256) -> MptResult<()> {
        let Some(entry) = self.deferred_entries.get(hash).copied() else {
            return Ok(());
        };
        let key = Self::key_bytes(self.prefix, hash);
        let lookup = match self.store.try_get_node_with_source(&key) {
            Ok(lookup) => lookup,
            Err(error) => {
                self.mutation_stats.finalization_lookup_errors = self
                    .mutation_stats
                    .finalization_lookup_errors
                    .saturating_add(1);
                return Err(error);
            }
        };
        let base = self.classify_finalization_lookup(*hash, lookup);
        let base_present = base.is_some();
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
        }
        let store = self.store.as_ref();
        let prefix = self.prefix;

        match self.entries.entry(*hash) {
            Entry::Occupied(entry) => Ok(entry.into_mut()),
            Entry::Vacant(entry) => {
                let node = Self::load_from_store_snapshot(store, prefix, hash)?;
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
                let key = Self::key_bytes(prefix, hash);
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
        let key = Self::key_bytes(prefix, hash);
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
        Self::key_bytes(prefix, hash).to_vec()
    }

    fn key_bytes(prefix: u8, hash: &UInt256) -> [u8; 1 + UINT256_SIZE] {
        let mut buffer = [0u8; 1 + UINT256_SIZE];
        buffer[0] = prefix;
        buffer[1..].copy_from_slice(&hash.to_array());
        buffer
    }
}
