//! Storage interfaces and types.
//!
//! This module defines the core storage abstractions that exactly match C# Neo persistence interfaces.

use super::seek_direction::SeekDirection;
use crate::error::{CoreError, CoreResult};
use crate::neo_config::MAX_SCRIPT_SIZE;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Compression algorithms (matches C# Neo compression support)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CompressionAlgorithm {
    None,
    Lz4,
    Zstd,
}

/// Compaction strategy for database optimization (matches C# Neo)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CompactionStrategy {
    Level,
    Universal,
    Fifo,
}

/// Batch operation for bulk database operations (matches C# Neo)
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BatchOperation {
    Put { key: Vec<u8>, value: Vec<u8> },
    Delete { key: Vec<u8> },
}

/// Storage configuration (matches C# Neo storage configuration, RocksDB only)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageConfig {
    pub path: PathBuf,
    pub compression_algorithm: CompressionAlgorithm,
    pub compaction_strategy: CompactionStrategy,
    pub max_open_files: Option<u32>,
    pub cache_size: Option<usize>,
    pub write_buffer_size: Option<usize>,
    pub enable_statistics: bool,
    pub read_only: bool,
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self {
            path: PathBuf::from("./data"),
            compression_algorithm: CompressionAlgorithm::Lz4,
            compaction_strategy: CompactionStrategy::Level,
            max_open_files: Some(1000),
            cache_size: Some(64 * MAX_SCRIPT_SIZE * MAX_SCRIPT_SIZE),
            write_buffer_size: Some(16 * MAX_SCRIPT_SIZE * MAX_SCRIPT_SIZE),
            enable_statistics: false,
            read_only: false,
        }
    }
}

/// Read-only store interface (matches C# IReadOnlyStore<TKey, TValue>)
pub trait IReadOnlyStore<TKey, TValue> {
    /// Tries to get a value by key (matches C# TryGet)
    fn try_get(&self, key: &TKey) -> Option<TValue>;

    /// Checks if a key exists (matches C# Contains)
    fn contains(&self, key: &TKey) -> bool;

    /// Finds entries with optional key prefix (matches C# Find)
    fn find(
        &self,
        key_or_prefix: Option<&[u8]>,
        direction: SeekDirection,
    ) -> Box<dyn Iterator<Item = (Vec<u8>, Vec<u8>)>>;
}

/// Write store interface (matches C# IWriteStore<TKey, TValue>)
pub trait IWriteStore<TKey, TValue> {
    /// Puts a key-value pair (matches C# Put)
    fn put(&mut self, key: TKey, value: TValue);

    /// Deletes a key (matches C# Delete)
    fn delete(&mut self, key: &TKey);

    /// Puts a key-value pair synchronously (matches C# PutSync)
    fn put_sync(&mut self, key: TKey, value: TValue) {
        self.put(key, value);
    }
}

/// Store interface (matches C# IStore)
pub trait IStore: IReadOnlyStore<Vec<u8>, Vec<u8>> + IWriteStore<Vec<u8>, Vec<u8>> {
    /// Creates a snapshot of the database (matches C# GetSnapshot)
    fn get_snapshot(&self) -> Box<dyn IStoreSnapshot>;
}

/// Store snapshot interface (matches C# IStoreSnapshot)
pub trait IStoreSnapshot: IReadOnlyStore<Vec<u8>, Vec<u8>> + IWriteStore<Vec<u8>, Vec<u8>> {
    /// Gets the store this snapshot belongs to (matches C# Store property)
    fn store(&self) -> &dyn IStore;

    /// Commits all changes in the snapshot to the database (matches C# Commit)
    fn commit(&mut self);
}

/// Storage provider interface (matches C# IStoreProvider) - RocksDB only
pub trait StorageProvider: Send + Sync {
    /// Gets the name of the storage provider
    fn name(&self) -> &str;

    /// Creates a new store instance
    fn create_store(&self, config: &StorageConfig) -> crate::Result<Box<dyn IStore>>;
}

/// Key used to address values in persistent storage.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct StorageKey {
    id: i32,
    key: Vec<u8>,
}

impl StorageKey {
    /// Creates a new storage key using the provided contract identifier and key bytes.
    pub fn new(id: i32, key: Vec<u8>) -> Self {
        Self { id, key }
    }

    /// Parses a storage key from the serialized representation (`id` + `key`).
    pub fn from_bytes(bytes: &[u8]) -> Self {
        if bytes.len() < 4 {
            return Self::new(0, bytes.to_vec());
        }

        let mut id_bytes = [0u8; 4];
        id_bytes.copy_from_slice(&bytes[..4]);
        let id = i32::from_le_bytes(id_bytes);
        let key = bytes[4..].to_vec();
        Self::new(id, key)
    }

    /// Serializes this key to bytes.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut buffer = Vec::with_capacity(4 + self.key.len());
        buffer.extend_from_slice(&self.id.to_le_bytes());
        buffer.extend_from_slice(&self.key);
        buffer
    }

    /// Contract identifier associated with this storage key.
    pub fn id(&self) -> i32 {
        self.id
    }

    /// Raw key bytes.
    pub fn key(&self) -> &[u8] {
        &self.key
    }
}

/// Value stored within the persistence layer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageItem {
    value: Vec<u8>,
    constant: bool,
}

impl StorageItem {
    /// Creates a new storage item with the supplied value.
    pub fn new<V: Into<Vec<u8>>>(value: V) -> Self {
        Self {
            value: value.into(),
            constant: false,
        }
    }

    /// Creates a constant storage item.
    pub fn new_constant<V: Into<Vec<u8>>>(value: V) -> Self {
        Self {
            value: value.into(),
            constant: true,
        }
    }

    /// Returns the stored value.
    pub fn value(&self) -> &[u8] {
        &self.value
    }

    /// Mutable access to the stored value. Panics if the item is flagged constant.
    pub fn value_mut(&mut self) -> &mut Vec<u8> {
        assert!(
            !self.constant,
            "attempted to mutate a constant storage item"
        );
        &mut self.value
    }

    /// Indicates whether this item was marked constant.
    pub fn is_constant(&self) -> bool {
        self.constant
    }

    /// Replaces the stored value. Panics if the item is constant.
    pub fn set_value<V: Into<Vec<u8>>>(&mut self, value: V) {
        assert!(
            !self.constant,
            "attempted to mutate a constant storage item"
        );
        self.value = value.into();
    }

    /// Clones the storage item, preserving the constant flag.
    pub fn clone_item(&self) -> Self {
        Self {
            value: self.value.clone(),
            constant: self.constant,
        }
    }
}

/// Convenience alias for storage-related results.
pub type StorageResult<T> = CoreResult<T>;

/// Error type used by cache operations.
#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    #[error("key not found")]
    NotFound,
    #[error("cache is read only")]
    ReadOnly,
    #[error("{0}")]
    Other(String),
}

impl From<StorageError> for CoreError {
    fn from(err: StorageError) -> Self {
        CoreError::InvalidOperation {
            message: err.to_string(),
        }
    }
}
