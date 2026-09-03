use super::store::Store;
use crate::error::StorageResult;
use std::any::Any;
use std::sync::Arc;

/// A provider used to create Store instances.
pub trait StoreProvider: Send + Sync + Any {
    /// Gets the name of the StoreProvider.
    fn name(&self) -> &str;

    /// Creates a new instance of the Store interface.
    fn get_store(&self, path: &str) -> StorageResult<Arc<dyn Store>>;
}
