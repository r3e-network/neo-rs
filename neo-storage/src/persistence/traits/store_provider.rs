use super::{storage::StorageConfig, store::Store};
use crate::error::StorageResult;
use std::any::Any;
use std::path::Path;
use std::sync::Arc;

/// Backend factory used to create [`Store`] instances.
///
/// This trait is named `StoreProvider` because each implementation provides one
/// named storage backend (`memory`, `mdbx`, `rocksdb`) to the
/// [`crate::persistence::StoreFactory`] registry. It is distinct from
/// `neo_runtime::StoreProvider`, which is an accessor for an already-created
/// store in composition code.
pub trait StoreProvider: Send + Sync + Any {
    /// Gets the name of the StoreProvider.
    fn name(&self) -> &str;

    /// Creates a new instance of the Store interface.
    fn get_store(&self, path: &Path) -> StorageResult<Arc<dyn Store>>;

    /// Creates a new store from a full storage configuration.
    ///
    /// Providers that only need a path can rely on this default. Providers
    /// with backend-specific tuning, durability, or read-only settings should
    /// override it so factory callers do not have to bypass the provider trait.
    fn get_store_with_config(&self, config: StorageConfig) -> StorageResult<Arc<dyn Store>> {
        self.get_store(&config.path)
    }

    /// Downcast support for provider tests and factory diagnostics.
    fn as_any(&self) -> &dyn Any;
}
