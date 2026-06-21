//! `OrderedDictionary` - matches C# Neo.Json.OrderedDictionary exactly

use indexmap::IndexMap;
use std::{borrow::Borrow, hash::Hash};

/// Ordered dictionary that maintains insertion order (matches C# `OrderedDictionary`<`TKey`, `TValue`>)
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OrderedDictionary<K: Eq + Hash, V> {
    pub(crate) items: IndexMap<K, V>,
}

impl<K: Eq + Hash, V> OrderedDictionary<K, V> {
    /// Creates a new ordered dictionary
    #[must_use]
    pub fn new() -> Self {
        Self {
            items: IndexMap::new(),
        }
    }

    /// Returns the number of entries.
    #[must_use]
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Returns true when there are no entries.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Gets value by key
    pub fn get<Q>(&self, key: &Q) -> Option<&V>
    where
        K: Borrow<Q>,
        Q: ?Sized + Eq + Hash,
    {
        self.items.get(key)
    }

    /// Inserts or updates a key-value pair
    pub fn insert(&mut self, key: K, value: V) {
        self.items.insert(key, value);
    }

    /// Inserts a key-value pair, rejecting duplicate keys.
    pub fn try_insert(&mut self, key: K, value: V) -> Result<(), (K, V)> {
        if self.items.contains_key(&key) {
            Err((key, value))
        } else {
            self.items.insert(key, value);
            Ok(())
        }
    }

    /// Removes by key
    pub fn remove(&mut self, key: &K) -> bool {
        self.items.shift_remove(key).is_some()
    }

    /// Checks if contains key
    pub fn contains_key<Q>(&self, key: &Q) -> bool
    where
        K: Borrow<Q>,
        Q: ?Sized + Eq + Hash,
    {
        self.items.contains_key(key)
    }

    /// Clears all items
    pub fn clear(&mut self) {
        self.items.clear();
    }

    /// Gets iterator
    pub fn iter(&self) -> impl Iterator<Item = (&K, &V)> {
        self.items.iter()
    }
}

impl<K: Eq + Hash, V> Default for OrderedDictionary<K, V> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
#[path = "../tests/json/ordered_dictionary.rs"]
mod tests;
