//! Cache - matches C# Neo.IO.Caching.Cache exactly
//!
//! This module provides thread-safe caching implementations with configurable
//! eviction policies (FIFO, LRU) matching the C# Neo reference implementation.

use crate::IoResult;
use linked_hash_map::LinkedHashMap;
use parking_lot::Mutex;
use std::hash::Hash;
use std::marker::PhantomData;
use std::sync::Arc;

/// Policy applied when cache entries are accessed.
pub trait CachePolicy<TKey, TValue>
where
    TKey: Eq + Hash + Clone,
{
    /// Called when an entry is accessed, allowing the policy to reorder entries.
    fn on_access(entries: &mut LinkedHashMap<TKey, TValue>, key: &TKey);
}

/// FIFO cache policy (matches C# FIFOCache behaviour where OnAccess is a no-op).
///
/// Entries are evicted in the order they were added, regardless of access patterns.
#[derive(Debug, Default, Clone, Copy)]
pub struct FifoPolicy;

impl<TKey, TValue> CachePolicy<TKey, TValue> for FifoPolicy
where
    TKey: Eq + Hash + Clone,
{
    #[inline]
    fn on_access(_: &mut LinkedHashMap<TKey, TValue>, _: &TKey) {}
}

/// LRU cache policy (matches C# LRUCache behaviour moving entries to the head on access).
///
/// Recently accessed entries are moved to the end, making them less likely to be evicted.
#[derive(Debug, Default, Clone, Copy)]
pub struct LruPolicy;

impl<TKey, TValue> CachePolicy<TKey, TValue> for LruPolicy
where
    TKey: Eq + Hash + Clone,
{
    #[inline]
    fn on_access(entries: &mut LinkedHashMap<TKey, TValue>, key: &TKey) {
        entries.get_refresh(key);
    }
}

#[derive(Debug)]
struct CacheInner<TKey, TValue>
where
    TKey: Eq + Hash + Clone,
{
    entries: LinkedHashMap<TKey, TValue>,
}

impl<TKey, TValue> CacheInner<TKey, TValue>
where
    TKey: Eq + Hash + Clone,
{
    fn with_capacity(capacity: usize) -> Self {
        Self {
            entries: LinkedHashMap::with_capacity(capacity),
        }
    }

    fn remove_oldest(&mut self) {
        self.entries.pop_front();
    }
}

/// Abstract cache base class matching C# Cache<TKey, TValue>.
///
/// This is a thread-safe cache implementation that supports configurable eviction
/// policies through the `Policy` type parameter.
///
/// # Type Parameters
///
/// * `TKey` - The key type, must be hashable and cloneable
/// * `TValue` - The value type, must be cloneable
/// * `Policy` - The eviction policy (e.g., `FifoPolicy` or `LruPolicy`)
///
/// # Example
///
/// ```rust,ignore
/// use neo_io::caching::{IoCache, LruPolicy};
///
/// let cache: IoCache<String, i32, LruPolicy> = IoCache::new(100, |v| format!("key_{}", v));
/// cache.add(42);
/// assert!(cache.contains_key(&"key_42".to_string()));
/// ```
pub struct IoCache<TKey, TValue, Policy>
where
    TKey: Eq + Hash + Clone,
    TValue: Clone,
    Policy: CachePolicy<TKey, TValue>,
{
    max_capacity: usize,
    key_selector: Arc<dyn Fn(&TValue) -> TKey + Send + Sync>,
    inner: Mutex<CacheInner<TKey, TValue>>,
    _policy: PhantomData<Policy>,
}

