//! Mutable overlay used while an ordered StateService MPT batch is prepared.

use super::{MptNodeReadSnapshot, MptOverlay, is_mpt_node_key};

use neo_io::{MemoryReader, Serializable};
use neo_primitives::UINT256_SIZE;
use neo_storage::persistence::{RawReadOnlyStore, Store};
use neo_trie::{
    MptError, MptResult, MptStoreLookup, MptStoreSnapshot, Node, UnresolvedDeferredNode,
};
use parking_lot::Mutex;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

pub(super) const MPT_NODE_KEY_SIZE: usize = 1 + UINT256_SIZE;
type MptNodeKey = [u8; MPT_NODE_KEY_SIZE];

/// Private write-staging view used by the MPT store.
///
/// Reads come from the frozen base generation and writes are buffered until
/// the containing Ledger and StateService batch can commit atomically.
pub(super) struct MptWriteBatch<S: Store> {
    base: Arc<HashMap<Vec<u8>, Option<Vec<u8>>>>,
    backing_snapshot: Option<Arc<S::Snapshot>>,
    node_snapshot: Option<Arc<dyn MptNodeReadSnapshot>>,
    pub(super) overlay: Mutex<MptOverlay>,
    /// Unresolved deferred full-state journal staged by the trie cache for
    /// the coordinated backing commit to resolve at its write cursor.
    pub(super) deferred_journal: Mutex<Vec<UnresolvedDeferredNode>>,
    /// Keys proven absent from the durable base before their first batch put.
    absent_from_base: Mutex<HashSet<MptNodeKey>>,
    overlay_has_entries: AtomicBool,
}

impl<S> MptWriteBatch<S>
where
    S: Store,
{
    pub(super) fn new(
        base: Arc<HashMap<Vec<u8>, Option<Vec<u8>>>>,
        backing_snapshot: Option<Arc<S::Snapshot>>,
        node_snapshot: Option<Arc<dyn MptNodeReadSnapshot>>,
        overlay_capacity: usize,
    ) -> Self {
        Self {
            base,
            backing_snapshot,
            node_snapshot,
            overlay: Mutex::new(MptOverlay::with_capacity_and_hasher(
                overlay_capacity,
                Default::default(),
            )),
            deferred_journal: Mutex::new(Vec::new()),
            absent_from_base: Mutex::new(HashSet::with_capacity(overlay_capacity)),
            overlay_has_entries: AtomicBool::new(false),
        }
    }

    pub(super) fn overlay_contains_entries(&self) -> bool {
        self.overlay_has_entries.load(Ordering::Acquire)
    }

    /// Clones the exact authoritative node generation used while this batch
    /// was prepared. Deferred split-store publication must resolve its journal
    /// against this generation rather than pinning a newer one at commit time.
    pub(super) fn pinned_node_snapshot(&self) -> Option<Arc<dyn MptNodeReadSnapshot>> {
        self.node_snapshot.clone()
    }

    fn mark_overlay_non_empty(&self) {
        self.overlay_has_entries.store(true, Ordering::Release);
    }

    fn node_key(key: &[u8]) -> Option<MptNodeKey> {
        key.try_into().ok()
    }

    fn stage(&self, key: Vec<u8>, value: Option<Vec<u8>>) {
        if value.is_none()
            && Self::node_key(&key)
                .is_some_and(|node_key| self.absent_from_base.lock().remove(&node_key))
        {
            self.overlay.lock().remove(&key);
            return;
        }

        self.mark_overlay_non_empty();
        self.overlay.lock().insert(key, value);
    }

    fn try_get_with_source_inner(&self, key: &[u8]) -> MptResult<MptStoreLookup<Vec<u8>>> {
        if self.overlay_contains_entries()
            && let Some(staged) = self.overlay.lock().get(key)
        {
            return Ok(MptStoreLookup::InMemory(staged.clone()));
        }
        let authoritative_node = is_mpt_node_key(key) && self.node_snapshot.is_some();
        if !authoritative_node && let Some(value) = self.base.get(key) {
            return Ok(MptStoreLookup::InMemory(value.clone()));
        }
        // Negative cache: a prior miss against this frozen base must not re-hit
        // durable storage. Proven-absent keys are cleared when a put stages them.
        if Self::node_key(key)
            .is_some_and(|node_key| self.absent_from_base.lock().contains(&node_key))
        {
            return Ok(MptStoreLookup::InMemory(None));
        }

        let value = if is_mpt_node_key(key)
            && let Some(node_snapshot) = self.node_snapshot.as_ref()
        {
            node_snapshot.try_get_node_bytes(key).map_err(|error| {
                MptError::storage(format!(
                    "MPT authoritative node snapshot read failed: {error}"
                ))
            })?
        } else if let Some(backing_snapshot) = self.backing_snapshot.as_ref() {
            backing_snapshot
                .try_get_bytes_result(key)
                .map_err(|error| {
                    MptError::storage(format!("MPT backing snapshot read failed: {error}"))
                })?
        } else {
            if let Some(node_key) = Self::node_key(key) {
                self.absent_from_base.lock().insert(node_key);
            }
            return Ok(MptStoreLookup::InMemory(None));
        };
        if value.is_none() {
            if let Some(node_key) = Self::node_key(key) {
                self.absent_from_base.lock().insert(node_key);
            }
        }
        Ok(MptStoreLookup::Backing(value))
    }

    fn decode_node_lookup(lookup: MptStoreLookup<Vec<u8>>) -> MptResult<MptStoreLookup<Node>> {
        fn decode(bytes: Option<Vec<u8>>) -> MptResult<Option<Node>> {
            let Some(bytes) = bytes else {
                return Ok(None);
            };
            let mut reader = MemoryReader::new(&bytes);
            Node::deserialize(&mut reader)
                .map(Some)
                .map_err(MptError::from)
        }

        match lookup {
            MptStoreLookup::InMemory(bytes) => Ok(MptStoreLookup::InMemory(decode(bytes)?)),
            MptStoreLookup::Backing(bytes) => Ok(MptStoreLookup::Backing(decode(bytes)?)),
        }
    }
}

