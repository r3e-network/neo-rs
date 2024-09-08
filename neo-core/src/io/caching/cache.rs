
use std::collections::HashMap;
use std::hash::Hash;
use std::sync::{Arc, RwLock};
use std::time::{SystemTime, UNIX_EPOCH};

pub struct Cache<K, V>
where
    K: Eq + Hash + Clone,
    V: Clone,
{
    inner: Arc<RwLock<InnerCache<K, V>>>,
    max_capacity: usize,
}

struct InnerCache<K, V>
where
    K: Eq + Hash + Clone,
    V: Clone,
{
    map: HashMap<K, CacheItem<K, V>>,
}

struct CacheItem<K, V>
where
    K: Clone,
    V: Clone,
{
    key: K,
    value: V,
    time: u64,
}

impl<K, V> Cache<K, V>
where
    K: Eq + Hash + Clone,
    V: Clone,
{
    pub fn new(max_capacity: usize) -> Self {
        Cache {
            inner: Arc::new(RwLock::new(InnerCache {
                map: HashMap::new(),
            })),
            max_capacity,
        }
    }

    pub fn get(&self, key: &K) -> Option<V> {
        let mut inner = self.inner.write().unwrap();
        if let Some(item) = inner.map.get_mut(key) {
            item.time = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs();
            Some(item.value.clone())
        } else {
            None
        }
    }

    pub fn insert(&self, key: K, value: V) {
        let mut inner = self.inner.write().unwrap();
        if inner.map.len() >= self.max_capacity {
            let oldest = inner
                .map
                .iter()
                .min_by_key(|(_, v)| v.time)
                .map(|(k, _)| k.clone());
            if let Some(oldest_key) = oldest {
                inner.map.remove(&oldest_key);
            }
        }
        inner.map.insert(
            key.clone(),
            CacheItem {
                key,
                value,
                time: SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_secs(),
            },
        );
    }

    pub fn remove(&self, key: &K) -> Option<V> {
        let mut inner = self.inner.write().unwrap();
        inner.map.remove(key).map(|item| item.value)
    }

    pub fn clear(&self) {
        let mut inner = self.inner.write().unwrap();
        inner.map.clear();
    }

    pub fn len(&self) -> usize {
        let inner = self.inner.read().unwrap();
        inner.map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn contains_key(&self, key: &K) -> bool {
        let inner = self.inner.read().unwrap();
        inner.map.contains_key(key)
    }
}

impl<K, V> Clone for Cache<K, V>
where
    K: Eq + Hash + Clone,
    V: Clone,
{
    fn clone(&self) -> Self {
        Cache {
            inner: self.inner.clone(),
            max_capacity: self.max_capacity,
        }
    }
}

// Note: The following traits are not implemented in this conversion
// as they are not typically used in NEO smart contracts:
// - ICollection<TValue>
// - IDisposable
// - IEnumerable

// The parallel processing and LINQ operations have been removed
// as they are not supported in NEO smart contracts.

// The ReaderWriterLockSlim has been replaced with a simple RwLock
// which is more idiomatic in Rust and suitable for NEO smart contracts.

// The custom CacheItem struct has been simplified and the DateTime
// has been replaced with a u64 timestamp for simplicity and compatibility.

// The GetKeyForItem and OnAccess methods have been removed as they
// were part of the abstract class implementation which is not
// necessary in this Rust version.

// Error handling has been simplified to use Option types instead of
// exceptions, which is more idiomatic in Rust and suitable for
// NEO smart contracts.
