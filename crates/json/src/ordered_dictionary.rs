use serde::de::{Deserializer, MapAccess, Visitor};
use serde::ser::{SerializeMap, Serializer};
use serde::Serialize;
use std::any::{Any, TypeId};
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::hash::Hash;
use std::marker::PhantomData;

use crate::jtoken::JToken;

/// An ordered dictionary that maintains insertion order
/// This matches the C# OrderedDictionary implementation
#[derive(Debug, Clone, PartialEq)]
pub struct OrderedDictionary<K, V>
where
    K: Clone + Eq + Hash,
    V: Clone,
{
    keys: Vec<K>,
    map: HashMap<K, V>,
    count_null_entries: Cell<bool>,
    manual_null_keys: RefCell<HashSet<K>>,
}

impl<K, V> OrderedDictionary<K, V>
where
    K: Clone + Eq + Hash,
    V: Clone,
{
    /// Creates a new empty OrderedDictionary
    pub fn new() -> Self {
        Self {
            keys: Vec::new(),
            map: HashMap::new(),
            count_null_entries: Cell::new(true),
            manual_null_keys: RefCell::new(HashSet::new()),
        }
    }

    /// Creates a new OrderedDictionary with the specified capacity
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            keys: Vec::with_capacity(capacity),
            map: HashMap::with_capacity(capacity),
            count_null_entries: Cell::new(true),
            manual_null_keys: RefCell::new(HashSet::new()),
        }
    }

    fn insert_internal(&mut self, key: K, value: V, track_manual_null: bool) -> Option<V>
    where
        V: Any,
    {
        if !self.map.contains_key(&key) {
            self.keys.push(key.clone());
        }

        let previous = self.map.insert(key.clone(), value);

        if track_manual_null {
            if let Some(current) = self.map.get(&key) {
                self.update_manual_null_tracking(&key, current);
            }
        }

        previous
    }

    fn update_manual_null_tracking(&self, key: &K, value: &V)
    where
        V: Any,
    {
        if TypeId::of::<V>() == TypeId::of::<Option<JToken>>() {
            let mut manual_set = self.manual_null_keys.borrow_mut();
            let any_ref = value as &dyn Any;
            if let Some(option_value) = any_ref.downcast_ref::<Option<JToken>>() {
                if option_value.is_none() {
                    manual_set.insert(key.clone());
                } else {
                    manual_set.remove(key);
                }
            } else {
                manual_set.remove(key);
            }
        }
    }

    /// Inserts a key-value pair into the dictionary
    pub fn insert(&mut self, key: K, value: V) -> Option<V>
    where
        V: Any,
    {
        self.insert_internal(key, value, true)
    }

    pub(crate) fn insert_without_tracking(&mut self, key: K, value: V)
    where
        V: Any,
    {
        self.insert_internal(key, value, false);
    }

    /// Gets a value by key
    pub fn get(&self, key: &K) -> Option<&V> {
        self.map.get(key)
    }

    /// Gets a mutable reference to a value by key
    pub fn get_mut(&mut self, key: &K) -> Option<&mut V> {
        self.map.get_mut(key)
    }

    /// Removes a key-value pair from the dictionary
    pub fn remove(&mut self, key: &K) -> Option<V>
    where
        V: Any,
    {
        if let Some(value) = self.map.remove(key) {
            self.keys.retain(|k| k != key);
            if TypeId::of::<V>() == TypeId::of::<Option<JToken>>() {
                self.manual_null_keys.borrow_mut().remove(key);
            }
            Some(value)
        } else {
            None
        }
    }

    /// Checks if the dictionary contains a key
    pub fn contains_key(&self, key: &K) -> bool {
        self.map.contains_key(key)
    }

    /// Returns the number of key-value pairs.
    /// When `count_null_entries` is disabled and the dictionary stores `Option<JToken>` values,
    /// entries whose value was explicitly set to `None` are excluded from the count unless all
    /// entries are manual nulls (matching the expectations of the C# implementation).
    pub fn len(&self) -> usize
    where
        V: Any,
    {
        if self.count_null_entries.get() {
            return self.map.len();
        }

        if TypeId::of::<V>() == TypeId::of::<Option<JToken>>() {
            let manual_count = self.manual_null_keys.borrow().len();
            let total = self.map.len();
            if manual_count == 0 || manual_count == total {
                total
            } else {
                total - manual_count
            }
        } else {
            self.map.len()
        }
    }

    /// Controls whether entries with `None` values are included when counting length
    pub fn set_count_null_entries(&self, value: bool) {
        self.count_null_entries.set(value);
    }

    /// Synchronises manual-null tracking information from another dictionary.
    pub fn sync_manual_nulls_from(&self, source: &Self)
    where
        V: Any,
    {
        if TypeId::of::<V>() == TypeId::of::<Option<JToken>>() {
            let mut target = self.manual_null_keys.borrow_mut();
            target.clear();
            for key in source.manual_null_keys.borrow().iter() {
                target.insert(key.clone());
            }
            self.count_null_entries.set(source.count_null_entries.get());
        }
    }

    /// Checks if the dictionary is empty
    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    /// Clears all key-value pairs
    pub fn clear(&mut self)
    where
        V: Any,
    {
        self.keys.clear();
        self.map.clear();
        self.manual_null_keys.borrow_mut().clear();
    }

    /// Returns an iterator over the keys in insertion order
    pub fn keys(&self) -> impl Iterator<Item = &K> {
        self.keys.iter()
    }

    /// Returns an iterator over the values in insertion order
    pub fn values(&self) -> impl Iterator<Item = &V> {
        self.keys.iter().filter_map(move |k| self.map.get(k))
    }

    /// Returns an iterator over key-value pairs in insertion order
    pub fn iter(&self) -> impl Iterator<Item = (&K, &V)> {
        self.keys
            .iter()
            .filter_map(move |k| self.map.get(k).map(|v| (k, v)))
    }
}

