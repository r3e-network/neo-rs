

use std::error::Error;
use crate::persistence::{IReadOnlyStore, ISnapshot};

/// This trait provides methods for reading, writing from/to database.
/// Developers should implement this trait to provide new storage engines for NEO.
pub trait IStore: IReadOnlyStore {
    /// Deletes an entry from the database.
    ///
    /// # Arguments
    ///
    /// * `key` - The key of the entry.
    fn delete(&mut self, key: &[u8]) -> Result<(), Box<dyn Error>>;

    /// Creates a snapshot of the database.
    ///
    /// # Returns
    ///
    /// A snapshot of the database.
    fn get_snapshot(&self) -> Box<dyn ISnapshot>;

    /// Puts an entry to the database.
    ///
    /// # Arguments
    ///
    /// * `key` - The key of the entry.
    /// * `value` - The data of the entry.
    fn put(&mut self, key: &[u8], value: &[u8]) -> Result<(), Box<dyn Error>>;

    /// Puts an entry to the database synchronously.
    ///
    /// # Arguments
    ///
    /// * `key` - The key of the entry.
    /// * `value` - The data of the entry.
    fn put_sync(&mut self, key: &[u8], value: &[u8]) -> Result<(), Box<dyn Error>> {
        self.put(key, value)
    }
}
