
use std::collections::HashMap;
use std::sync::Once;
use std::sync::RwLock;
use lazy_static::lazy_static;
use crate::persistence::{IStoreProvider, MemoryStoreProvider};

pub struct StoreFactory;

lazy_static! {
    static ref PROVIDERS: RwLock<HashMap<String, Box<dyn IStoreProvider>>> = RwLock::new(HashMap::new());
    static ref INIT: Once = Once::new();
}

impl StoreFactory {
    fn initialize() {
        INIT.call_once(|| {
            let mem_provider = Box::new(MemoryStoreProvider::new());
            Self::register_provider(mem_provider.clone());

            // Default cases
            PROVIDERS.write().unwrap().insert(String::new(), mem_provider);
        });
    }

    pub fn register_provider(provider: Box<dyn IStoreProvider>) {
        Self::initialize();
        let mut providers = PROVIDERS.write().unwrap();
        providers.insert(provider.name().to_string(), provider);
    }

    /// Get store provider by name
    ///
    /// # Arguments
    ///
    /// * `name` - Name of the provider
    ///
    /// # Returns
    ///
    /// Option containing the store provider if found, None otherwise
    pub fn get_store_provider(name: &str) -> Option<Box<dyn IStoreProvider>> {
        Self::initialize();
        let providers = PROVIDERS.read().unwrap();
        providers.get(name).cloned()
    }

    /// Get store from name
    ///
    /// # Arguments
    ///
    /// * `storage_provider` - The storage engine used to create the `IStore` objects. If this parameter is None, a default in-memory storage engine will be used.
    /// * `path` - The path of the storage. If `storage_provider` is the default in-memory storage engine, this parameter is ignored.
    ///
    /// # Returns
    ///
    /// The storage engine.
    pub fn get_store(storage_provider: &str, path: &str) -> Box<dyn IStore> {
        Self::initialize();
        let providers = PROVIDERS.read().unwrap();
        providers.get(storage_provider)
            .expect("Storage provider not found")
            .get_store(path)
    }
}
