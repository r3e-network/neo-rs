
use std::collections::BTreeMap;
use std::collections::btree_map::{Keys, Values};

pub struct OrderedDictionary<K: Ord, V> {
    map: BTreeMap<K, V>,
}

impl<K: Ord, V> OrderedDictionary<K, V> {
    pub fn new() -> Self {
        OrderedDictionary {
            map: BTreeMap::new(),
        }
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    pub fn keys(&self) -> Keys<'_, K, V> {
        self.map.keys()
    }

    pub fn values(&self) -> Values<'_, K, V> {
        self.map.values()
    }

    pub fn get(&self, key: &K) -> Option<&V> {
        self.map.get(key)
    }

    pub fn insert(&mut self, key: K, value: V) -> Option<V> {
        self.map.insert(key, value)
    }

    pub fn contains_key(&self, key: &K) -> bool {
        self.map.contains_key(key)
    }

    pub fn remove(&mut self, key: &K) -> Option<V> {
        self.map.remove(key)
    }

    pub fn clear(&mut self) {
        self.map.clear();
    }

    pub fn iter(&self) -> std::collections::btree_map::Iter<'_, K, V> {
        self.map.iter()
    }
}

impl<K: Ord, V> std::ops::Index<&K> for OrderedDictionary<K, V> {
    type Output = V;

    fn index(&self, key: &K) -> &Self::Output {
        &self.map[key]
    }
}

impl<K: Ord, V> std::ops::IndexMut<K> for OrderedDictionary<K, V> {
    fn index_mut(&mut self, key: K) -> &mut Self::Output {
        self.map.entry(key).or_insert_with(|| panic!("Key not found"))
    }
}

impl<K: Ord, V> IntoIterator for OrderedDictionary<K, V> {
    type Item = (K, V);
    type IntoIter = std::collections::btree_map::IntoIter<K, V>;

    fn into_iter(self) -> Self::IntoIter {
        self.map.into_iter()
    }
}

impl<K: Ord, V> FromIterator<(K, V)> for OrderedDictionary<K, V> {
    fn from_iter<T: IntoIterator<Item = (K, V)>>(iter: T) -> Self {
        OrderedDictionary {
            map: iter.into_iter().collect(),
        }
    }
}