impl<S> MptWriteBatch<S>
where
    S: Store,
{
    fn try_get_nodes_with_source_inner<K>(
        &self,
        keys: &[K],
        sorted: bool,
    ) -> MptResult<Vec<MptStoreLookup<Vec<u8>>>>
    where
        K: AsRef<[u8]>,
    {
        let mut lookups = Vec::with_capacity(keys.len());
        lookups.resize_with(keys.len(), || None);
        let mut node_indices = Vec::new();
        let mut backing_indices = Vec::new();
        let mut proven_absent = Vec::new();

        let staged = self.overlay_contains_entries().then(|| self.overlay.lock());
        let absent = self.absent_from_base.lock();
        for (index, key) in keys.iter().enumerate() {
            let key = key.as_ref();
            let authoritative_node = is_mpt_node_key(key) && self.node_snapshot.is_some();
            if let Some(value) = staged.as_ref().and_then(|overlay| overlay.get(key)) {
                lookups[index] = Some(MptStoreLookup::InMemory(value.clone()));
            } else if authoritative_node
                && Self::node_key(key).is_some_and(|node_key| absent.contains(&node_key))
            {
                // Reuse a prior durable miss without another MDBX round-trip.
                lookups[index] = Some(MptStoreLookup::InMemory(None));
            } else if authoritative_node {
                node_indices.push(index);
            } else if let Some(value) = self.base.get(key) {
                lookups[index] = Some(MptStoreLookup::InMemory(value.clone()));
            } else if Self::node_key(key).is_some_and(|node_key| absent.contains(&node_key)) {
                lookups[index] = Some(MptStoreLookup::InMemory(None));
            } else if self.backing_snapshot.is_some() {
                backing_indices.push(index);
            } else {
                lookups[index] = Some(MptStoreLookup::InMemory(None));
                proven_absent.push(index);
            }
        }
        drop(absent);
        drop(staged);

        if !node_indices.is_empty() {
            let node_snapshot = self
                .node_snapshot
                .as_ref()
                .ok_or_else(|| MptError::storage("MPT authoritative node snapshot disappeared"))?;
            let node_keys = node_indices
                .iter()
                .map(|index| keys[*index].as_ref())
                .collect::<Vec<_>>();
            let values = if sorted {
                node_snapshot.try_get_node_bytes_sorted(&node_keys)
            } else {
                node_keys
                    .iter()
                    .map(|key| node_snapshot.try_get_node_bytes(key))
                    .collect()
            }
            .map_err(|error| {
                MptError::storage(format!(
                    "MPT authoritative node snapshot batch read failed: {error}"
                ))
            })?;
            if values.len() != node_indices.len() {
                return Err(MptError::storage(format!(
                    "MPT authoritative node batch returned {} results for {} keys",
                    values.len(),
                    node_indices.len()
                )));
            }
            for (index, value) in node_indices.into_iter().zip(values) {
                if value.is_none() {
                    proven_absent.push(index);
                }
                lookups[index] = Some(MptStoreLookup::Backing(value));
            }
        }

        if !backing_indices.is_empty() {
            let backing = self
                .backing_snapshot
                .as_ref()
                .ok_or_else(|| MptError::storage("MPT backing snapshot disappeared"))?;
            let backing_keys = backing_indices
                .iter()
                .map(|index| keys[*index].as_ref())
                .collect::<Vec<_>>();
            let values = if sorted {
                // The finalizer writes the resolved overlay immediately after
                // this frozen read. Ask durable backends for the write-intent
                // path so negative probes warm the B-tree pages used by the
                // following cursor writes.
                backing.try_get_many_bytes_sorted_for_write(&backing_keys)
            } else {
                backing.try_get_many_bytes(&backing_keys)
            }
            .map_err(|error| {
                MptError::storage(format!("MPT backing snapshot batch read failed: {error}"))
            })?;
            if values.len() != backing_indices.len() {
                return Err(MptError::storage(format!(
                    "MPT backing batch returned {} results for {} keys",
                    values.len(),
                    backing_indices.len()
                )));
            }
            for (index, value) in backing_indices.into_iter().zip(values) {
                if value.is_none() {
                    proven_absent.push(index);
                }
                lookups[index] = Some(MptStoreLookup::Backing(value));
            }
        }

        if !proven_absent.is_empty() {
            let mut absent_from_base = self.absent_from_base.lock();
            for index in proven_absent {
                if let Some(node_key) = Self::node_key(keys[index].as_ref()) {
                    absent_from_base.insert(node_key);
                }
            }
        }
        lookups
            .into_iter()
            .map(|lookup| {
                lookup.ok_or_else(|| MptError::storage("MPT batch lookup omitted an input key"))
            })
            .collect()
    }

    fn try_get_nodes_with_source_nodes_inner<K>(
        &self,
        keys: &[K],
        sorted: bool,
    ) -> MptResult<Vec<MptStoreLookup<Node>>>
    where
        K: AsRef<[u8]>,
    {
        self.try_get_nodes_with_source_inner(keys, sorted)?
            .into_iter()
            .map(Self::decode_node_lookup)
            .collect()
    }
}

