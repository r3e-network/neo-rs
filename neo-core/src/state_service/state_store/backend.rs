use crate::cryptography::mpt_trie::{MptResult, MptStoreSnapshot};
use crate::persistence::store::Store;
use parking_lot::{Mutex, RwLock};
use std::collections::HashMap;
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

/// Backend trait for state store persistence.
pub trait StateStoreBackend: Send + Sync {
    /// Try to get a value by key.
    fn try_get(&self, key: &[u8]) -> Option<Vec<u8>>;
    /// Put a key-value pair.
    fn put(&self, key: Vec<u8>, value: Vec<u8>);
    /// Delete a key.
    fn delete(&self, key: &[u8]);
    /// Commit changes.
    fn commit(&self) -> Result<(), String>;
}

/// Minimal transactional wrapper over a `StateStoreBackend`.
///
/// This helper batches writes and applies them atomically via the backend, keeping
/// commit semantics explicit for callers that need a scoped write.
pub struct StateStoreTransaction {
    backend: Arc<dyn StateStoreBackend>,
    writes: Vec<(Vec<u8>, Option<Vec<u8>>)>,
}

impl StateStoreTransaction {
    /// Creates a transaction bound to the provided backend.
    pub fn new(backend: Arc<dyn StateStoreBackend>) -> Self {
        Self {
            backend,
            writes: Vec::new(),
        }
    }

    /// Enqueue a put operation.
    pub fn put(&mut self, key: Vec<u8>, value: Vec<u8>) {
        self.writes.push((key, Some(value)));
    }

    /// Enqueue a delete operation.
    pub fn delete(&mut self, key: &[u8]) {
        self.writes.push((key.to_vec(), None));
    }

    /// Applies all enqueued operations and commits the backend.
    pub fn commit(mut self) -> Result<(), String> {
        for (key, value) in self.writes.drain(..) {
            match value {
                Some(v) => self.backend.put(key, v),
                None => self.backend.delete(&key),
            }
        }
        self.backend.commit()
    }
}

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
