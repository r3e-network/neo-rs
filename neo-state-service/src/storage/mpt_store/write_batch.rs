//! Mutable overlay used while an ordered StateService MPT batch is prepared.

use super::MptOverlay;

use neo_crypto::mpt_trie::{MptError, MptResult, MptStoreLookup, MptStoreSnapshot, Node};
use neo_io::{MemoryReader, Serializable};
use neo_primitives::UINT256_SIZE;
use neo_storage::persistence::{RawReadOnlyStore, Store};
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
    pub(super) overlay: Mutex<MptOverlay>,
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
        overlay_capacity: usize,
    ) -> Self {
        Self {
            base,
            backing_snapshot,
            overlay: Mutex::new(MptOverlay::with_capacity_and_hasher(
                overlay_capacity,
                Default::default(),
            )),
            absent_from_base: Mutex::new(HashSet::with_capacity(overlay_capacity)),
            overlay_has_entries: AtomicBool::new(false),
        }
    }

    pub(super) fn overlay_contains_entries(&self) -> bool {
        self.overlay_has_entries.load(Ordering::Acquire)
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
        if let Some(value) = self.base.get(key) {
            return Ok(MptStoreLookup::InMemory(value.clone()));
        }
        // Negative cache: a prior miss against this frozen base must not re-hit
        // durable storage. Proven-absent keys are cleared when a put stages them.
        if Self::node_key(key).is_some_and(|node_key| self.absent_from_base.lock().contains(&node_key))
        {
            return Ok(MptStoreLookup::InMemory(None));
        }

        let Some(backing_snapshot) = self.backing_snapshot.as_ref() else {
            if let Some(node_key) = Self::node_key(key) {
                self.absent_from_base.lock().insert(node_key);
            }
            return Ok(MptStoreLookup::InMemory(None));
        };
        let value = backing_snapshot
            .try_get_bytes_result(key)
            .map_err(|error| {
                MptError::storage(format!("MPT backing snapshot read failed: {error}"))
            })?;
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
        let mut lookups = Vec::with_capacity(keys.len());
        lookups.resize_with(keys.len(), || None);
        let mut backing_indices = Vec::new();
        let mut proven_absent = Vec::new();

        let staged = self.overlay_contains_entries().then(|| self.overlay.lock());
        let absent = self.absent_from_base.lock();
        for (index, key) in keys.iter().enumerate() {
            if let Some(value) = staged.as_ref().and_then(|overlay| overlay.get(key)) {
                lookups[index] = Some(MptStoreLookup::InMemory(value.clone()));
            } else if let Some(value) = self.base.get(key) {
                lookups[index] = Some(MptStoreLookup::InMemory(value.clone()));
            } else if Self::node_key(key).is_some_and(|node_key| absent.contains(&node_key)) {
                // Reuse a prior durable miss without another MDBX round-trip.
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

        if !backing_indices.is_empty() {
            let backing = self
                .backing_snapshot
                .as_ref()
                .ok_or_else(|| MptError::storage("MPT backing snapshot disappeared"))?;
            let backing_keys = backing_indices
                .iter()
                .map(|index| keys[*index].as_slice())
                .collect::<Vec<_>>();
            let values = backing.try_get_many_bytes(&backing_keys).map_err(|error| {
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

        let decoded =
            lookups
                .into_iter()
                .map(|lookup| {
                    Self::decode_node_lookup(lookup.ok_or_else(|| {
                        MptError::storage("MPT batch lookup omitted an input key")
                    })?)
                })
                .collect::<MptResult<Vec<_>>>()?;

        if !proven_absent.is_empty() {
            let mut absent_from_base = self.absent_from_base.lock();
            for index in proven_absent {
                if let Some(node_key) = Self::node_key(&keys[index]) {
                    absent_from_base.insert(node_key);
                }
            }
        }
        Ok(decoded)
    }

    fn put(&self, key: Vec<u8>, value: Vec<u8>) -> MptResult<()> {
        self.stage(key, Some(value));
        Ok(())
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
