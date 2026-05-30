//! Pluggable storage backend trait for the state store.

/// Backend trait for state store persistence.
pub trait StateStoreBackend: Send + Sync {
    /// Try to get a value by key.
    fn try_get(&self, key: &[u8]) -> Option<Vec<u8>>;
    /// Put a key-value pair.
    fn put(&self, key: Vec<u8>, value: Vec<u8>);
    /// Delete a key.
    fn delete(&self, key: &[u8]);
    /// Commit changes.
    fn commit(&self) -> Result<(), String>;
}
