use std::collections::HashMap;
use neo_io::{CacheInterface, FIFOCache};
use crate::cryptography::ECPoint;

#[derive(Clone)]
pub struct ECDsaCacheItem {
    key: ECPoint,
    value: VerifyingKey,
}

impl ECDsaCacheItem {
    pub fn new(key: ECPoint, value: VerifyingKey) -> Self {
        Self { key, value }
    }
}

pub struct ECDsaCache {
    cache: FIFOCache<ECPoint, ECDsaCacheItem>,
}

impl ECDsaCache {
    pub fn new(max_capacity: usize) -> Self {
        Self {
            cache: FIFOCache::new(max_capacity, HashMap::new()),
        }
    }

    fn get_key_for_item(&self, item: &ECDsaCacheItem) -> ECPoint {
        item.key.clone()
    }
}

impl Default for ECDsaCache {
    fn default() -> Self {
        Self::new(20000)
    }
}
