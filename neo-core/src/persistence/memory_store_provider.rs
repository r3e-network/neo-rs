use crate::persistence::IStoreProvider;
use crate::store::Store;

pub struct MemoryStoreProvider;

impl IStoreProvider for MemoryStoreProvider {
    fn name(&self) -> &str {
        "MemoryStore"
    }

    fn get_store(&self, _path: &str) -> Box<dyn Store> {
        Box::new(MemoryStore::new())
    }
}
