use super::{
    providers::{MemoryStoreProvider, RuntimeStore},
    storage::StorageConfig,
    store_provider::StoreProvider,
};
use crate::error::{StorageError, StorageResult};
use crate::mdbx::MdbxStoreProvider;
use std::path::Path;
use std::sync::Arc;

const MEMORY_PROVIDER: &str = "memory";
const MDBX_PROVIDER: &str = "mdbx";

/// Built-in storage providers supported by production node configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoreProviderKind {
    /// Process-local in-memory store for tests and ephemeral nodes.
    Memory,
    /// MDBX-backed store used by production nodes.
    Mdbx,
}

impl StoreProviderKind {
    /// Canonical configuration name for the provider.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Memory => MEMORY_PROVIDER,
            Self::Mdbx => MDBX_PROVIDER,
        }
    }

    fn from_name(name: &str) -> Option<Self> {
        match provider_key(name).as_str() {
            MEMORY_PROVIDER => Some(Self::Memory),
            MDBX_PROVIDER => Some(Self::Mdbx),
            _ => None,
        }
    }

    fn get_store<P>(self, path: P) -> StorageResult<Arc<RuntimeStore>>
    where
        P: AsRef<Path>,
    {
        match self {
            Self::Memory => runtime_store_from_provider(&MemoryStoreProvider::new(), path.as_ref()),
            Self::Mdbx => runtime_store_from_provider(
                &MdbxStoreProvider::new(StorageConfig::default()),
                path.as_ref(),
            ),
        }
    }

    fn get_store_with_config(self, config: StorageConfig) -> StorageResult<Arc<RuntimeStore>> {
        match self {
            Self::Memory => runtime_store_from_provider_config(&MemoryStoreProvider::new(), config),
            Self::Mdbx => runtime_store_from_provider_config(
                &MdbxStoreProvider::new(StorageConfig::default()),
                config,
            ),
        }
    }
}

/// Facade for creating stores from named built-in providers.
///
/// This is the only production entry point for opening storage backends by
/// name. The provider choice is a small static enum, not a plugin registry,
/// because neo-rs supports a fixed backend set: persistent `mdbx` and
/// ephemeral `memory`.
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
    /// configuration beyond a path, such as read-only mode or MDBX geometry.
    pub fn get_store_with_config(
        storage_provider: &str,
        config: StorageConfig,
    ) -> StorageResult<Arc<RuntimeStore>> {
        provider_for(storage_provider)?.get_store_with_config(config)
    }
}

fn runtime_store_from_provider<P>(provider: &P, path: &Path) -> StorageResult<Arc<RuntimeStore>>
where
    P: StoreProvider,
    P::Store: Clone,
    RuntimeStore: From<P::Store>,
{
    provider
        .get_store(path)
        .map(|store| Arc::new(RuntimeStore::from(store.as_ref().clone())))
}

fn runtime_store_from_provider_config<P>(
    provider: &P,
    config: StorageConfig,
) -> StorageResult<Arc<RuntimeStore>>
where
    P: StoreProvider,
    P::Store: Clone,
    RuntimeStore: From<P::Store>,
{
    provider
        .get_store_with_config(config)
        .map(|store| Arc::new(RuntimeStore::from(store.as_ref().clone())))
}

fn empty_provider_error() -> StorageError {
    StorageError::invalid_operation(
        "empty storage provider is not supported; choose mdbx or memory explicitly",
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
        [MEMORY_PROVIDER, MDBX_PROVIDER].join(", ")
    ))
}

#[cfg(test)]
#[path = "../../tests/persistence/store_factory.rs"]
mod tests;
