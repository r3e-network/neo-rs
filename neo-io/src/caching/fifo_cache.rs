//! FIFOCache - matches C# Neo.IO.Caching.FIFOCache exactly

use super::cache::{Cache, FifoPolicy};
use std::hash::Hash;
use std::ops::{Deref, DerefMut};

/// FIFO cache matching C# FIFOCache<TKey, TValue>.
pub struct FIFOCache<TKey, TValue>
where
    TKey: Eq + Hash + Clone,
    TValue: Clone,
{
    inner: Cache<TKey, TValue, FifoPolicy>,
}

impl<TKey, TValue> FIFOCache<TKey, TValue>
where
    TKey: Eq + Hash + Clone,
    TValue: Clone,
{
    /// Creates a new FIFO cache with the specified max capacity.
    pub fn new(
        max_capacity: usize,
        key_selector: impl Fn(&TValue) -> TKey + Send + Sync + 'static,
    ) -> Self {
        Self {
            inner: Cache::new(max_capacity, key_selector),
        }
    }
}

impl<TKey, TValue> Deref for FIFOCache<TKey, TValue>
where
    TKey: Eq + Hash + Clone,
    TValue: Clone,
{
    type Target = Cache<TKey, TValue, FifoPolicy>;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl<TKey, TValue> DerefMut for FIFOCache<TKey, TValue>
where
    TKey: Eq + Hash + Clone,
    TValue: Clone,
{
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}
