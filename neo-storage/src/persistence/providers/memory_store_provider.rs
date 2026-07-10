use super::memory_store::MemoryStore;
use crate::error::StorageResult;
use crate::persistence::store_provider::StoreProvider;
use std::path::Path;
use std::sync::Arc;

/// A provider for creating MemoryStore instances.
pub struct MemoryStoreProvider;

impl MemoryStoreProvider {
    /// Creates a new MemoryStoreProvider.
    pub fn new() -> Self {
        Self
    }

    /// Opens an in-memory store. The path is accepted for provider API
    /// consistency and ignored because memory stores are process-local.
    pub fn get_store<P>(&self, _path: P) -> StorageResult<Arc<MemoryStore>>
    where
        P: AsRef<Path>,
    {
        Ok(Arc::new(MemoryStore::new()))
    }
}

neo_io::impl_default_via_new!(MemoryStoreProvider);

impl StoreProvider for MemoryStoreProvider {
    type Store = MemoryStore;

    fn name(&self) -> &str {
        "memory"
    }

    fn get_store(&self, _path: &Path) -> StorageResult<Arc<MemoryStore>> {
        MemoryStoreProvider::get_store(self, Path::new(""))
    }
}
