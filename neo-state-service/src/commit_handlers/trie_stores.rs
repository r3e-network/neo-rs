use super::*;
/// Persistent MPT node reader used for historical proofs.
pub(crate) struct TrieNodeStore {
    pub(super) store_cache: Arc<StoreCache>,
}

impl TrieNodeStore {
    /// Maps an MPT-internal node key onto its namespaced storage key.
    pub(super) fn namespaced_key(mpt_key: &[u8]) -> StorageKey {
        let mut key = Vec::with_capacity(1 + mpt_key.len());
        key.push(PREFIX_TRIE_NODE);
        key.extend_from_slice(mpt_key);
        StorageKey::new(STATE_ROOT_STORAGE_ID, key)
    }
}

impl MptStoreSnapshot for TrieNodeStore {
    fn try_get(&self, key: &[u8]) -> MptResult<Option<Vec<u8>>> {
        Ok(self
            .store_cache
            .get(&Self::namespaced_key(key))
            .map(|item| item.value().to_vec()))
    }

    fn put(&self, key: Vec<u8>, value: Vec<u8>) -> MptResult<()> {
        let storage_key = Self::namespaced_key(&key);
        let item = StorageItem::from_bytes(value);
        let result = if self.store_cache.get(&storage_key).is_some() {
            self.store_cache.data_cache().try_update(storage_key, item)
        } else {
            self.store_cache.data_cache().try_add(storage_key, item)
        };
        result.map_err(|e| MptError::storage(format!("failed to persist MPT node: {e}")))
    }

    fn delete(&self, key: Vec<u8>) -> MptResult<()> {
        let storage_key = Self::namespaced_key(&key);
        if self.store_cache.get(&storage_key).is_some() {
            self.store_cache
                .data_cache()
                .try_delete(&storage_key)
                .map_err(|e| MptError::storage(format!("failed to delete MPT node: {e}")))?;
        }
        Ok(())
    }
}

/// Block-scoped MPT writes. All new nodes are staged in the exact snapshot
/// committed by the ledger; previously committed nodes remain readable through
/// the persistent store. No independent cache write can survive a block abort.
pub(super) struct BlockTrieNodeStore {
    pub(super) persisted: Arc<TrieNodeStore>,
    pub(super) snapshot: PersistenceDataCache,
}

impl BlockTrieNodeStore {
    fn get_item(&self, key: &StorageKey) -> Option<StorageItem> {
        let item = self.snapshot.get(key);
        if item.is_some() || self.snapshot.has_backing_store() {
            item
        } else {
            // Detached test/embedder snapshots have no persistent read view.
            self.persisted.store_cache.get(key)
        }
    }
}

impl MptStoreSnapshot for BlockTrieNodeStore {
    fn try_get(&self, key: &[u8]) -> MptResult<Option<Vec<u8>>> {
        let storage_key = TrieNodeStore::namespaced_key(key);
        Ok(self
            .get_item(&storage_key)
            .map(|item| item.value().to_vec()))
    }

    fn put(&self, key: Vec<u8>, value: Vec<u8>) -> MptResult<()> {
        let storage_key = TrieNodeStore::namespaced_key(&key);
        let item = StorageItem::from_bytes(value);
        let result = if self.get_item(&storage_key).is_some() {
            self.snapshot.try_update(storage_key, item)
        } else {
            self.snapshot.try_add(storage_key, item)
        };
        result.map_err(|e| MptError::storage(format!("failed to stage MPT node: {e}")))
    }

    fn put_with_hint(
        &self,
        key: Vec<u8>,
        value: Vec<u8>,
        known_absent_in_store: bool,
    ) -> MptResult<()> {
        if !known_absent_in_store {
            return self.put(key, value);
        }

        // MptCache established absence while resolving this node. Keep the
        // snapshot's own state machine fail-closed: try_add still rejects an
        // unexpected Added/Changed/None entry instead of silently upserting it.
        self.snapshot
            .try_add(
                TrieNodeStore::namespaced_key(&key),
                StorageItem::from_bytes(value),
            )
            .map_err(|e| MptError::storage(format!("failed to stage new MPT node: {e}")))
    }

    fn put_full_state(&self, key: Vec<u8>, value: Vec<u8>) -> MptResult<()> {
        // Full-state tries never use reference counts to delete history. Stage
        // the content-addressed body directly instead of reading the previous
        // reference count from RocksDB merely to increment an unused field.
        self.snapshot
            .try_update(
                TrieNodeStore::namespaced_key(&key),
                StorageItem::from_bytes(value),
            )
            .map_err(|e| MptError::storage(format!("failed to stage full-state MPT node: {e}")))
    }

    fn delete(&self, key: Vec<u8>) -> MptResult<()> {
        let storage_key = TrieNodeStore::namespaced_key(&key);
        if self.get_item(&storage_key).is_some() {
            self.snapshot
                .try_delete(&storage_key)
                .map_err(|e| MptError::storage(format!("failed to stage MPT deletion: {e}")))?;
        }
        Ok(())
    }
}

/// Maximum compact MPT nodes retained by each state-service store.
///
/// This is deliberately store-scoped rather than process-global: independent
/// chains cannot share entries, and a restart always begins with a cold cache.
pub(super) const STATE_ROOT_NODE_CACHE_CAPACITY: usize = 65_536;

/// Observation-only snapshot of a state-service store's compact-node cache.
///
/// `Default` is the zero snapshot a caller uses when the state service is not
/// registered; `enabled` distinguishes that from a registered but idle cache.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct StateRootNodeCacheStats {
    pub enabled: bool,
    pub capacity: usize,
    pub entries: usize,
    pub hits: u64,
    pub misses: u64,
    pub hit_rate_percent: f32,
    /// Resolved nodes reachable from the committed warm root, excluding hash stubs.
    pub warm_root_nodes: usize,
    /// Owned-node and buffer estimate; excludes allocator, Arc and lock overhead.
    pub warm_root_estimated_bytes: usize,
}