impl<TKey, TValue, Policy> IoCache<TKey, TValue, Policy>
where
    TKey: Eq + Hash + Clone,
    TValue: Clone,
    Policy: CachePolicy<TKey, TValue>,
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
            inner: Mutex::new(CacheInner::with_capacity(max_capacity)),
            _policy: PhantomData,
        }
    }

    /// Gets the number of cached entries (C# Count property).
    #[inline]
    pub fn count(&self) -> usize {
        self.inner.lock().entries.len()
    }

    /// Indicates whether the cache is empty (C# IsEmpty helper via ICollection).
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.inner.lock().entries.is_empty()
    }

    /// Indicates whether the cache is read-only (always false in C# implementation).
    #[inline]
    pub const fn is_read_only(&self) -> bool {
        false
    }

    /// Adds an item to the cache (C# Add).
    ///
    /// If an item with the same key already exists, the access policy is applied
    /// but the value is not updated. If the cache is at capacity, the oldest
    /// entry (according to the policy) is evicted.
    pub fn add(&self, item: TValue) {
        let key = (self.key_selector)(&item);
        let mut guard = self.inner.lock();

        if guard.entries.contains_key(&key) {
            Policy::on_access(&mut guard.entries, &key);
            return;
        }

        if guard.entries.len() >= self.max_capacity {
            guard.remove_oldest();
        }

        guard.entries.insert(key, item);
    }

    /// Adds a range of items to the cache (C# AddRange).
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
        self.inner.lock().entries.clear();
    }

    /// Determines whether the cache contains an item with the specified key (C# Contains(TKey)).
    pub fn contains_key(&self, key: &TKey) -> bool {
        let mut guard = self.inner.lock();
        let exists = guard.entries.contains_key(key);
        if exists {
            Policy::on_access(&mut guard.entries, key);
        }
        exists
    }

    /// Determines whether the cache contains the specified item (C# Contains(TValue)).
    pub fn contains(&self, item: &TValue) -> bool {
        let key = (self.key_selector)(item);
        self.contains_key(&key)
    }

    /// Retrieves an item by key, returning `None` when it is absent (C# indexer).
    pub fn get(&self, key: &TKey) -> Option<TValue> {
        let mut guard = self.inner.lock();
        let result = guard.entries.get(key).cloned();
        if result.is_some() {
            Policy::on_access(&mut guard.entries, key);
        }
        result
    }

    /// Copies cache contents to the provided slice (C# CopyTo).
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

        let guard = self.inner.lock();
        let count = guard.entries.len();
        if start_index + count > destination.len() {
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

        for (offset, value) in guard.entries.values().cloned().enumerate() {
            destination[start_index + offset] = value;
        }

        Ok(())
    }

    /// Removes an item by key (C# Remove(TKey)).
    ///
    /// Returns `true` if the item was found and removed, `false` otherwise.
    pub fn remove_key(&self, key: &TKey) -> bool {
        self.inner.lock().entries.remove(key).is_some()
    }

    /// Removes an item (C# Remove(TValue)).
    ///
    /// Returns `true` if the item was found and removed, `false` otherwise.
    pub fn remove(&self, item: &TValue) -> bool {
        let key = (self.key_selector)(item);
        self.remove_key(&key)
    }

    /// Attempts to retrieve an item by key (C# TryGet).
    #[inline]
    pub fn try_get(&self, key: &TKey) -> Option<TValue> {
        self.get(key)
    }

    /// Returns a snapshot of the cache values preserving access order (C# GetEnumerator).
    pub fn values(&self) -> Vec<TValue> {
        self.inner.lock().entries.values().cloned().collect()
    }

    /// Maximum number of elements allowed in the cache.
    #[inline]
    pub fn max_capacity(&self) -> usize {
        self.max_capacity
    }
}

/// Backwards-compatible alias matching the original `Cache<TKey, TValue, Policy>` name.
pub type Cache<TKey, TValue, Policy> = IoCache<TKey, TValue, Policy>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fifo_cache_basic_operations() {
        let cache: IoCache<i32, i32, FifoPolicy> = IoCache::new(3, |v| *v);

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
    fn test_lru_cache_access_pattern() {
        let cache: IoCache<i32, i32, LruPolicy> = IoCache::new(3, |v| *v);

        cache.add(1);
        cache.add(2);
        cache.add(3);

        // Access item 1 to make it recently used
        cache.get(&1);

        // Adding a 4th item should evict 2 (least recently used)
        cache.add(4);
        assert!(cache.contains_key(&1));
        assert!(!cache.contains_key(&2));
        assert!(cache.contains_key(&3));
        assert!(cache.contains_key(&4));
    }

    #[test]
    fn test_copy_to_success() {
        let cache: IoCache<i32, i32, FifoPolicy> = IoCache::new(3, |v| *v);
        cache.add(1);
        cache.add(2);

        let mut dest = vec![0; 5];
        assert!(cache.copy_to(&mut dest, 1).is_ok());
        assert_eq!(dest[1], 1);
        assert_eq!(dest[2], 2);
    }

    #[test]
    fn test_copy_to_bounds_error() {
        let cache: IoCache<i32, i32, FifoPolicy> = IoCache::new(3, |v| *v);
        cache.add(1);
        cache.add(2);

        let mut dest = vec![0; 2];
        assert!(cache.copy_to(&mut dest, 1).is_err());
    }

    #[test]
    fn test_clear() {
        let cache: IoCache<i32, i32, FifoPolicy> = IoCache::new(3, |v| *v);
        cache.add(1);
        cache.add(2);

        cache.clear();
        assert!(cache.is_empty());
        assert_eq!(cache.count(), 0);
    }

    #[test]
    fn test_remove() {
        let cache: IoCache<i32, i32, FifoPolicy> = IoCache::new(3, |v| *v);
        cache.add(1);
        cache.add(2);

        assert!(cache.remove_key(&1));
        assert!(!cache.contains_key(&1));
        assert!(!cache.remove_key(&1)); // Already removed
    }
}
