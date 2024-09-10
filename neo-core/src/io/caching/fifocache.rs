use std::hash::Hash;
use std::collections::VecDeque;
use crate::io::caching::{Cache, CacheInterface};

pub struct FIFOCache<K, V>
where
    K: Eq + Hash + Clone,
    V: Clone,
{
    inner: Cache<K, V>,
    order: VecDeque<K>,
}

impl<K, V> CacheInterface<K, V> for FIFOCache<K, V>
where
    K: Eq + Hash + Clone,
    V: Clone,
{
    fn new(max_capacity: usize) -> Self {
        FIFOCache {
            inner: Cache::new(max_capacity),
            order: VecDeque::with_capacity(max_capacity),
        }
    }

    fn get(&self, key: &K) -> Option<V> {
        self.inner.get(key)
    }

    fn insert(&self, key: K, value: V) {
        if self.len() >= self.inner.max_capacity {
            if let Some(oldest_key) = self.order.pop_front() {
                self.inner.remove(&oldest_key);
            }
        }
        self.order.push_back(key.clone());
        self.inner.insert(key, value);
    }

    fn remove(&self, key: &K) -> Option<V> {
        let result = self.inner.remove(key);
        if result.is_some() {
            self.order.retain(|k| k != key);
        }
        result
    }

    fn clear(&self) {
        self.inner.clear();
        self.order.clear();
    }

    fn len(&self) -> usize {
        self.inner.len()
    }

    fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    fn contains_key(&self, key: &K) -> bool {
        self.inner.contains_key(key)
    }
}

impl<K, V> Clone for FIFOCache<K, V>
where
    K: Eq + Hash + Clone,
    V: Clone,
{
    fn clone(&self) -> Self {
        FIFOCache {
            inner: self.inner.clone(),
            order: self.order.clone(),
        }
    }
}
