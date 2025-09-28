//! Cache - matches C# Neo.IO.Caching.Cache exactly

use linked_hash_map::LinkedHashMap;
use std::hash::Hash;
use std::marker::PhantomData;
use std::sync::{Arc, Mutex};

/// Policy applied when cache entries are accessed.
pub trait CachePolicy<TKey, TValue>
where
    TKey: Eq + Hash + Clone,
{
    fn on_access(entries: &mut LinkedHashMap<TKey, TValue>, key: &TKey);
}

/// FIFO cache policy (matches C# FIFOCache behaviour where OnAccess is a no-op).
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
pub struct Cache<TKey, TValue, Policy>
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

impl<TKey, TValue, Policy> Cache<TKey, TValue, Policy>
where
    TKey: Eq + Hash + Clone,
    TValue: Clone,
    Policy: CachePolicy<TKey, TValue>,
{
    /// Creates a new cache with the specified maximum capacity and key selector.
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
    pub fn count(&self) -> usize {
        let guard = self.inner.lock().expect("cache mutex poisoned");
        guard.entries.len()
    }

    /// Indicates whether the cache is empty (C# IsEmpty helper via ICollection).
    pub fn is_empty(&self) -> bool {
        let guard = self.inner.lock().expect("cache mutex poisoned");
        guard.entries.is_empty()
    }

    /// Indicates whether the cache is read-only (always false in C# implementation).
    pub const fn is_read_only(&self) -> bool {
        false
    }

    /// Adds an item to the cache (C# Add).
    pub fn add(&self, item: TValue) {
        let key = (self.key_selector)(&item);
        let mut guard = self.inner.lock().expect("cache mutex poisoned");

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
        let mut guard = self.inner.lock().expect("cache mutex poisoned");
        guard.entries.clear();
    }

    /// Determines whether the cache contains an item with the specified key (C# Contains(TKey)).
    pub fn contains_key(&self, key: &TKey) -> bool {
        let mut guard = self.inner.lock().expect("cache mutex poisoned");
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
        let mut guard = self.inner.lock().expect("cache mutex poisoned");
        let result = guard.entries.get(key).cloned();
        if result.is_some() {
            Policy::on_access(&mut guard.entries, key);
        }
        result
    }

    /// Copies cache contents to the provided slice (C# CopyTo).
    pub fn copy_to(&self, destination: &mut [TValue], start_index: usize) {
        if start_index > destination.len() {
            panic!("start_index exceeds destination length");
        }

        let guard = self.inner.lock().expect("cache mutex poisoned");
        let count = guard.entries.len();
        if start_index + count > destination.len() {
            panic!(
                "start_index ({}) + count ({}) > destination length ({})",
                start_index,
                count,
                destination.len()
            );
        }

        for (offset, value) in guard.entries.values().cloned().enumerate() {
            destination[start_index + offset] = value;
        }
    }

    /// Removes an item by key (C# Remove(TKey)).
    pub fn remove_key(&self, key: &TKey) -> bool {
        let mut guard = self.inner.lock().expect("cache mutex poisoned");
        guard.entries.remove(key).is_some()
    }

    /// Removes an item (C# Remove(TValue)).
    pub fn remove(&self, item: &TValue) -> bool {
        let key = (self.key_selector)(item);
        self.remove_key(&key)
    }

    /// Attempts to retrieve an item by key (C# TryGet).
    pub fn try_get(&self, key: &TKey) -> Option<TValue> {
        self.get(key)
    }

    /// Returns a snapshot of the cache values preserving access order (C# GetEnumerator).
    pub fn values(&self) -> Vec<TValue> {
        let guard = self.inner.lock().expect("cache mutex poisoned");
        guard.entries.values().cloned().collect()
    }

    /// Maximum number of elements allowed in the cache.
    pub fn max_capacity(&self) -> usize {
        self.max_capacity
    }
}
