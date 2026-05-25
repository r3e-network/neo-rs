//! Cache - matches C# Neo.IO.Caching.Cache exactly
//!
//! This module provides the shared FIFO cache implementation used by the
//! specialised cache wrappers.

use crate::IoResult;
use indexmap::IndexMap;
use parking_lot::Mutex;
use std::hash::Hash;
use std::sync::Arc;

/// Abstract cache base class matching C# Cache<`TKey`, `TValue`>.
///
/// This is a thread-safe cache implementation that supports configurable eviction
/// policy. LRU behavior is implemented by [`crate::caching::lru_cache::LRUCache`]
/// using the upstream `lru` crate.
///
/// # Type Parameters
///
/// * `TKey` - The key type, must be hashable and cloneable
/// * `TValue` - The value type, must be cloneable
/// # Example
///
/// ```rust,ignore
/// use neo_io::caching::IoCache;
///
/// let cache: IoCache<String, i32> = IoCache::new(100, |v| format!("key_{}", v));
/// cache.add(42);
/// assert!(cache.contains_key(&"key_42".to_string()));
/// ```
pub struct IoCache<TKey, TValue>
where
    TKey: Eq + Hash + Clone,
    TValue: Clone,
{
    max_capacity: usize,
    key_selector: Arc<dyn Fn(&TValue) -> TKey + Send + Sync>,
    entries: Mutex<IndexMap<TKey, TValue>>,
}

impl<TKey, TValue> IoCache<TKey, TValue>
where
    TKey: Eq + Hash + Clone,
    TValue: Clone,
{
    /// Creates a new cache with the specified maximum capacity and key selector.
    ///
    /// # Arguments
    ///
    /// * `max_capacity` - Maximum number of entries the cache can hold
    /// * `key_selector` - Function to extract the key from a value
    pub fn new(
        max_capacity: usize,
        key_selector: impl Fn(&TValue) -> TKey + Send + Sync + 'static,
    ) -> Self {
        Self {
            max_capacity,
            key_selector: Arc::new(key_selector),
            entries: Mutex::new(IndexMap::with_capacity(max_capacity)),
        }
    }

    /// Gets the number of cached entries (C# Count property).
    #[inline]
    pub fn count(&self) -> usize {
        self.entries.lock().len()
    }

    /// Indicates whether the cache is empty (C# `IsEmpty` helper via `ICollection`).
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.entries.lock().is_empty()
    }

    /// Indicates whether the cache is read-only (always false in C# implementation).
    #[inline]
    pub const fn is_read_only(&self) -> bool {
        false
    }

    /// Adds an item to the cache (C# Add).
    ///
    /// If an item with the same key already exists, the value is not updated.
    /// If the cache is at capacity, the oldest entry is evicted.
    pub fn add(&self, item: TValue) {
        if self.max_capacity == 0 {
            return;
        }

        let key = (self.key_selector)(&item);
        let mut entries = self.entries.lock();

        if entries.contains_key(&key) {
            return;
        }

        if entries.len() >= self.max_capacity {
            entries.shift_remove_index(0);
        }

        entries.insert(key, item);
    }

    /// Adds a range of items to the cache (C# `AddRange`).
    pub fn add_range<I>(&self, items: I)
    where
        I: IntoIterator<Item = TValue>,
    {
        for item in items {
            self.add(item);
        }
    }

    /// Clears the cache (C# Clear).
    pub fn clear(&self) {
        self.entries.lock().clear();
    }

    /// Determines whether the cache contains an item with the specified key (C# Contains(TKey)).
    pub fn contains_key(&self, key: &TKey) -> bool {
        self.entries.lock().contains_key(key)
    }

    /// Determines whether the cache contains the specified item (C# Contains(TValue)).
    pub fn contains(&self, item: &TValue) -> bool {
        let key = (self.key_selector)(item);
        self.contains_key(&key)
    }

    /// Retrieves an item by key, returning `None` when it is absent (C# indexer).
    pub fn get(&self, key: &TKey) -> Option<TValue> {
        self.entries.lock().get(key).cloned()
    }

    /// Copies cache contents to the provided slice (C# `CopyTo`).
    ///
    /// # Arguments
    ///
    /// * `destination` - The slice to copy values into
    /// * `start_index` - The starting index in the destination slice
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - `start_index` exceeds the destination length
    /// - The cache contents don't fit in the remaining destination space
    pub fn copy_to(&self, destination: &mut [TValue], start_index: usize) -> IoResult<()> {
        if start_index > destination.len() {
            return Err(crate::IoError::InvalidData {
                context: "copy_to".to_string(),
                value: format!(
                    "start_index ({}) exceeds destination length ({})",
                    start_index,
                    destination.len()
                ),
            });
        }

        let entries = self.entries.lock();
        let count = entries.len();
        let end_index =
            start_index
                .checked_add(count)
                .ok_or_else(|| crate::IoError::InvalidData {
                    context: "copy_to".to_string(),
                    value: format!("start_index ({start_index}) + count ({count}) overflows"),
                })?;
        if end_index > destination.len() {
            return Err(crate::IoError::InvalidData {
                context: "copy_to".to_string(),
                value: format!(
                    "start_index ({}) + count ({}) > destination length ({})",
                    start_index,
                    count,
                    destination.len()
                ),
            });
        }

        for (offset, value) in entries.values().cloned().enumerate() {
            destination[start_index + offset] = value;
        }

        Ok(())
    }

    /// Removes an item by key (C# Remove(TKey)).
    ///
    /// Returns `true` if the item was found and removed, `false` otherwise.
    pub fn remove_key(&self, key: &TKey) -> bool {
        self.entries.lock().shift_remove(key).is_some()
    }

    /// Removes an item (C# Remove(TValue)).
    ///
    /// Returns `true` if the item was found and removed, `false` otherwise.
    pub fn remove(&self, item: &TValue) -> bool {
        let key = (self.key_selector)(item);
        self.remove_key(&key)
    }

    /// Attempts to retrieve an item by key (C# `TryGet`).
    #[inline]
    pub fn try_get(&self, key: &TKey) -> Option<TValue> {
        self.get(key)
    }

    /// Returns a snapshot of the cache values preserving access order (C# `GetEnumerator`).
    pub fn values(&self) -> Vec<TValue> {
        self.entries.lock().values().cloned().collect()
    }

    /// Maximum number of elements allowed in the cache.
    #[inline]
    pub const fn max_capacity(&self) -> usize {
        self.max_capacity
    }
}

