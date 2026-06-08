use super::{
    store::Store, store_provider::StoreProvider,
    providers::memory_store_provider::MemoryStoreProvider,
};
use crate::error::{StorageError, StorageResult};
use hashbrown::HashMap;
use std::sync::LazyLock;
use parking_lot::RwLock;
use std::sync::Arc;

/// Global registry of store providers.
static PROVIDERS: LazyLock<RwLock<HashMap<String, Arc<dyn StoreProvider>>>> = LazyLock::new(|| {
    let mut providers = HashMap::new();

    // Register default memory provider
    let mem_provider = Arc::new(MemoryStoreProvider::new()) as Arc<dyn StoreProvider>;
    providers.insert("Memory".to_string(), mem_provider.clone());
    providers.insert("".to_string(), mem_provider); // Default case

    RwLock::new(providers)
});

/// Factory for creating stores.
pub struct StoreFactory;

impl StoreFactory {
    /// Register a store provider.
    pub fn register_provider(provider: Arc<dyn StoreProvider>) {
        let mut providers = PROVIDERS.write();
        providers.insert(provider.name().to_string(), provider);
    }

    /// Get store provider by name.
    pub fn get_store_provider(name: &str) -> Option<Arc<dyn StoreProvider>> {
        let providers = PROVIDERS.read();
        providers.get(name).cloned()
    }

    /// Get store from name.
    ///
    /// # Arguments
    /// * `storage_provider` - The storage engine used to create the Store objects.
    ///   If this parameter is empty, a default in-memory storage engine will be used.
    /// * `path` - The path of the storage.
    ///   If storage_provider is the default in-memory storage engine, this parameter is ignored.
    pub fn get_store(storage_provider: &str, path: &str) -> StorageResult<Arc<dyn Store>> {
        let providers = PROVIDERS.read();
        let provider = providers
            .get(storage_provider)
            .or_else(|| providers.get(""))
            .cloned()
            .ok_or_else(|| StorageError::invalid_operation("Store provider not found"))?;
        provider.get_store(path)
    }
}