impl<S> MptStoreSnapshot for MptWriteBatch<S>
where
    S: Store,
{
    fn try_get(&self, key: &[u8]) -> MptResult<Option<Vec<u8>>> {
        let value = match self.try_get_with_source_inner(key)? {
            MptStoreLookup::InMemory(value) | MptStoreLookup::Backing(value) => value,
        };
        Ok(value)
    }

    fn try_get_node_with_source(&self, key: &[u8]) -> MptResult<MptStoreLookup<Node>> {
        Self::decode_node_lookup(self.try_get_with_source_inner(key)?)
    }

    fn try_get_nodes_with_source(&self, keys: &[Vec<u8>]) -> MptResult<Vec<MptStoreLookup<Node>>> {
        self.try_get_nodes_with_source_nodes_inner(keys, false)
    }

    fn try_get_nodes_with_source_borrowed<K>(
        &self,
        keys: &[K],
    ) -> MptResult<Vec<MptStoreLookup<Node>>>
    where
        K: AsRef<[u8]>,
    {
        self.try_get_nodes_with_source_nodes_inner(keys, true)
    }

    fn try_get_nodes_with_source_raw_borrowed<K>(
        &self,
        keys: &[K],
    ) -> MptResult<Vec<MptStoreLookup<Vec<u8>>>>
    where
        K: AsRef<[u8]>,
    {
        self.try_get_nodes_with_source_inner(keys, true)
    }

    fn put(&self, key: Vec<u8>, value: Vec<u8>) -> MptResult<()> {
        self.stage(key, Some(value));
        Ok(())
    }

    fn stage_unresolved_deferred_journal(
        &self,
        journal: Vec<UnresolvedDeferredNode>,
    ) -> MptResult<bool> {
        *self.deferred_journal.lock() = journal;
        Ok(true)
    }

    fn delete(&self, key: Vec<u8>) -> MptResult<()> {
        self.stage(key, None);
        Ok(())
    }

    fn apply_overlay(&self, overlay: Vec<(Vec<u8>, Option<Vec<u8>>)>) -> MptResult<()> {
        if overlay.is_empty() {
            return Ok(());
        }

        let mut absent_from_base = self.absent_from_base.lock();
        let mut staged = self.overlay.lock();
        for (key, value) in overlay {
            if value.is_none()
                && Self::node_key(&key).is_some_and(|node_key| absent_from_base.remove(&node_key))
            {
                staged.remove(&key);
            } else {
                staged.insert(key, value);
            }
        }
        if !staged.is_empty() {
            self.mark_overlay_non_empty();
        }
        Ok(())
    }
}
