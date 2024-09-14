use alloc::rc::Rc;
use crate::cryptography::ECPoint;
use crate::io::caching::{CacheInterface, FIFOCache};

pub struct ECPointCache {
    pub inner_cache: FIFOCache<Vec<u8>, ECPoint>,
}

impl ECPointCache {
    pub fn new(max_capacity: usize) -> Self {
        Self {
            inner_cache: FIFOCache::new(max_capacity),
        }
    }

    pub fn insert(&mut self, mut item: ECPoint) {
        let key = item.encode_point(true);
        self.inner_cache.insert(key, item);
    }
    pub fn get_ecpoint(&self, encoded: &[u8]) -> Option<Rc<ECPoint>> {
        let key = encoded.to_vec();
        self.inner_cache.get(&key).map(Rc::new)
    }

    pub fn remove(&mut self, item: &mut ECPoint) -> Option<ECPoint> {
        let key = item.encode_point(true);
        self.inner_cache.remove(&key)
    }

    pub fn clear(&mut self) {
        self.inner_cache.clear();
    }

    pub fn len(&self) -> usize {
        self.inner_cache.len()
    }

    pub fn is_empty(&self) -> bool {
        self.inner_cache.is_empty()
    }

    pub fn contains_key(&self, item: &mut ECPoint) -> bool {
        let key = item.encode_point(true);
        self.inner_cache.contains_key(&key)
    }
}