//! Ordered dictionary implementation for the Neo VM.
//!
//! Port of `Neo.VM/Collections/OrderedDictionary.cs`. The C# version keeps insertion
//! order by layering a dictionary on top of a list. In Rust we mirror those semantics
//! with a lightweight Vec-backed structure that preserves insertion order while
//! providing dictionary-like access.

use std::collections::BTreeMap;
use std::fmt::{self, Debug};

/// Ordered dictionary keeping keys in insertion order while offering key-based lookup.
#[derive(Clone)]
pub struct VmOrderedDictionary<K, V>
where
    K: PartialEq,
{
    entries: Vec<(K, V)>,
}

impl<K, V> VmOrderedDictionary<K, V>
where
    K: PartialEq,
{
    /// Creates an empty ordered dictionary.
    #[must_use] 
    pub const fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    /// Creates an ordered dictionary with the specified capacity.
    #[must_use] 
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            entries: Vec::with_capacity(capacity),
        }
    }

    /// Returns the number of stored entries.
    #[must_use] 
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Returns `true` when the dictionary has no entries.
    #[must_use] 
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Returns an immutable reference to the value mapped to `key`.
    pub fn get(&self, key: &K) -> Option<&V> {
        self.entries.iter().find(|(k, _)| k == key).map(|(_, v)| v)
    }

    /// Returns a mutable reference to the value mapped to `key`.
    pub fn get_mut(&mut self, key: &K) -> Option<&mut V> {
        self.entries
            .iter_mut()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v)
    }

    /// Inserts the value for the given key, returning the previous value if present.
    pub fn insert(&mut self, key: K, value: V) -> Option<V> {
        if let Some((_, existing)) = self.entries.iter_mut().find(|(k, _)| *k == key) {
            return Some(std::mem::replace(existing, value));
        }
        self.entries.push((key, value));
        None
    }

    /// Removes the entry for the specified key and returns its value if present.
    pub fn remove(&mut self, key: &K) -> Option<V> {
        if let Some(index) = self.entries.iter().position(|(k, _)| k == key) {
            Some(self.entries.remove(index).1)
        } else {
            None
        }
    }

    /// Clears the dictionary, removing every entry.
    pub fn clear(&mut self) {
        self.entries.clear();
    }

    /// Checks whether the dictionary contains the provided key.
    pub fn contains_key(&self, key: &K) -> bool {
        self.entries.iter().any(|(k, _)| k == key)
    }

    /// Returns an iterator over the key/value pairs in insertion order.
    #[must_use] 
    pub fn iter(&self) -> impl DoubleEndedIterator<Item = (&K, &V)> + ExactSizeIterator {
        self.entries.iter().map(|(k, v)| (k, v))
    }

    /// Returns a mutable iterator over the key/value pairs in insertion order.
    pub fn iter_mut(
        &mut self,
    ) -> impl DoubleEndedIterator<Item = (&K, &mut V)> + ExactSizeIterator {
        self.entries.iter_mut().map(|(k, v)| (&*k, v))
    }

    /// Returns an iterator over the keys in insertion order.
    pub fn keys(&self) -> impl Iterator<Item = &K> {
        self.entries.iter().map(|(k, _)| k)
    }

    /// Returns an iterator over the values in insertion order.
    pub fn values(&self) -> impl Iterator<Item = &V> {
        self.entries.iter().map(|(_, v)| v)
    }
}

impl<K, V> From<Vec<(K, V)>> for VmOrderedDictionary<K, V>
where
    K: PartialEq,
{
    fn from(entries: Vec<(K, V)>) -> Self {
        Self { entries }
    }
}

impl<K, V> From<BTreeMap<K, V>> for VmOrderedDictionary<K, V>
where
    K: Ord + PartialEq,
{
    fn from(map: BTreeMap<K, V>) -> Self {
        let entries = map.into_iter().collect::<Vec<_>>();
        Self { entries }
    }
}

impl<K, V> IntoIterator for VmOrderedDictionary<K, V>
where
    K: PartialEq,
{
    type Item = (K, V);
    type IntoIter = std::vec::IntoIter<(K, V)>;

    fn into_iter(self) -> Self::IntoIter {
        self.entries.into_iter()
    }
}

const fn entry_to_ref<K, V>(entry: &(K, V)) -> (&K, &V) {
    (&entry.0, &entry.1)
}

fn entry_to_ref_mut<K, V>(entry: &mut (K, V)) -> (&K, &mut V) {
    let (key, value) = entry;
    (key, value)
}

impl<'a, K, V> IntoIterator for &'a VmOrderedDictionary<K, V>
where
    K: PartialEq,
{
    type Item = (&'a K, &'a V);
    type IntoIter = std::iter::Map<std::slice::Iter<'a, (K, V)>, fn(&(K, V)) -> (&K, &V)>;

    fn into_iter(self) -> Self::IntoIter {
        self.entries.iter().map(entry_to_ref::<K, V>)
    }
}

impl<'a, K, V> IntoIterator for &'a mut VmOrderedDictionary<K, V>
where
    K: PartialEq,
{
    type Item = (&'a K, &'a mut V);
    type IntoIter =
        std::iter::Map<std::slice::IterMut<'a, (K, V)>, fn(&mut (K, V)) -> (&K, &mut V)>;

    fn into_iter(self) -> Self::IntoIter {
        self.entries.iter_mut().map(entry_to_ref_mut::<K, V>)
    }
}

impl<K, V> Default for VmOrderedDictionary<K, V>
where
    K: PartialEq,
{
    fn default() -> Self {
        Self::new()
    }
}

impl<K, V> Debug for VmOrderedDictionary<K, V>
where
    K: PartialEq + Debug,
    V: Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_map()
            .entries(self.entries.iter().map(|(k, v)| (k, v)))
            .finish()
    }
}