/// Backwards-compatible alias matching the original `Cache<TKey, TValue>` name.
pub type Cache<TKey, TValue> = IoCache<TKey, TValue>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fifo_cache_basic_operations() {
        let cache: IoCache<i32, i32> = IoCache::new(3, |v| *v);

        cache.add(1);
        cache.add(2);
        cache.add(3);

        assert_eq!(cache.count(), 3);
        assert!(cache.contains_key(&1));
        assert!(cache.contains_key(&2));
        assert!(cache.contains_key(&3));

        // Adding a 4th item should evict the oldest (1)
        cache.add(4);
        assert_eq!(cache.count(), 3);
        assert!(!cache.contains_key(&1));
        assert!(cache.contains_key(&4));
    }

    #[test]
    fn test_copy_to_success() {
        let cache: IoCache<i32, i32> = IoCache::new(3, |v| *v);
        cache.add(1);
        cache.add(2);

        let mut dest = vec![0; 5];
        assert!(cache.copy_to(&mut dest, 1).is_ok());
        assert_eq!(dest[1], 1);
        assert_eq!(dest[2], 2);
    }

    #[test]
    fn test_copy_to_bounds_error() {
        let cache: IoCache<i32, i32> = IoCache::new(3, |v| *v);
        cache.add(1);
        cache.add(2);

        let mut dest = vec![0; 2];
        assert!(cache.copy_to(&mut dest, 1).is_err());
    }

    #[test]
    fn test_clear() {
        let cache: IoCache<i32, i32> = IoCache::new(3, |v| *v);
        cache.add(1);
        cache.add(2);

        cache.clear();
        assert!(cache.is_empty());
        assert_eq!(cache.count(), 0);
    }

    #[test]
    fn test_remove() {
        let cache: IoCache<i32, i32> = IoCache::new(3, |v| *v);
        cache.add(1);
        cache.add(2);

        assert!(cache.remove_key(&1));
        assert!(!cache.contains_key(&1));
        assert!(!cache.remove_key(&1)); // Already removed
    }
}
