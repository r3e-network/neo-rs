//! `KeyedCollectionSlim` - faithful port of Neo.IO.Caching.KeyedCollectionSlim

use indexmap::IndexMap;
use std::hash::Hash;
use std::sync::Arc;

/// Signature for retrieving the key associated with a collection item.
pub type KeySelector<TKey, TItem> = Arc<dyn Fn(&TItem) -> TKey + Send + Sync>;

/// A slimmed down keyed collection mirroring C# `KeyedCollectionSlim<TKey, TItem>`.
pub struct KeyedCollectionSlim<TKey, TItem>
where
    TKey: Eq + Hash,
{
    items: IndexMap<TKey, TItem>,
    key_selector: KeySelector<TKey, TItem>,
}

impl<TKey, TItem> KeyedCollectionSlim<TKey, TItem>
where
    TKey: Eq + Hash,
{
    /// Initializes a new instance with the specified initial capacity and key selector.
    pub fn new(initial_capacity: usize, key_selector: KeySelector<TKey, TItem>) -> Self {
        Self {
            items: IndexMap::with_capacity(initial_capacity),
            key_selector,
        }
    }

    /// Convenience constructor mirroring C# subclass overrides by accepting a closure.
    pub fn with_selector(
        initial_capacity: usize,
        selector: impl Fn(&TItem) -> TKey + Send + Sync + 'static,
    ) -> Self {
        Self::new(initial_capacity, Arc::new(selector))
    }

    /// Total number of items stored in the collection (C# `Count`).
    #[inline]
    #[must_use] 
    pub fn count(&self) -> usize {
        self.items.len()
    }

    /// Returns the first item or `None` if the collection is empty (C# `FirstOrDefault`).
    #[inline]
    #[must_use] 
    pub fn first_or_default(&self) -> Option<&TItem> {
        self.items.first().map(|(_, value)| value)
    }

    /// Adds a new item when its key is not already present (C# `TryAdd`).
    pub fn try_add(&mut self, item: TItem) -> bool {
        let key = (self.key_selector)(&item);
        if self.items.contains_key(&key) {
            return false;
        }
        self.items.insert(key, item);
        true
    }

    /// Checks whether the specified key is present (C# `Contains`).
    #[inline]
    pub fn contains(&self, key: &TKey) -> bool {
        self.items.contains_key(key)
    }

    /// Removes the item associated with the key (C# `Remove`).
    #[inline]
    pub fn remove(&mut self, key: &TKey) -> bool {
        self.items.shift_remove(key).is_some()
    }

    /// Removes the first item from the collection (C# `RemoveFirst`).
    pub fn remove_first(&mut self) -> bool {
        if self.items.is_empty() {
            return false;
        }
        self.items.shift_remove_index(0);
        true
    }

    /// Removes all items (C# `Clear`). Rebuilds the map to avoid the linear clear cost.
    #[inline]
    pub fn clear(&mut self) {
        let capacity = self.items.capacity();
        self.items = IndexMap::with_capacity(capacity);
    }

    /// Returns an iterator over the values (C# `GetEnumerator`).
    #[inline]
    pub fn iter(&self) -> impl Iterator<Item = &TItem> {
        self.items.values()
    }
}

impl<'a, TKey, TItem> IntoIterator for &'a KeyedCollectionSlim<TKey, TItem>
where
    TKey: Eq + Hash,
{
    type Item = &'a TItem;
    type IntoIter = indexmap::map::Values<'a, TKey, TItem>;

    fn into_iter(self) -> Self::IntoIter {
        self.items.values()
    }
}
