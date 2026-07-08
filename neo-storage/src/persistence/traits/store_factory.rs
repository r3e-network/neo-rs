use super::{
    providers::memory_store_provider::MemoryStoreProvider, storage::StorageConfig, store::Store,
    store_provider::StoreProvider,
};
use crate::error::{StorageError, StorageResult};
use crate::mdbx::MdbxStoreProvider;
use crate::rocksdb::RocksDBStoreProvider;
use hashbrown::HashMap;
use parking_lot::RwLock;
use std::path::Path;
use std::sync::Arc;
use std::sync::LazyLock;

const MEMORY_PROVIDER: &str = "memory";
const ROCKSDB_PROVIDER: &str = "rocksdb";
const MDBX_PROVIDER: &str = "mdbx";

/// Global registry of store providers.
static PROVIDERS: LazyLock<RwLock<HashMap<String, Arc<dyn StoreProvider>>>> = LazyLock::new(|| {
    let mut providers = HashMap::new();

    let mem_provider = Arc::new(MemoryStoreProvider::new()) as Arc<dyn StoreProvider>;
    register_builtin_provider(&mut providers, MEMORY_PROVIDER, mem_provider);

    let rocksdb_provider =
        Arc::new(RocksDBStoreProvider::new(StorageConfig::default())) as Arc<dyn StoreProvider>;
    register_builtin_provider(&mut providers, ROCKSDB_PROVIDER, rocksdb_provider);

    let mdbx_provider =
        Arc::new(MdbxStoreProvider::new(StorageConfig::default())) as Arc<dyn StoreProvider>;
    register_builtin_provider(&mut providers, MDBX_PROVIDER, mdbx_provider);

    RwLock::new(providers)
});

/// Registry-backed facade for creating stores from named providers.
///
/// This is the only production entry point for opening storage backends by
/// name. Concrete backends implement [`StoreProvider`]; callers ask this facade
/// for `memory`, `mdbx`, or `rocksdb` stores instead of constructing backend
/// adapters directly.
pub struct StoreFactory;

impl StoreFactory {
    /// Register a store provider.
    pub fn register_provider(provider: Arc<dyn StoreProvider>) {
        let mut providers = PROVIDERS.write();
        providers.insert(provider_key(provider.name()), provider);
    }

    /// Get store provider by name.
    pub fn get_store_provider(name: &str) -> Option<Arc<dyn StoreProvider>> {
        if provider_key(name).is_empty() {
            return None;
        }
        let providers = PROVIDERS.read();
        providers.get(&provider_key(name)).cloned()
    }

    /// Creates a store through an explicitly named provider.
    ///
    /// # Arguments
    /// * `storage_provider` - The storage engine used to create the Store objects.
    ///   Empty names are rejected so production callers cannot accidentally
    ///   fall back to an ephemeral in-memory store.
    /// * `path` - The path used by persistent stores. In-memory stores ignore it.
    pub fn get_store<P>(storage_provider: &str, path: P) -> StorageResult<Arc<dyn Store>>
    where
        P: AsRef<Path>,
    {
        provider_for(storage_provider)?.get_store(path.as_ref())
    }

    /// Get store from a named provider and full storage configuration.
    ///
    /// This keeps callers on the provider/factory path when they need backend
    /// configuration beyond a path, such as read-only mode or cache settings.
    pub fn get_store_with_config(
        storage_provider: &str,
        config: StorageConfig,
    ) -> StorageResult<Arc<dyn Store>> {
        provider_for(storage_provider)?.get_store_with_config(config)
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

fn register_builtin_provider(
    providers: &mut HashMap<String, Arc<dyn StoreProvider>>,
    name: &str,
    provider: Arc<dyn StoreProvider>,
) {
    providers.insert(provider_key(name), provider);
}

fn provider_for(storage_provider: &str) -> StorageResult<Arc<dyn StoreProvider>> {
    let key = provider_key(storage_provider);
    if key.is_empty() {
        return Err(empty_provider_error());
    }
    let providers = PROVIDERS.read();
    providers
        .get(&key)
        .cloned()
        .ok_or_else(|| unknown_provider_error(storage_provider, &providers))
}

fn unknown_provider_error(
    requested: &str,
    providers: &HashMap<String, Arc<dyn StoreProvider>>,
) -> StorageError {
    let mut available = providers
        .keys()
        .filter(|name| !name.is_empty())
        .cloned()
        .collect::<Vec<_>>();
    available.sort_unstable();
    available.dedup();

    StorageError::invalid_operation(format!(
        "Store provider {requested:?} not found; available providers: {}",
        available.join(", ")
    ))
}

#[cfg(test)]
#[path = "../../tests/persistence/store_factory.rs"]
mod tests;
