//! LRUCache - matches C# Neo.IO.Caching.LRUCache exactly

use super::cache::{Cache, LruPolicy};
use std::hash::Hash;
use std::ops::{Deref, DerefMut};

/// LRU cache matching C# LRUCache<TKey, TValue>.
pub struct LRUCache<TKey, TValue>
where
    TKey: Eq + Hash + Clone,
    TValue: Clone,
{
    inner: Cache<TKey, TValue, LruPolicy>,
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
            inner: Cache::new(max_capacity, key_selector),
        }
    }
}

impl<TKey, TValue> Deref for LRUCache<TKey, TValue>
where
    TKey: Eq + Hash + Clone,
    TValue: Clone,
{
    type Target = Cache<TKey, TValue, LruPolicy>;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl<TKey, TValue> DerefMut for LRUCache<TKey, TValue>
where
    TKey: Eq + Hash + Clone,
    TValue: Clone,
{
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}
