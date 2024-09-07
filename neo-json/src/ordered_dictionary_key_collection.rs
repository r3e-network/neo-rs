
use std::collections::BTreeMap;
use std::iter::Iterator;

pub struct OrderedDictionary<K, V> {
    internal_collection: BTreeMap<K, V>,
}

impl<K: Ord, V> OrderedDictionary<K, V> {
    pub struct KeyCollection<'a, K: 'a, V: 'a> {
        internal_collection: &'a BTreeMap<K, V>,
    }

    impl<'a, K: Ord, V> KeyCollection<'a, K, V> {
        pub fn new(internal_collection: &'a BTreeMap<K, V>) -> Self {
            KeyCollection { internal_collection }
        }

        pub fn get(&self, index: usize) -> Option<&K> {
            self.internal_collection.keys().nth(index)
        }

        pub fn len(&self) -> usize {
            self.internal_collection.len()
        }

        pub fn is_empty(&self) -> bool {
            self.internal_collection.is_empty()
        }

        pub fn contains(&self, key: &K) -> bool {
            self.internal_collection.contains_key(key)
        }

        pub fn copy_to(&self, array: &mut [K], array_index: usize) 
        where
            K: Clone,
        {
            for (i, key) in self.internal_collection.keys().enumerate() {
                if i + array_index < array.len() {
                    array[i + array_index] = key.clone();
                } else {
                    break;
                }
            }
        }

        pub fn iter(&self) -> impl Iterator<Item = &K> {
            self.internal_collection.keys()
        }
    }

    impl<'a, K: Ord, V> IntoIterator for &'a KeyCollection<'a, K, V> {
        type Item = &'a K;
        type IntoIter = std::collections::btree_map::Keys<'a, K, V>;

        fn into_iter(self) -> Self::IntoIter {
            self.internal_collection.keys()
        }
    }
}
