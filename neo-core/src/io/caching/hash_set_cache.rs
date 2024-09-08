
use std::collections::{HashSet, LinkedList};
use std::hash::Hash;

pub struct HashSetCache<T>
where
    T: Eq + Hash,
{
    /// Sets where the Hashes are stored
    sets: LinkedList<HashSet<T>>,

    /// Maximum capacity of each bucket inside each HashSet of `sets`.
    bucket_capacity: usize,

    /// Maximum number of buckets for the LinkedList, meaning its maximum cardinality.
    max_bucket_count: usize,

    /// Entry count
    count: usize,
}

impl<T> HashSetCache<T>
where
    T: Eq + Hash,
{
    pub fn new(bucket_capacity: usize, max_bucket_count: usize) -> Self {
        if bucket_capacity == 0 {
            panic!("bucket_capacity should be greater than 0");
        }
        if max_bucket_count == 0 {
            panic!("max_bucket_count should be greater than 0");
        }

        let mut sets = LinkedList::new();
        sets.push_front(HashSet::new());

        Self {
            sets,
            bucket_capacity,
            max_bucket_count,
            count: 0,
        }
    }

    pub fn add(&mut self, item: T) -> bool {
        if self.contains(&item) {
            return false;
        }

        self.count += 1;

        if let Some(first) = self.sets.front_mut() {
            if first.len() < self.bucket_capacity {
                return first.insert(item);
            }
        }

        let mut new_set = HashSet::new();
        new_set.insert(item);
        self.sets.push_front(new_set);

        if self.sets.len() > self.max_bucket_count {
            if let Some(last) = self.sets.pop_back() {
                self.count -= last.len();
            }
        }

        true
    }

    pub fn contains(&self, item: &T) -> bool {
        self.sets.iter().any(|set| set.contains(item))
    }

    pub fn except_with<I>(&mut self, items: I)
    where
        I: IntoIterator<Item = T>,
    {
        let mut remove_list = Vec::new();

        for item in items {
            for (index, set) in self.sets.iter_mut().enumerate() {
                if set.remove(&item) {
                    self.count -= 1;
                    if set.is_empty() {
                        remove_list.push(index);
                    }
                    break;
                }
            }
        }

        for index in remove_list.into_iter().rev() {
            let mut cursor = self.sets.cursor_front_mut();
            for _ in 0..index {
                cursor.move_next();
            }
            cursor.remove_current();
        }
    }

    pub fn len(&self) -> usize {
        self.count
    }

    pub fn is_empty(&self) -> bool {
        self.count == 0
    }
}

impl<T> IntoIterator for HashSetCache<T>
where
    T: Eq + Hash,
{
    type Item = T;
    type IntoIter = HashSetCacheIntoIter<T>;

    fn into_iter(self) -> Self::IntoIter {
        HashSetCacheIntoIter {
            sets: self.sets.into_iter(),
            current_set_iter: None,
        }
    }
}

pub struct HashSetCacheIntoIter<T> {
    sets: std::collections::linked_list::IntoIter<HashSet<T>>,
    current_set_iter: Option<std::collections::hash_set::IntoIter<T>>,
}

impl<T> Iterator for HashSetCacheIntoIter<T> {
    type Item = T;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if let Some(ref mut current_iter) = self.current_set_iter {
                if let Some(item) = current_iter.next() {
                    return Some(item);
                }
            }
            
            if let Some(next_set) = self.sets.next() {
                self.current_set_iter = Some(next_set.into_iter());
            } else {
                return None;
            }
        }
    }
}
