//! In-memory [`StateStoreBackend`] implementation for testing.

use super::backend::StateStoreBackend;
use parking_lot::RwLock;
use std::collections::HashMap;

/// In-memory implementation of StateStoreBackend for testing.
#[derive(Default)]
pub struct MemoryStateStoreBackend {
    data: RwLock<HashMap<Vec<u8>, Vec<u8>>>,
    pending: RwLock<HashMap<Vec<u8>, Option<Vec<u8>>>>,
}

impl MemoryStateStoreBackend {
    pub fn new() -> Self {
        Self::default()
    }
}

impl StateStoreBackend for MemoryStateStoreBackend {
    fn try_get(&self, key: &[u8]) -> Option<Vec<u8>> {
        // Check pending first
        if let Some(pending_value) = self.pending.read().get(key).cloned() {
            return pending_value;
        }
        // Then check committed data
        self.data.read().get(key).cloned()
    }

    fn put(&self, key: Vec<u8>, value: Vec<u8>) {
        self.pending.write().insert(key, Some(value));
    }

    fn delete(&self, key: &[u8]) {
        self.pending.write().insert(key.to_vec(), None);
    }

    fn commit(&self) -> Result<(), String> {
        let mut data = self.data.write();
        let mut pending = self.pending.write();
        for (key, value) in pending.drain() {
            match value {
                Some(v) => {
                    data.insert(key, v);
                }
                None => {
                    data.remove(&key);
                }
            }
        }
        Ok(())
    }
}
