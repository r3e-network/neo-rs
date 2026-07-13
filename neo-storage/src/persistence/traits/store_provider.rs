use super::{storage::StorageConfig, store::Store};
use crate::error::StorageResult;
use std::path::Path;
use std::sync::Arc;

/// Backend factory used to create [`Store`] instances.
///
/// This trait is named `StoreProvider` because each implementation provides one
/// named storage backend (`memory` or `mdbx`) to the
/// [`crate::persistence::StoreFactory`] registry. It is distinct from
/// `neo_runtime::StoreProvider`, which is an accessor for an already-created
/// store in composition code.
pub trait StoreProvider: Send + Sync {
    /// Concrete store type created by this provider.
    type Store: Store;

    /// Gets the name of the StoreProvider.
    fn name(&self) -> &str;

    /// Creates a new concrete store instance.
    fn get_store(&self, path: &Path) -> StorageResult<Arc<Self::Store>>;

    /// Creates a new store from a full storage configuration.
    ///
    /// Providers that only need a path can rely on this default. Providers
    /// with backend-specific tuning, durability, or read-only settings should
    /// override it so factory callers do not have to bypass the provider trait.
    fn get_store_with_config(&self, config: StorageConfig) -> StorageResult<Arc<Self::Store>> {
        self.get_store(&config.path)
    }
}
