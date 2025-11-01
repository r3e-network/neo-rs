//! OrderedDictionary.KeyCollection - matches C# Neo.Json.OrderedDictionary.KeyCollection exactly

/// Key collection for OrderedDictionary (matches C# nested KeyCollection class)
pub struct KeyCollection<'a, K: Clone + Eq + std::hash::Hash, V> {
    dict: &'a crate::ordered_dictionary::OrderedDictionary<K, V>,
}

impl<'a, K: Clone + Eq + std::hash::Hash, V> KeyCollection<'a, K, V> {
    /// Creates a new key collection
    pub fn new(dict: &'a crate::ordered_dictionary::OrderedDictionary<K, V>) -> Self {
        Self { dict }
    }

    /// Gets count
    pub fn count(&self) -> usize {
        self.dict.count()
    }

    /// Gets key at index
    pub fn get(&self, index: usize) -> Option<&K> {
        self.dict.items.get(index).map(|(k, _)| k)
    }

    /// Checks if contains key
    pub fn contains(&self, key: &K) -> bool {
        self.dict.contains_key(key)
    }

    /// Gets iterator
    pub fn iter(&self) -> impl Iterator<Item = &K> + '_ {
        self.dict.keys().into_iter()
    }
}
