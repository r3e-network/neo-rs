
use std::path::Path;
use crate::persistence::IStore;

/// A provider used to create `IStore` instances.
pub trait StoreProviderTrait {
    /// Gets the name of the `IStoreProvider`.
    fn name(&self) -> &str;

    /// Creates a new instance of the `IStore` trait.
    ///
    /// # Arguments
    ///
    /// * `path` - The path of the database.
    ///
    /// # Returns
    ///
    /// The created `IStore` instance.
    fn get_store<P: AsRef<Path>>(&self, path: P) -> Box<dyn IStore>;
}
