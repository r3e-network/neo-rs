use std::collections::{HashMap, HashSet};
use std::hash::Hash;

/// Static collection helpers mirroring the C# Collections utilities.
pub struct Collections;

impl Collections {
    pub fn hashmap_from_pairs<K, V, I>(pairs: I) -> HashMap<K, V>
    where
        K: Eq + Hash,
        I: IntoIterator<Item = (K, V)>,
    {
        pairs.into_iter().collect()
    }

    pub fn hashset_from_values<T, I>(values: I) -> HashSet<T>
    where
        T: Eq + Hash,
        I: IntoIterator<Item = T>,
    {
        values.into_iter().collect()
    }

    pub fn group_by<T, K, I, F>(items: I, key_selector: F) -> HashMap<K, Vec<T>>
    where
        I: IntoIterator<Item = T>,
        K: Eq + Hash,
        F: Fn(&T) -> K,
    {
        let mut groups: HashMap<K, Vec<T>> = HashMap::new();

        for item in items {
            let key = key_selector(&item);
            groups.entry(key).or_default().push(item);
        }

        groups
    }

    pub fn count_occurrences<T>(items: &[T]) -> HashMap<T, usize>
    where
        T: Eq + Hash + Clone,
    {
        let mut counts: HashMap<T, usize> = HashMap::new();

        for item in items {
            *counts.entry(item.clone()).or_insert(0) += 1;
        }

        counts
    }

    pub fn most_common<T>(items: &[T]) -> Option<T>
    where
        T: Eq + Hash + Clone,
    {
        let mut counts: HashMap<T, usize> = HashMap::new();
        let mut best: Option<(T, usize)> = None;

        for item in items {
            let counter = counts.entry(item.clone()).or_insert(0);
            *counter += 1;

            if best
                .as_ref()
                .map_or(true, |(_, best_count)| *counter > *best_count)
            {
                best = Some((item.clone(), *counter));
            }
        }

        best.map(|(value, _)| value)
    }

    pub fn partition<T, I, F>(items: I, predicate: F) -> (Vec<T>, Vec<T>)
    where
        I: IntoIterator<Item = T>,
        F: Fn(&T) -> bool,
    {
        let mut matched = Vec::new();
        let mut missed = Vec::new();

        for item in items {
            if predicate(&item) {
                matched.push(item);
            } else {
                missed.push(item);
            }
        }

        (matched, missed)
    }
}
