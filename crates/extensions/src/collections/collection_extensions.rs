// Copyright (C) 2015-2025 The Neo Project.
//
// collection_extensions.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use std::collections::HashMap;

/// Collection extensions matching C# CollectionExtensions exactly
pub trait CollectionExtensions<K, V> {
    /// Removes the key-value pairs from the dictionary that match the specified predicate.
    /// Matches C# RemoveWhere method
    fn remove_where<F>(&mut self, predicate: F, after_removed: Option<fn(&K, &V)>)
    where
        F: Fn(&K, &V) -> bool;

    /// Chunks the source collection into chunks of the specified size.
    /// Matches C# Chunk method
    fn chunk(&self, chunk_size: usize) -> Result<Vec<Vec<V>>, String>
    where
        V: Clone;
}

impl<K, V> CollectionExtensions<K, V> for HashMap<K, V>
where
    K: Clone + std::hash::Hash + Eq,
    V: Clone,
{
    fn remove_where<F>(&mut self, predicate: F, after_removed: Option<fn(&K, &V)>)
    where
        F: Fn(&K, &V) -> bool,
    {
        let mut items_to_remove = Vec::new();

        for (key, value) in self.iter() {
            if predicate(key, value) {
                items_to_remove.push((key.clone(), value.clone()));
            }
        }

        for (key, value) in items_to_remove {
            if self.remove(&key).is_some() {
                if let Some(callback) = after_removed {
                    callback(&key, &value);
                }
            }
        }
    }

    fn chunk(&self, chunk_size: usize) -> Result<Vec<Vec<V>>, String>
    where
        V: Clone,
    {
        if chunk_size <= 0 {
            return Err("Chunk size must > 0".to_string());
        }

        let values: Vec<V> = self.values().cloned().collect();
        let mut chunks = Vec::new();
        let mut remaining = values.len();
        let mut index = 0;

        while remaining > 0 {
            let chunk_size_actual = remaining.min(chunk_size);
            let chunk = values[index..index + chunk_size_actual].to_vec();
            chunks.push(chunk);
            index += chunk_size_actual;
            remaining -= chunk_size_actual;
        }

        Ok(chunks)
    }
}

impl<T> CollectionExtensions<usize, T> for Vec<T>
where
    T: Clone,
{
    fn remove_where<F>(&mut self, predicate: F, after_removed: Option<fn(&usize, &T)>)
    where
        F: Fn(&usize, &T) -> bool,
    {
        let mut indices_to_remove = Vec::new();

        for (index, item) in self.iter().enumerate() {
            if predicate(&index, item) {
                indices_to_remove.push((index, item.clone()));
            }
        }

        // Remove in reverse order to maintain indices
        for (index, item) in indices_to_remove.iter().rev() {
            if let Some(removed) = self.get(*index) {
                if let Some(callback) = after_removed {
                    callback(index, removed);
                }
            }
            self.remove(*index);
        }
    }

    fn chunk(&self, chunk_size: usize) -> Result<Vec<Vec<T>>, String>
    where
        T: Clone,
    {
        if chunk_size <= 0 {
            return Err("Chunk size must > 0".to_string());
        }

        let mut chunks = Vec::new();
        let mut remaining = self.len();
        let mut index = 0;

        while remaining > 0 {
            let chunk_size_actual = remaining.min(chunk_size);
            let chunk = self[index..index + chunk_size_actual].to_vec();
            chunks.push(chunk);
            index += chunk_size_actual;
            remaining -= chunk_size_actual;
        }

        Ok(chunks)
    }
}
