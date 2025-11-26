use std::borrow::Borrow;
use std::collections::HashMap;
use std::hash::Hash;

/// HashMap extensions aligned with the C# Neo.Extensions helpers.
pub trait HashMapExtensions<K, V> {
    /// Get a mutable reference for `key`, inserting `Default` if missing.
    fn get_or_insert_default(&mut self, key: K) -> &mut V
    where
        V: Default;

    /// Merge another map, overwriting existing keys with values from `other`.
    fn merge(&mut self, other: HashMap<K, V>);

    /// Batch lookup helper returning a parallel Vec of options.
    fn get_many<Q>(&self, keys: &[Q]) -> Vec<Option<&V>>
    where
        K: Borrow<Q>,
        Q: Hash + Eq;

    /// Filter entries by value, returning a new HashMap.
    fn filter_values<F>(&self, predicate: F) -> HashMap<K, V>
    where
        F: Fn(&V) -> bool,
        K: Clone,
        V: Clone;
}

impl<K, V> HashMapExtensions<K, V> for HashMap<K, V>
where
    K: Eq + Hash,
{
    fn get_or_insert_default(&mut self, key: K) -> &mut V
    where
        V: Default,
    {
        self.entry(key).or_insert_with(Default::default)
    }

    fn merge(&mut self, other: HashMap<K, V>) {
        self.extend(other.into_iter());
    }

    fn get_many<Q>(&self, keys: &[Q]) -> Vec<Option<&V>>
    where
        K: Borrow<Q>,
        Q: Hash + Eq,
    {
        keys.iter().map(|key| self.get(key)).collect()
    }

    fn filter_values<F>(&self, predicate: F) -> HashMap<K, V>
    where
        F: Fn(&V) -> bool,
        K: Clone,
        V: Clone,
    {
        self.iter()
            .filter(|(_, value)| predicate(value))
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect()
    }
}
