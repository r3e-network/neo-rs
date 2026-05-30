//! [`StateStoreBackend`] that persists through the core `Store` via snapshots.

use super::backend::StateStoreBackend;
use crate::persistence::store::Store;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::Arc;

/// Snapshot-backed backend that persists through the core `Store`.
pub struct SnapshotBackedStateStoreBackend {
    store: Arc<dyn Store>,
    pending: Mutex<HashMap<Vec<u8>, Option<Vec<u8>>>>,
}

impl SnapshotBackedStateStoreBackend {
    pub fn new(store: Arc<dyn Store>) -> Self {
        Self {
            store,
            pending: Mutex::new(HashMap::new()),
        }
    }
}

impl StateStoreBackend for SnapshotBackedStateStoreBackend {
    fn try_get(&self, key: &[u8]) -> Option<Vec<u8>> {
        // Pending writes take precedence
        if let Some(value) = self.pending.lock().get(key).cloned() {
            return value;
        }

        let snapshot = self.store.snapshot();
        snapshot.try_get(&key.to_vec())
    }

    fn put(&self, key: Vec<u8>, value: Vec<u8>) {
        self.pending.lock().insert(key, Some(value));
    }

    fn delete(&self, key: &[u8]) {
        self.pending.lock().insert(key.to_vec(), None);
    }

    fn commit(&self) -> Result<(), String> {
        let mut pending = self.pending.lock();
        if pending.is_empty() {
            return Ok(());
        }

        let mut snapshot = self.store.snapshot();
        let Some(snapshot_mut) = Arc::get_mut(&mut snapshot) else {
            return Err("state service commit aborted: snapshot has additional references".into());
        };

        for (key, value) in pending.iter() {
            let result = match value {
                Some(v) => snapshot_mut.put(key.clone(), v.clone()),
                None => snapshot_mut.delete(key.clone()),
            };
            result.map_err(|e| format!("state service commit: storage write failed: {e}"))?;
        }
        snapshot_mut
            .try_commit()
            .map_err(|e| format!("state service commit failed: {e}"))?;
        pending.clear();
        Ok(())
    }
}