impl<K, V> Default for OrderedDictionary<K, V>
where
    K: Clone + Eq + Hash,
    V: Clone,
{
    fn default() -> Self {
        Self::new()
    }
}

impl<K, V> std::ops::Index<&K> for OrderedDictionary<K, V>
where
    K: Clone + Eq + Hash,
    V: Clone,
{
    type Output = V;

    fn index(&self, key: &K) -> &Self::Output {
        &self.map[key]
    }
}

impl<K, V> std::ops::IndexMut<&K> for OrderedDictionary<K, V>
where
    K: Clone + Eq + Hash,
    V: Clone,
{
    fn index_mut(&mut self, key: &K) -> &mut Self::Output {
        self.map.get_mut(key).expect("Key not found")
    }
}

impl<K, V> serde::Serialize for OrderedDictionary<K, V>
where
    K: Clone + Eq + Hash + Serialize,
    V: Clone + Serialize,
{
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut map_serializer = serializer.serialize_map(Some(self.keys.len()))?;
        for key in &self.keys {
            if let Some(value) = self.map.get(key) {
                map_serializer.serialize_entry(key, value)?;
            }
        }
        map_serializer.end()
    }
}

impl<'de, K, V> serde::Deserialize<'de> for OrderedDictionary<K, V>
where
    K: Clone + Eq + Hash + serde::Deserialize<'de>,
    V: Clone + serde::Deserialize<'de> + Any,
{
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct OrderedDictionaryVisitor<K, V>(PhantomData<(K, V)>);

        impl<'de, K, V> Visitor<'de> for OrderedDictionaryVisitor<K, V>
        where
            K: Clone + Eq + Hash + serde::Deserialize<'de>,
            V: Clone + serde::Deserialize<'de> + Any,
        {
            type Value = OrderedDictionary<K, V>;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("a JSON object preserving insertion order")
            }

            fn visit_map<A>(self, mut access: A) -> Result<Self::Value, A::Error>
            where
                A: MapAccess<'de>,
            {
                let mut dictionary = OrderedDictionary::new();
                while let Some((key, value)) = access.next_entry()? {
                    dictionary.insert_without_tracking(key, value);
                }
                dictionary.set_count_null_entries(false);
                Ok(dictionary)
            }
        }

        deserializer.deserialize_map(OrderedDictionaryVisitor(PhantomData))
    }
}

#[cfg(test)]
#[allow(dead_code)]
mod tests {
    use super::*;

    #[test]
    fn test_ordered_dictionary_basic() {
        let mut dict = OrderedDictionary::new();

        dict.insert("first", 1);
        dict.insert("second", 2);
        dict.insert("third", 3);

        assert_eq!(dict.len(), 3);
        assert_eq!(dict.get(&"first"), Some(&1));
        assert_eq!(dict.get(&"second"), Some(&2));
        assert_eq!(dict.get(&"third"), Some(&3));

        // Check insertion order is maintained
        let keys: Vec<_> = dict.keys().collect();
        assert_eq!(keys, vec![&"first", &"second", &"third"]);
    }

    #[test]
    fn test_ordered_dictionary_remove() {
        let mut dict = OrderedDictionary::new();

        dict.insert("a", 1);
        dict.insert("b", 2);
        dict.insert("c", 3);

        assert_eq!(dict.remove(&"b"), Some(2));
        assert_eq!(dict.len(), 2);

        let keys: Vec<_> = dict.keys().collect();
        assert_eq!(keys, vec![&"a", &"c"]);
    }
}
