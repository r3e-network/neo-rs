use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use crate::persistence::{ReadOnlyStoreTrait, SnapshotTrait, SeekDirection};
use crate::persistence::persistence_error::PersistenceError;

pub struct MemorySnapshot {
    inner_data: Arc<RwLock<HashMap<Vec<u8>, Vec<u8>>>>,
    write_batch: RwLock<HashMap<Vec<u8>, Option<Vec<u8>>>>,
}

impl MemorySnapshot {
    pub fn new(inner_data: Arc<RwLock<HashMap<Vec<u8>, Vec<u8>>>>) -> Self {
        MemorySnapshot {
            inner_data,
            write_batch: RwLock::new(HashMap::new()),
        }
    }
}

impl ReadOnlyStoreTrait for MemorySnapshot {
    fn seek(&self, key: &[u8], direction: SeekDirection) -> Box<dyn Iterator<Item=(Vec<u8>, Vec<u8>)>> {
        let inner_data = self.inner_data.read().unwrap();
        let write_batch = self.write_batch.read().unwrap();

        let mut results: Vec<(Vec<u8>, Vec<u8>)> = inner_data
            .iter()
            .filter(|(k, _)| k.as_slice().starts_with(key))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();

        // Apply write batch changes
        for (k, v_opt) in write_batch.iter() {
            if k.as_slice().starts_with(key) {
                if let Some(v) = v_opt {
                    if let Some(existing) = results.iter_mut().find(|(existing_k, _)| existing_k == k) {
                        existing.1 = v.clone();
                    } else {
                        results.push((k.clone(), v.clone()));
                    }
                } else {
                    results.retain(|(existing_k, _)| existing_k != k);
                }
            }
        }

        match direction {
            SeekDirection::Forward => results.sort_by(|a, b| a.0.cmp(&b.0)),
            SeekDirection::Backward => results.sort_by(|a, b| b.0.cmp(&a.0)),
        }

        Box::new(results.into_iter())
    }

    fn try_get(&self, key: &[u8]) -> Option<Vec<u8>> {
        let write_batch = self.write_batch.read().unwrap();
        if let Some(value_opt) = write_batch.get(key) {
            return value_opt.clone();
        }

        let inner_data = self.inner_data.read().unwrap();
        inner_data.get(key).cloned()
    }

    fn contains(&self, key: &[u8]) -> bool {
        let write_batch = self.write_batch.read().unwrap();
        if let Some(value_opt) = write_batch.get(key) {
            return value_opt.is_some();
        }

        let inner_data = self.inner_data.read().unwrap();
        inner_data.contains_key(key)
    }
}

impl SnapshotTrait for MemorySnapshot {
    fn commit(&self) -> Result<(), PersistenceError> {
        let mut inner_data = self.inner_data.write().map_err(|_| "Lock poisoned")?;
        let write_batch = self.write_batch.read().map_err(|_| "Lock poisoned")?;

        for (key, value_opt) in write_batch.iter() {
            match value_opt {
                Some(value) => {
                    inner_data.insert(key.clone(), value.clone());
                }
                None => {
                    inner_data.remove(key);
                }
            }
        }
        Ok(())
    }

    fn delete(&mut self, key: &[u8]) -> Result<(), PersistenceError> {
        let mut write_batch = self.write_batch.write().map_err(|_| "Lock poisoned")?;
        write_batch.insert(Vec::from(key), None);
        Ok(())
    }

    fn put(&mut self, key: &[u8], value: &[u8]) -> Result<(), PersistenceError> {
        let mut write_batch = self.write_batch.write().map_err(|_| "Lock poisoned")?;
        write_batch.insert(Vec::from(key), Some(Vec::from(value)));
        Ok(())
    }
}
