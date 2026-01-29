//! OrderedDictionary.ValueCollection - matches C# Neo.Json.OrderedDictionary.ValueCollection exactly

/// Value collection for `OrderedDictionary` (matches C# nested `ValueCollection` class)
pub struct ValueCollection<'a, K: Clone + Eq + std::hash::Hash, V> {
    dict: &'a crate::ordered_dictionary::OrderedDictionary<K, V>,
}

impl<'a, K: Clone + Eq + std::hash::Hash, V> ValueCollection<'a, K, V> {
    /// Creates a new value collection
    #[must_use] 
    pub const fn new(dict: &'a crate::ordered_dictionary::OrderedDictionary<K, V>) -> Self {
        Self { dict }
    }

    /// Gets count
    #[must_use] 
    pub fn count(&self) -> usize {
        self.dict.count()
    }

    /// Gets value at index
    #[must_use] 
    pub fn get(&self, index: usize) -> Option<&V> {
        self.dict.get_at(index)
    }

    /// Gets iterator
    pub fn iter(&self) -> impl Iterator<Item = &V> + '_ {
        self.dict.values()
    }
}
