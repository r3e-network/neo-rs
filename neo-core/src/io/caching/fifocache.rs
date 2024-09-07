

use std::hash::Hash;
use std::collections::HashMap;

pub mod caching {
    use crate::io::caching::Cache;
    use super::*;

    pub struct FIFOCache<K, V>
    where
        K: Eq + Hash + Clone,
        V: Clone,
    {
        inner: Cache<K, V>,
    }

    impl<K, V> FIFOCache<K, V>
    where
        K: Eq + Hash + Clone,
        V: Clone,
    {
        pub fn new(max_capacity: usize) -> Self {
            FIFOCache {
                inner: Cache::new(max_capacity),
            }
        }
    }

    impl<K, V> Cache<K, V> for FIFOCache<K, V>
    where
        K: Eq + Hash + Clone,
        V: Clone,
    {
        fn on_access(&mut self, _item: &CacheItem<K, V>) {
            // FIFO cache doesn't need to do anything on access
        }
    }
}
