
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use crate::persistence::{ReadOnlyStoreTrait, SnapshotTrait, IStore, MemorySnapshot, SeekDirection};

/// An in-memory `IStore` implementation that uses HashMap as the underlying storage.
pub struct MemoryStore {
    inner_data: Arc<RwLock<HashMap<Vec<u8>, Vec<u8>>>>,
}

impl MemoryStore {
    pub fn new() -> Self {
        MemoryStore {
            inner_data: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub fn reset(&mut self) {
        self.inner_data.write().unwrap().clear();
    }
}

impl ReadOnlyStoreTrait for MemoryStore {
    fn seek(&self, key: &[u8], direction: SeekDirection) -> Box<dyn Iterator<Item=(Vec<u8>, Vec<u8>)>> {
        todo!()
    }

    fn try_get(&self, key: &[u8]) -> Option<Vec<u8>> {
        todo!()
    }

    fn contains(&self, key: &[u8]) -> bool {
        todo!()
    }
}

impl IStore for MemoryStore {
    fn delete(&mut self, key: &[u8]) -> Result<(), Box<dyn std::error::Error>> {
        self.inner_data.write().unwrap().remove(&Vec::from(key));
        Ok(())
    }

    fn get_snapshot(&self) -> Box<dyn SnapshotTrait> {
        Box::new(MemorySnapshot::new(Arc::clone(&self.inner_data)))
    }

    fn put(&mut self, key: &[u8], value: &[u8]) -> Result<(), Box<dyn std::error::Error>> {
        self.inner_data.write().unwrap().insert(Vec::from(key), Vec::from(value));
        Ok(())
    }

    fn seek(&self, key_or_prefix: &[u8], direction: SeekDirection) -> Box<dyn Iterator<Item = (Vec<u8>, Vec<u8>)>> {
        let data = self.inner_data.read().unwrap();
        let mut items: Vec<_> = data.iter()
            .filter(|(k, _)| k.as_slice().starts_with(key_or_prefix))
            .map(|(k, v)| (k.to_vec(), v.to_vec()))
            .collect();

        match direction {
            SeekDirection::Forward => items.sort_by(|a, b| a.0.cmp(&b.0)),
            SeekDirection::Backward => items.sort_by(|a, b| b.0.cmp(&a.0)),
        }

        Box::new(items.into_iter())
    }

    fn try_get(&self, key: &[u8]) -> Option<Vec<u8>> {
        self.inner_data.read().unwrap().get(&Vec::from(key)).cloned().map(|v| v.to_vec())
    }

    fn contains(&self, key: &[u8]) -> bool {
        self.inner_data.read().unwrap().contains_key(&Vec::from(key))
    }
}

impl Drop for MemoryStore {
    fn drop(&mut self) {
        // No need to implement anything here as Rust's ownership system will handle cleanup
    }
}
