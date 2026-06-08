//! Cache - matches C# Neo.IO.Caching.Cache exactly
//!
//! This module provides the shared FIFO cache implementation used by the
//! specialised cache wrappers.

use super::cache_entries::FifoEntries;
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
    entries: Mutex<FifoEntries<TKey, TValue>>,
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
            entries: Mutex::new(FifoEntries::new(max_capacity)),
        }
    }

    /// Adds an item to the cache (C# Add).
    ///
    /// If an item with the same key already exists, the value is not updated.
    /// If the cache is at capacity, the oldest entry is evicted.
    pub fn add(&self, item: TValue) {
        let key = (self.key_selector)(&item);
        self.entries.lock().insert_if_absent(key, item);
    }

    /// Determines whether the cache contains an item with the specified key (C# Contains(TKey)).
    pub fn contains_key(&self, key: &TKey) -> bool {
        self.entries.lock().contains(key)
    }

    /// Retrieves an item by key, returning `None` when it is absent (C# indexer).
    pub fn get(&self, key: &TKey) -> Option<TValue> {
        self.entries.lock().peek_cloned(key)
    }

    impl_cache_facade!();
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
