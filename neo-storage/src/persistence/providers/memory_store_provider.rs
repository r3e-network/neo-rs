use super::memory_store::MemoryStore;
use crate::error::StorageResult;
use crate::persistence::{store::Store, store_provider::StoreProvider};
use std::sync::Arc;

/// A provider for creating MemoryStore instances.
pub struct MemoryStoreProvider;

impl MemoryStoreProvider {
    /// Creates a new MemoryStoreProvider.
    pub fn new() -> Self {
        Self
    }
}

neo_io::impl_default_via_new!(MemoryStoreProvider);

impl StoreProvider for MemoryStoreProvider {
    fn name(&self) -> &str {
        "Memory"
    }

    fn get_store(&self, _path: &str) -> StorageResult<Arc<dyn Store>> {
        Ok(Arc::new(MemoryStore::new()))
    }
}
