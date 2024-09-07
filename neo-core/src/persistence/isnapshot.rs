use std::error::Error;
use crate::persistence::IReadOnlyStore;

/// This trait provides methods for reading, writing, and committing from/to snapshot.
pub trait ISnapshot: IReadOnlyStore {
    /// Commits all changes in the snapshot to the database.
    fn commit(&mut self) -> Result<(), Box<dyn Error>>;

    /// Deletes an entry from the snapshot.
    ///
    /// # Arguments
    ///
    /// * `key` - The key of the entry.
    fn delete(&mut self, key: &[u8]) -> Result<(), Box<dyn Error>>;

    /// Puts an entry to the snapshot.
    ///
    /// # Arguments
    ///
    /// * `key` - The key of the entry.
    /// * `value` - The data of the entry.
    fn put(&mut self, key: &[u8], value: &[u8]) -> Result<(), Box<dyn Error>>;
}
