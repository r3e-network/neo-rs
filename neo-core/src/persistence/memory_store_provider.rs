use crate::persistence::{IStore, StoreProviderTrait, MemoryStore};
use crate::store::Store;

pub struct MemoryStoreProvider;

impl StoreProviderTrait for MemoryStoreProvider {
    fn name(&self) -> &str {
        "MemoryStore"
    }

    fn get_store(&self, _path: &str) -> Box<dyn IStore> {
        Box::new(MemoryStore::new())
    }
}