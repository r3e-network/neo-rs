//! Scoped transactional wrapper over a [`StateStoreBackend`].

use super::backend::StateStoreBackend;
use std::sync::Arc;

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
