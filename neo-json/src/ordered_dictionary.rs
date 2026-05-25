//! `OrderedDictionary` - matches C# Neo.Json.OrderedDictionary exactly

use indexmap::IndexMap;

/// Ordered dictionary that maintains insertion order (matches C# `OrderedDictionary`<`TKey`, `TValue`>)
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OrderedDictionary<K: Clone + Eq + std::hash::Hash, V> {
    pub(crate) items: IndexMap<K, V>,
}

impl<K: Clone + Eq + std::hash::Hash, V> OrderedDictionary<K, V> {
    /// Creates a new ordered dictionary
    #[must_use]
    pub fn new() -> Self {
        Self {
            items: IndexMap::new(),
        }
    }

    /// Gets count
    #[must_use]
    pub fn count(&self) -> usize {
        self.items.len()
    }

    /// Gets keys as an iterator.
    ///
    /// Returns `impl Iterator` to avoid unnecessary Vec allocation.
    /// Caller can call `.collect()` if they need a Vec.
    pub fn keys(&self) -> impl Iterator<Item = &K> + '_ {
        self.items.keys()
    }

    /// Gets values as an iterator.
    ///
    /// Returns `impl Iterator` to avoid unnecessary Vec allocation.
    /// Caller can call `.collect()` if they need a Vec.
    pub fn values(&self) -> impl Iterator<Item = &V> + '_ {
        self.items.values()
    }

    /// Gets value by key
    pub fn get<Q>(&self, key: &Q) -> Option<&V>
    where
        K: std::borrow::Borrow<Q>,
        Q: ?Sized + Eq + std::hash::Hash,
    {
        self.items.get(key)
    }

    /// Gets mutable value by key
    pub fn get_mut(&mut self, key: &K) -> Option<&mut V> {
        self.items.get_mut(key)
    }

    /// Gets value by index
    #[must_use]
    pub fn get_at(&self, index: usize) -> Option<&V> {
        self.items.get_index(index).map(|(_, v)| v)
    }

    /// Inserts or updates a key-value pair
    pub fn insert(&mut self, key: K, value: V) {
        self.items.insert(key, value);
    }

    /// Adds a key-value pair
    pub fn add(&mut self, key: K, value: V) -> bool {
        if self.items.contains_key(&key) {
            false
        } else {
            self.items.insert(key, value);
            true
        }
    }

    /// Removes by key
    pub fn remove(&mut self, key: &K) -> bool {
        self.items.shift_remove(key).is_some()
    }

    /// Checks if contains key
    pub fn contains_key<Q>(&self, key: &Q) -> bool
    where
        K: std::borrow::Borrow<Q>,
        Q: ?Sized + Eq + std::hash::Hash,
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

#[cfg(test)]
mod tests {
    use super::OrderedDictionary;

    #[test]
    fn insert_updates_without_moving_existing_key() {
        let mut dict = OrderedDictionary::new();

        dict.insert("a", 1);
        dict.insert("b", 2);
        dict.insert("a", 3);

        assert_eq!(dict.keys().copied().collect::<Vec<_>>(), vec!["a", "b"]);
        assert_eq!(dict.values().copied().collect::<Vec<_>>(), vec![3, 2]);
    }

    #[test]
    fn add_rejects_duplicates_without_moving_existing_key() {
        let mut dict = OrderedDictionary::new();

        assert!(dict.add("a", 1));
        assert!(dict.add("b", 2));
        assert!(!dict.add("a", 3));

        assert_eq!(dict.keys().copied().collect::<Vec<_>>(), vec!["a", "b"]);
        assert_eq!(dict.get(&"a"), Some(&1));
    }

    #[test]
    fn remove_preserves_remaining_insertion_order() {
        let mut dict = OrderedDictionary::new();

        dict.insert("a", 1);
        dict.insert("b", 2);
        dict.insert("c", 3);

        assert!(dict.remove(&"b"));
        assert_eq!(dict.keys().copied().collect::<Vec<_>>(), vec!["a", "c"]);
        assert_eq!(dict.get_at(0), Some(&1));
        assert_eq!(dict.get_at(1), Some(&3));
        assert_eq!(dict.get_at(2), None);
    }
}
