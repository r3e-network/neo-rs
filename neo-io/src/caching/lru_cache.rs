//! `LRUCache` - matches C# Neo.IO.Caching.LRUCache exactly

use super::ordered_cache::OrderedCache;
use parking_lot::Mutex;
use std::{hash::Hash, sync::Arc};

/// LRU cache matching C# `LRUCache`<`TKey`, `TValue`>.
pub struct LRUCache<TKey, TValue>
where
    TKey: Eq + Hash + Clone,
    TValue: Clone,
{
    max_capacity: usize,
    key_selector: Arc<dyn Fn(&TValue) -> TKey + Send + Sync>,
    entries: Mutex<OrderedCache<TKey, TValue>>,
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
            entries: Mutex::new(OrderedCache::new(max_capacity)),
        }
    }

    /// Adds an item to the cache.
    pub fn add(&self, item: TValue) {
        let key = (self.key_selector)(&item);
        self.entries.lock().insert_or_touch(key, item);
    }

    /// Determines whether the cache contains an item with the specified key.
    pub fn contains_key(&self, key: &TKey) -> bool {
        self.entries.lock().touch(key)
    }

    /// Retrieves an item by key, returning `None` when it is absent.
    pub fn get(&self, key: &TKey) -> Option<TValue> {
        self.entries.lock().get_cloned(key)
    }

    impl_ordered_cache_facade!();
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
