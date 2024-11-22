use std::collections::{HashSet, LinkedList};
use std::hash::Hash;
use std::sync::{Arc, RwLock};

use crate::CacheInterface;

pub struct HashSetCache<T>
where T: Eq + Hash + Clone
{
    inner: Arc<RwLock<InnerHashSetCache<T>>>,
    bucket_capacity: usize,
    max_bucket_count: usize,
}

struct InnerHashSetCache<T>
where T: Eq + Hash + Clone
{
    sets:  LinkedList<HashSet<T>>,
    count: usize,
}

impl<T> CacheInterface<T, ()> for HashSetCache<T>
where T: Eq + Hash + Clone
{
    fn new(max_capacity: usize) -> Self {
        let bucket_capacity = max_capacity.max(1);
        let max_bucket_count = 1;
        let mut sets = LinkedList::new();
        sets.push_front(HashSet::new());

        HashSetCache {
            inner: Arc::new(RwLock::new(InnerHashSetCache { sets, count: 0 })),
            bucket_capacity,
            max_bucket_count,
        }
    }

    fn get(&self, key: &T) -> Option<()> {
        let inner = self.inner.read().unwrap();
        if inner.sets.iter().any(|set| set.contains(key)) {
            Some(())
        } else {
            None
        }
    }

    fn insert(&mut self, key: T, _value: ()) {
        let mut inner = self.inner.write().unwrap();
        if !inner.sets.iter().any(|set| set.contains(&key)) {
            inner.count += 1;
            if let Some(first) = inner.sets.front_mut() {
                if first.len() < self.bucket_capacity {
                    first.insert(key);
                    return;
                }
            }
            let mut new_set = HashSet::new();
            new_set.insert(key);
            inner.sets.push_front(new_set);
            if inner.sets.len() > self.max_bucket_count {
                if let Some(last) = inner.sets.pop_back() {
                    inner.count -= last.len();
                }
            }
        }
    }

    fn remove(&mut self, key: &T) -> Option<()> {
        let mut inner = self.inner.write().unwrap();
        for set in inner.sets.iter_mut() {
            if set.remove(key) {
                inner.count -= 1;
                return Some(());
            }
        }
        None
    }

    fn clear(&mut self) {
        let mut inner = self.inner.write().unwrap();
        inner.sets.clear();
        inner.sets.push_front(HashSet::new());
        inner.count = 0;
    }

    fn len(&self) -> usize {
        let inner = self.inner.read().unwrap();
        inner.count
    }

    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn contains_key(&self, key: &T) -> bool {
        self.get(key).is_some()
    }
}

impl<T> HashSetCache<T>
where T: Eq + Hash + Clone
{
    pub fn new_with_buckets(bucket_capacity: usize, max_bucket_count: usize) -> Self {
        if bucket_capacity == 0 {
            panic!("bucket_capacity should be greater than 0");
        }
        if max_bucket_count == 0 {
            panic!("max_bucket_count should be greater than 0");
        }

        let mut sets = LinkedList::new();
        sets.push_front(HashSet::new());

        HashSetCache {
            inner: Arc::new(RwLock::new(InnerHashSetCache { sets, count: 0 })),
            bucket_capacity,
            max_bucket_count,
        }
    }

    pub fn except_with<I>(&self, items: I)
    where I: IntoIterator<Item = T> {
        let mut inner = self.inner.write().unwrap();
        for item in items {
            for set in inner.sets.iter_mut() {
                if set.remove(&item) {
                    inner.count -= 1;
                    break;
                }
            }
        }
        let non_empty: Vec<_> = inner.sets.iter().cloned().filter(|set| !set.is_empty()).collect();
        inner.sets.clear();
        inner.sets.extend(non_empty);
    }
}

impl<T> Clone for HashSetCache<T>
where T: Eq + Hash + Clone
{
    fn clone(&self) -> Self {
        HashSetCache {
            inner: self.inner.clone(),
            bucket_capacity: self.bucket_capacity,
            max_bucket_count: self.max_bucket_count,
        }
    }
}
