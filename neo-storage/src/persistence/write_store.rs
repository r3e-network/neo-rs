use crate::error::StorageResult;

/// This interface provides methods to write to the database.
pub trait WriteStore<TKey, TValue> {
    /// Deletes an entry from the store.
    fn delete(&mut self, key: TKey) -> StorageResult<()>;

    /// Puts an entry to the store.
    fn put(&mut self, key: TKey, value: TValue) -> StorageResult<()>;

    /// Puts an entry to the database synchronously.
    fn put_sync(&mut self, key: TKey, value: TValue) -> StorageResult<()> {
        self.put(key, value)
    }
}
