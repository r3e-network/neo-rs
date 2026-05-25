//! `LRUCache` - matches C# Neo.IO.Caching.LRUCache exactly

use crate::IoResult;
use lru::LruCache;
use parking_lot::Mutex;
use std::{hash::Hash, num::NonZeroUsize, sync::Arc};

/// LRU cache matching C# `LRUCache`<`TKey`, `TValue`>.
pub struct LRUCache<TKey, TValue>
where
    TKey: Eq + Hash + Clone,
    TValue: Clone,
{
    max_capacity: usize,
    key_selector: Arc<dyn Fn(&TValue) -> TKey + Send + Sync>,
    entries: Mutex<Option<LruCache<TKey, TValue>>>,
}

impl<TKey, TValue> LRUCache<TKey, TValue>
where
    TKey: Eq + Hash + Clone,
    TValue: Clone,
{
    /// Creates a new LRU cache with the specified max capacity.
    pub fn new(
        max_capacity: usize,
        key_selector: impl Fn(&TValue) -> TKey + Send + Sync + 'static,
    ) -> Self {
        Self {
            max_capacity,
            key_selector: Arc::new(key_selector),
            entries: Mutex::new(NonZeroUsize::new(max_capacity).map(LruCache::new)),
        }
    }

    /// Gets the number of cached entries (C# Count property).
    pub fn count(&self) -> usize {
        self.entries.lock().as_ref().map_or(0, LruCache::len)
    }

    /// Indicates whether the cache is empty.
    pub fn is_empty(&self) -> bool {
        self.count() == 0
    }

    /// Indicates whether the cache is read-only (always false in C# implementation).
    pub const fn is_read_only(&self) -> bool {
        false
    }

    /// Adds an item to the cache.
    pub fn add(&self, item: TValue) {
        let key = (self.key_selector)(&item);
        let mut guard = self.entries.lock();
        let Some(entries) = guard.as_mut() else {
            return;
        };

        if entries.get(&key).is_some() {
            return;
        }

        entries.put(key, item);
    }

    /// Adds a range of items to the cache.
    pub fn add_range<I>(&self, items: I)
    where
        I: IntoIterator<Item = TValue>,
    {
        for item in items {
            self.add(item);
        }
    }

    /// Clears the cache.
    pub fn clear(&self) {
        if let Some(entries) = self.entries.lock().as_mut() {
            entries.clear();
        }
    }

    /// Determines whether the cache contains an item with the specified key.
    pub fn contains_key(&self, key: &TKey) -> bool {
        self.entries
            .lock()
            .as_mut()
            .is_some_and(|entries| entries.get(key).is_some())
    }

    /// Determines whether the cache contains the specified item.
    pub fn contains(&self, item: &TValue) -> bool {
        let key = (self.key_selector)(item);
        self.contains_key(&key)
    }

    /// Retrieves an item by key, returning `None` when it is absent.
    pub fn get(&self, key: &TKey) -> Option<TValue> {
        self.entries
            .lock()
            .as_mut()
            .and_then(|entries| entries.get(key).cloned())
    }

    /// Copies cache contents to the provided slice from least to most recently used.
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

        let guard = self.entries.lock();
        let count = guard.as_ref().map_or(0, LruCache::len);
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

        if let Some(entries) = guard.as_ref() {
            for (offset, value) in entries
                .iter()
                .rev()
                .map(|(_, value)| value.clone())
                .enumerate()
            {
                destination[start_index + offset] = value;
            }
        }

        Ok(())
    }

    /// Removes an item by key.
    pub fn remove_key(&self, key: &TKey) -> bool {
        self.entries
            .lock()
            .as_mut()
            .is_some_and(|entries| entries.pop(key).is_some())
    }

    /// Removes an item.
    pub fn remove(&self, item: &TValue) -> bool {
        let key = (self.key_selector)(item);
        self.remove_key(&key)
    }

    /// Attempts to retrieve an item by key.
    pub fn try_get(&self, key: &TKey) -> Option<TValue> {
        self.get(key)
    }

    /// Returns a snapshot of cache values from least to most recently used.
    pub fn values(&self) -> Vec<TValue> {
        self.entries
            .lock()
            .as_ref()
            .map(|entries| {
                entries
                    .iter()
                    .rev()
                    .map(|(_, value)| value.clone())
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Maximum number of elements allowed in the cache.
    pub const fn max_capacity(&self) -> usize {
        self.max_capacity
    }
}

#[cfg(test)]
mod tests {
    use super::LRUCache;

    #[test]
    fn zero_capacity_keeps_no_items() {
        let cache = LRUCache::new(0, |value: &u32| *value);

        cache.add(1);

        assert_eq!(cache.count(), 0);
        assert!(cache.is_empty());
        assert!(!cache.contains_key(&1));
    }
}
