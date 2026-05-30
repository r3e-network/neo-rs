//! MPT-trie snapshot adapter over a [`StateStoreBackend`].

use super::backend::StateStoreBackend;
use neo_crypto::mpt_trie::{MptResult, MptStoreSnapshot};
use std::sync::Arc;

/// Adapter to make our store work with the MPT Trie's `MptStoreSnapshot` trait.
pub struct StateStoreSnapshot {
    store: Arc<dyn StateStoreBackend>,
}

impl StateStoreSnapshot {
    pub fn new(store: Arc<dyn StateStoreBackend>) -> Self {
        Self { store }
    }
}

impl MptStoreSnapshot for StateStoreSnapshot {
    fn try_get(&self, key: &[u8]) -> MptResult<Option<Vec<u8>>> {
        Ok(self.store.try_get(key))
    }

    fn put(&self, key: Vec<u8>, value: Vec<u8>) -> MptResult<()> {
        self.store.put(key, value);
        Ok(())
    }

    fn delete(&self, key: Vec<u8>) -> MptResult<()> {
        self.store.delete(&key);
        Ok(())
    }
}
