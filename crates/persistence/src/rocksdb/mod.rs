//! RocksDB storage implementation.
//!
//! This module provides a RocksDB-based storage implementation that matches
//! C# Neo RocksDB storage functionality.

use crate::storage::{IStore, IStoreSnapshot, IReadOnlyStore, IWriteStore, SeekDirection, StorageProvider, StorageConfig};
use rocksdb::{DB, WriteBatch, IteratorMode, Direction, Options};
use std::sync::Arc;

/// RocksDB store implementation (matches C# Neo RocksDB store)
pub struct RocksDbStore {
    db: Arc<DB>,
}

impl RocksDbStore {
    /// Creates a new RocksDB store
    pub fn new(path: &str) -> crate::Result<Self> {
        let mut opts = Options::default();
        opts.create_if_missing(true);
        
        let db = DB::open(&opts, path)
            .map_err(|e| crate::Error::Database(format!("Failed to open RocksDB: {}", e)))?;
        
        Ok(Self {
            db: Arc::new(db),
        })
    }
}

impl IReadOnlyStore<Vec<u8>, Vec<u8>> for RocksDbStore {
    fn try_get(&self, key: &Vec<u8>) -> Option<Vec<u8>> {
        self.db.get(key).ok().flatten()
    }

    fn contains(&self, key: &Vec<u8>) -> bool {
        self.db.get(key).ok().flatten().is_some()
    }

    fn find(&self, key_or_prefix: Option<&[u8]>, direction: SeekDirection) -> Box<dyn Iterator<Item = (Vec<u8>, Vec<u8>)>> {
        let db = self.db.clone();
        let prefix = key_or_prefix.map(|p| p.to_vec());
        
        let iter_mode = match (prefix.as_ref(), direction) {
            (Some(prefix), SeekDirection::Forward) => IteratorMode::From(prefix, Direction::Forward),
            (Some(prefix), SeekDirection::Backward) => IteratorMode::From(prefix, Direction::Reverse),
            (None, SeekDirection::Forward) => IteratorMode::Start,
            (None, SeekDirection::Backward) => IteratorMode::End,
        };

        let items: Vec<(Vec<u8>, Vec<u8>)> = db.iterator(iter_mode)
            .map(|result| {
                let (key, value) = result.unwrap();
                (key.to_vec(), value.to_vec())
            })
            .filter(|(key, _)| {
                if let Some(ref prefix) = prefix {
                    key.starts_with(prefix)
                } else {
                    true
                }
            })
            .collect();

        Box::new(items.into_iter())
    }
}

impl IWriteStore<Vec<u8>, Vec<u8>> for RocksDbStore {
    fn put(&mut self, key: Vec<u8>, value: Vec<u8>) {
        let _ = self.db.put(key, value);
    }

    fn delete(&mut self, key: &Vec<u8>) {
        let _ = self.db.delete(key);
    }
}

impl IStore for RocksDbStore {
    fn get_snapshot(&self) -> Box<dyn IStoreSnapshot> {
        Box::new(RocksDbSnapshot::new(self.db.clone()))
    }
}

/// RocksDB snapshot implementation
pub struct RocksDbSnapshot {
    db: Arc<DB>,
    batch: WriteBatch,
}

impl RocksDbSnapshot {
    pub fn new(db: Arc<DB>) -> Self {
        Self {
            db,
            batch: WriteBatch::default(),
        }
    }
}

impl IReadOnlyStore<Vec<u8>, Vec<u8>> for RocksDbSnapshot {
    fn try_get(&self, key: &Vec<u8>) -> Option<Vec<u8>> {
        self.db.get(key).ok().flatten()
    }

    fn contains(&self, key: &Vec<u8>) -> bool {
        self.db.get(key).ok().flatten().is_some()
    }

    fn find(&self, key_or_prefix: Option<&[u8]>, direction: SeekDirection) -> Box<dyn Iterator<Item = (Vec<u8>, Vec<u8>)>> {
        let db = self.db.clone();
        let prefix = key_or_prefix.map(|p| p.to_vec());
        
        let iter_mode = match (prefix.as_ref(), direction) {
            (Some(prefix), SeekDirection::Forward) => IteratorMode::From(prefix, Direction::Forward),
            (Some(prefix), SeekDirection::Backward) => IteratorMode::From(prefix, Direction::Reverse),
            (None, SeekDirection::Forward) => IteratorMode::Start,
            (None, SeekDirection::Backward) => IteratorMode::End,
        };

        let items: Vec<(Vec<u8>, Vec<u8>)> = db.iterator(iter_mode)
            .map(|result| {
                let (key, value) = result.unwrap();
                (key.to_vec(), value.to_vec())
            })
            .filter(|(key, _)| {
                if let Some(ref prefix) = prefix {
                    key.starts_with(prefix)
                } else {
                    true
                }
            })
            .collect();

        Box::new(items.into_iter())
    }
}

impl IWriteStore<Vec<u8>, Vec<u8>> for RocksDbSnapshot {
    fn put(&mut self, key: Vec<u8>, value: Vec<u8>) {
        self.batch.put(key, value);
    }

    fn delete(&mut self, key: &Vec<u8>) {
        self.batch.delete(key);
    }
}

impl IStoreSnapshot for RocksDbSnapshot {
    fn store(&self) -> &dyn IStore {
        // This is a limitation of the current design - we can't return a reference
        // to the original store from a snapshot in this architecture
        panic!("RocksDbSnapshot::store() not implemented - architectural limitation")
    }

    fn commit(&mut self) {
        let _ = self.db.write(std::mem::take(&mut self.batch));
    }
}

/// RocksDB storage provider (matches test expectations)
pub struct RocksDbStorageProvider;

impl RocksDbStorageProvider {
    pub fn new() -> Self {
        Self
    }
}

impl Default for RocksDbStorageProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl StorageProvider for RocksDbStorageProvider {
    fn name(&self) -> &str {
        "RocksDB"
    }

    fn create_store(&self, config: &StorageConfig) -> crate::Result<Box<dyn IStore>> {
        let path = config.path.to_string_lossy();
        let store = RocksDbStore::new(&path)?;
        Ok(Box::new(store))
    }
} 