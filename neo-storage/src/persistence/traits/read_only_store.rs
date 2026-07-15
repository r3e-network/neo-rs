use super::seek_direction::SeekDirection;
use crate::StorageResult;
use crate::types::{StorageItem, StorageKey};

/// This interface provides methods to read from the database.
pub trait ReadOnlyStore: ReadOnlyStoreGeneric<StorageKey, StorageItem> {}

/// This interface provides methods to read from the database (generic version).
pub trait ReadOnlyStoreGeneric<TKey, TValue>
where
    TKey: Clone,
    TValue: Clone,
{
    /// Concrete iterator returned by [`ReadOnlyStoreGeneric::find`].
    ///
    /// Storage backends are concrete performance components, so prefix scans
    /// should not be forced through a boxed iterator at the trait boundary.
    /// Backends can still choose their own iterator shape: a materialized
    /// in-memory iterator, a database cursor adapter, or an enum over multiple
    /// scan modes.
    type FindIterator<'a>: Iterator<Item = (TKey, TValue)> + 'a
    where
        Self: 'a,
        TKey: 'a,
        TValue: 'a;

    /// Reads a specified entry from the database.
    /// Returns the data of the entry, or None if it doesn't exist.
    ///
    /// Backends with fallible I/O must not map read failures to `None` on the
    /// consensus path. Prefer [`Self::try_get_result`] when absence and backend
    /// failure must be distinguished. The legacy method remains for call sites
    /// that only need best-effort presence and already tolerate soft failures.
    fn try_get(&self, key: &TKey) -> Option<TValue>;

    /// Fallible point lookup that distinguishes a missing key from a failed read.
    ///
    /// The default preserves compatibility with infallible stores by wrapping
    /// [`Self::try_get`]. Durable backends should override this and return
    /// `Err` when the underlying storage operation fails so canonical mutation
    /// can abort instead of treating I/O errors as absent state.
    fn try_get_result(&self, key: &TKey) -> StorageResult<Option<TValue>> {
        Ok(self.try_get(key))
    }

    /// Determines whether the database contains the specified entry.
    fn contains(&self, key: &TKey) -> bool {
        self.try_get(key).is_some()
    }

    /// Finds the entries starting with the specified prefix.
    fn find(&self, key_prefix: Option<&TKey>, direction: SeekDirection) -> Self::FindIterator<'_>;

    /// Gets the entry with the specified key, returning `None` if absent.
    fn get(&self, key: &TKey) -> Option<TValue> {
        self.try_get(key)
    }
}

/// Borrowed raw byte-key lookup for backends whose consensus/storage hot paths
/// already hold keys as byte slices.
pub trait RawReadOnlyStore {
    /// Reads a raw byte-key entry without forcing callers to allocate a
    /// temporary `Vec<u8>` just to satisfy the generic store trait.
    fn try_get_bytes(&self, key: &[u8]) -> Option<Vec<u8>>;

    /// Fallible raw byte-key lookup.
    ///
    /// The default preserves compatibility with stores whose existing raw read
    /// surface is infallible. Backends with fallible I/O should override this so
    /// callers can distinguish a missing key from a failed read.
    fn try_get_bytes_result(&self, key: &[u8]) -> StorageResult<Option<Vec<u8>>> {
        Ok(self.try_get_bytes(key))
    }

    /// Reads several raw byte keys in caller-supplied order.
    ///
    /// The returned vector has one entry per input key, preserving both order
    /// and duplicates. Callers with content-addressed keys should sort and
    /// deduplicate before invoking this method when they want storage locality.
    /// Backends may override the default to reuse one transaction or cursor.
    fn try_get_many_bytes<K>(&self, keys: &[K]) -> StorageResult<Vec<Option<Vec<u8>>>>
    where
        K: AsRef<[u8]>,
    {
        keys.iter()
            .map(|key| self.try_get_bytes_result(key.as_ref()))
            .collect()
    }
}
