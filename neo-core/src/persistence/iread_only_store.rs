
/// Direction for seeking in the database.
pub enum SeekDirection {
    Forward,
    Backward,
}

/// This trait provides methods to read from the database.
pub trait IReadOnlyStore {
    /// Seeks to the entry with the specified key.
    ///
    /// # Arguments
    ///
    /// * `key` - The key to be sought.
    /// * `direction` - The direction of seek.
    ///
    /// # Returns
    ///
    /// An iterator containing all the entries after seeking.
    fn seek(&self, key: &[u8], direction: SeekDirection) -> Box<dyn Iterator<Item = (Vec<u8>, Vec<u8>)>>;

    /// Reads a specified entry from the database.
    ///
    /// # Arguments
    ///
    /// * `key` - The key of the entry.
    ///
    /// # Returns
    ///
    /// The data of the entry, or `None` if it doesn't exist.
    fn try_get(&self, key: &[u8]) -> Option<Vec<u8>>;

    /// Determines whether the database contains the specified entry.
    ///
    /// # Arguments
    ///
    /// * `key` - The key of the entry.
    ///
    /// # Returns
    ///
    /// `true` if the database contains an entry with the specified key; otherwise, `false`.
    fn contains(&self, key: &[u8]) -> bool;
}
