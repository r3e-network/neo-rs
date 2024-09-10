use crate::persistence::ReadOnlyStoreTrait;
use crate::persistence::persistence_error::PersistenceError;

/// This trait provides methods for reading, writing, and committing from/to snapshot.
pub trait SnapshotTrait: ReadOnlyStoreTrait {
    /// Commits all changes in the snapshot to the database.
    fn commit(&mut self) -> Result<(), PersistenceError>;

    /// Deletes an entry from the snapshot.
    ///
    /// # Arguments
    ///
    /// * `key` - The key of the entry.
    fn delete(&mut self, key: &[u8]) -> Result<(), PersistenceError>;

    /// Puts an entry to the snapshot.
    ///
    /// # Arguments
    ///
    /// * `key` - The key of the entry.
    /// * `value` - The data of the entry.
    fn put(&mut self, key: &[u8], value: &[u8]) -> Result<(), PersistenceError>;
}
