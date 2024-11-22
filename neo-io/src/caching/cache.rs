use std::collections::HashMap;
use std::hash::Hash;
use std::sync::{Arc, RwLock};
use std::time::{SystemTime, UNIX_EPOCH};

pub trait CacheInterface<K, V>
where
    K: Eq + Hash + Clone,
    V: Clone,
{
    fn new(max_capacity: usize) -> Self;
    fn get(&self, key: &K) -> Option<V>;
    fn insert(&mut self, key: K, value: V);
    fn remove(&mut self, key: &K) -> Option<V>;
    fn clear(&mut self);
    fn len(&self) -> usize;
    fn is_empty(&self) -> bool;
    fn contains_key(&self, key: &K) -> bool;
}

pub struct Cache<K, V>
where
    K: Eq + Hash + Clone,
    V: Clone,
{
    inner: Arc<RwLock<InnerCache<K, V>>>,
    pub max_capacity: usize,
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
    key:   K,
    value: V,
    time:  u64,
}

impl<K, V> CacheInterface<K, V> for Cache<K, V>
where
    K: Eq + Hash + Clone,
    V: Clone,
{
    fn new(max_capacity: usize) -> Self {
        Cache { inner: Arc::new(RwLock::new(InnerCache { map: HashMap::new() })), max_capacity }
    }

    fn get(&self, key: &K) -> Option<V> {
        let mut inner = self.inner.write().unwrap();
        if let Some(item) = inner.map.get_mut(key) {
            item.time = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
            Some(item.value.clone())
        } else {
            None
        }
    }

    fn insert(&mut self, key: K, value: V) {
        let mut inner = self.inner.write().unwrap();
        if inner.map.len() >= self.max_capacity {
            let oldest = inner.map.iter().min_by_key(|(_, v)| v.time).map(|(k, _)| k.clone());
            if let Some(oldest_key) = oldest {
                inner.map.remove(&oldest_key);
            }
        }
        inner.map.insert(key.clone(), CacheItem {
            key,
            value,
            time: SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs(),
        });
    }

    fn remove(&mut self, key: &K) -> Option<V> {
        let mut inner = self.inner.write().unwrap();
        inner.map.remove(key).map(|item| item.value)
    }

    fn clear(&mut self) {
        let mut inner = self.inner.write().unwrap();
        inner.map.clear();
    }

    fn len(&self) -> usize {
        let inner = self.inner.read().unwrap();
        inner.map.len()
    }

    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn contains_key(&self, key: &K) -> bool {
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
        Cache { inner: self.inner.clone(), max_capacity: self.max_capacity }
    }
}
