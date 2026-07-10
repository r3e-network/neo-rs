use super::{
    providers::{MemoryStore, RuntimeStore},
    storage::StorageConfig,
};
use crate::error::{StorageError, StorageResult};
use crate::mdbx::MdbxStoreProvider;
use crate::rocksdb::RocksDBStoreProvider;
use std::path::Path;
use std::sync::Arc;

const MEMORY_PROVIDER: &str = "memory";
const ROCKSDB_PROVIDER: &str = "rocksdb";
const MDBX_PROVIDER: &str = "mdbx";

/// Built-in storage providers supported by production node configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoreProviderKind {
    /// Process-local in-memory store for tests and ephemeral nodes.
    Memory,
    /// RocksDB-backed store retained as a supported compatibility backend.
    RocksDb,
    /// MDBX-backed store used by production nodes.
    Mdbx,
}

impl StoreProviderKind {
    /// Canonical configuration name for the provider.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Memory => MEMORY_PROVIDER,
            Self::RocksDb => ROCKSDB_PROVIDER,
            Self::Mdbx => MDBX_PROVIDER,
        }
    }

    fn from_name(name: &str) -> Option<Self> {
        match provider_key(name).as_str() {
            MEMORY_PROVIDER => Some(Self::Memory),
            ROCKSDB_PROVIDER => Some(Self::RocksDb),
            MDBX_PROVIDER => Some(Self::Mdbx),
            _ => None,
        }
    }

    fn get_store<P>(self, path: P) -> StorageResult<Arc<RuntimeStore>>
    where
        P: AsRef<Path>,
    {
        match self {
            Self::Memory => Ok(Arc::new(RuntimeStore::Memory(MemoryStore::new()))),
            Self::RocksDb => RocksDBStoreProvider::new(StorageConfig::default())
                .get_rocksdb_store(path)
                .map(RuntimeStore::RocksDb)
                .map(Arc::new),
            Self::Mdbx => MdbxStoreProvider::new(StorageConfig::default())
                .get_mdbx_store(path)
                .map(RuntimeStore::Mdbx)
                .map(Arc::new),
        }
    }

    fn get_store_with_config(self, config: StorageConfig) -> StorageResult<Arc<RuntimeStore>> {
        match self {
            Self::Memory => Ok(Arc::new(RuntimeStore::Memory(MemoryStore::new()))),
            Self::RocksDb => RocksDBStoreProvider::new(config)
                .get_rocksdb_store(Path::new(""))
                .map(RuntimeStore::RocksDb)
                .map(Arc::new),
            Self::Mdbx => MdbxStoreProvider::new(config)
                .get_mdbx_store(Path::new(""))
                .map(RuntimeStore::Mdbx)
                .map(Arc::new),
        }
    }

    fn get_runtime_store<P>(self, path: P) -> StorageResult<Arc<RuntimeStore>>
    where
        P: AsRef<Path>,
    {
        self.get_store(path)
    }

    fn get_runtime_store_with_config(
        self,
        config: StorageConfig,
    ) -> StorageResult<Arc<RuntimeStore>> {
        self.get_store_with_config(config)
    }
}

/// Facade for creating stores from named built-in providers.
///
/// This is the only production entry point for opening storage backends by
/// name. The provider choice is a small static enum, not a plugin registry,
/// because neo-rs production nodes support a fixed backend set:
/// `memory`, `mdbx`, and `rocksdb`.
pub struct StoreFactory;

impl StoreFactory {
    /// Get store provider by name.
    pub fn get_store_provider(name: &str) -> Option<StoreProviderKind> {
        if provider_key(name).is_empty() {
            return None;
        }
        StoreProviderKind::from_name(name)
    }

    /// Creates a store through an explicitly named provider.
    ///
    /// # Arguments
    /// * `storage_provider` - The storage engine used to create the Store objects.
    ///   Empty names are rejected so production callers cannot accidentally
    ///   fall back to an ephemeral in-memory store.
    /// * `path` - The path used by persistent stores. In-memory stores ignore it.
    pub fn get_store<P>(storage_provider: &str, path: P) -> StorageResult<Arc<RuntimeStore>>
    where
        P: AsRef<Path>,
    {
        provider_for(storage_provider)?.get_store(path)
    }

    /// Get store from a named provider and full storage configuration.
    ///
    /// This keeps callers on the provider/factory path when they need backend
    /// configuration beyond a path, such as read-only mode or cache settings.
    pub fn get_store_with_config(
        storage_provider: &str,
        config: StorageConfig,
    ) -> StorageResult<Arc<RuntimeStore>> {
        provider_for(storage_provider)?.get_store_with_config(config)
    }

    /// Creates a concrete runtime-selected store through an explicitly named
    /// provider.
    ///
    /// Use this in composition roots after configuration has selected the
    /// backend. It keeps downstream code generic over the concrete
    /// [`RuntimeStore`] enum instead of spreading an erased `Store` handle.
    pub fn get_runtime_store<P>(storage_provider: &str, path: P) -> StorageResult<Arc<RuntimeStore>>
    where
        P: AsRef<Path>,
    {
        provider_for(storage_provider)?.get_runtime_store(path)
    }

    /// Creates a concrete runtime-selected store from a full storage
    /// configuration.
    pub fn get_runtime_store_with_config(
        storage_provider: &str,
        config: StorageConfig,
    ) -> StorageResult<Arc<RuntimeStore>> {
        provider_for(storage_provider)?.get_runtime_store_with_config(config)
    }
}

fn empty_provider_error() -> StorageError {
    StorageError::invalid_operation(
        "empty storage provider is not supported; choose mdbx, rocksdb, or memory explicitly",
    )
}

fn provider_key(name: &str) -> String {
    name.trim().to_ascii_lowercase()
}

fn provider_for(storage_provider: &str) -> StorageResult<StoreProviderKind> {
    let key = provider_key(storage_provider);
    if key.is_empty() {
        return Err(empty_provider_error());
    }
    StoreProviderKind::from_name(storage_provider)
        .ok_or_else(|| unknown_provider_error(storage_provider))
}

fn unknown_provider_error(requested: &str) -> StorageError {
    StorageError::invalid_operation(format!(
        "Store provider {requested:?} not found; available providers: {}",
        [MEMORY_PROVIDER, MDBX_PROVIDER, ROCKSDB_PROVIDER].join(", ")
    ))
}

#[cfg(test)]
#[path = "../../tests/persistence/store_factory.rs"]
mod tests;
