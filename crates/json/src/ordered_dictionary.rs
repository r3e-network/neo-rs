//! OrderedDictionary - matches C# Neo.Json.OrderedDictionary exactly

use std::collections::HashMap;

/// Ordered dictionary that maintains insertion order (matches C# OrderedDictionary<TKey, TValue>)
#[derive(Clone, Debug, PartialEq)]
pub struct OrderedDictionary<K: Clone + Eq + std::hash::Hash, V> {
    pub(crate) items: Vec<(K, V)>,
    index_map: HashMap<K, usize>,
}

impl<K: Clone + Eq + std::hash::Hash, V> OrderedDictionary<K, V> {
    /// Creates a new ordered dictionary
    pub fn new() -> Self {
        Self {
            items: Vec::new(),
            index_map: HashMap::new(),
        }
    }

    /// Gets count
    pub fn count(&self) -> usize {
        self.items.len()
    }

    /// Gets keys
    pub fn keys(&self) -> Vec<&K> {
        self.items.iter().map(|(k, _)| k).collect()
    }

    /// Gets values
    pub fn values(&self) -> Vec<&V> {
        self.items.iter().map(|(_, v)| v).collect()
    }

    /// Gets value by key
    pub fn get<Q>(&self, key: &Q) -> Option<&V>
    where
        K: std::borrow::Borrow<Q>,
        Q: ?Sized + Eq + std::hash::Hash,
    {
        self.index_map.get(key).map(|&idx| &self.items[idx].1)
    }

    /// Gets mutable value by key
    pub fn get_mut(&mut self, key: &K) -> Option<&mut V> {
        if let Some(&idx) = self.index_map.get(key) {
            Some(&mut self.items[idx].1)
        } else {
            None
        }
    }

    /// Gets value by index
    pub fn get_at(&self, index: usize) -> Option<&V> {
        self.items.get(index).map(|(_, v)| v)
    }

    /// Inserts or updates a key-value pair
    pub fn insert(&mut self, key: K, value: V) {
        if let Some(&idx) = self.index_map.get(&key) {
            self.items[idx].1 = value;
        } else {
            let idx = self.items.len();
            self.index_map.insert(key.clone(), idx);
            self.items.push((key, value));
        }
    }

    /// Adds a key-value pair
    pub fn add(&mut self, key: K, value: V) -> bool {
        if self.index_map.contains_key(&key) {
            false
        } else {
            let idx = self.items.len();
            self.index_map.insert(key.clone(), idx);
            self.items.push((key, value));
            true
        }
    }

    /// Removes by key
    pub fn remove(&mut self, key: &K) -> bool {
        if let Some(&idx) = self.index_map.get(key) {
            self.items.remove(idx);
            self.index_map.remove(key);
            // Update indices for items after the removed one
            for (i, (k, _)) in self.items.iter().enumerate().skip(idx) {
                self.index_map.insert(k.clone(), i);
            }
            true
        } else {
            false
        }
    }

    /// Checks if contains key
    pub fn contains_key<Q>(&self, key: &Q) -> bool
    where
        K: std::borrow::Borrow<Q>,
        Q: ?Sized + Eq + std::hash::Hash,
    {
        self.index_map.contains_key(key)
    }

    /// Clears all items
    pub fn clear(&mut self) {
        self.items.clear();
        self.index_map.clear();
    }

    /// Gets iterator
    pub fn iter(&self) -> impl Iterator<Item = (&K, &V)> {
        self.items.iter().map(|(k, v)| (k, v))
    }

    /// Try get value
    pub fn try_get_value(&self, key: &K) -> Option<&V> {
        self.get(key)
    }
}

impl<K: Clone + Eq + std::hash::Hash, V> Default for OrderedDictionary<K, V> {
    fn default() -> Self {
        Self::new()
    }
}
