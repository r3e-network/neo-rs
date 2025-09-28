// Copyright (C) 2015-2025 The Neo Project.
//
// hash_set_extensions.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use std::collections::{HashSet, HashMap};
use std::hash::Hash;

/// HashSet extensions matching C# HashSetExtensions exactly
pub trait HashSetExtensions<T> {
    /// Removes elements from the set that are in the other set.
    /// Matches C# Remove method with ISet<T>
    fn remove_set(&mut self, other: &HashSet<T>)
    where
        T: Hash + Eq + Clone;
    
    /// Removes elements from the set that are in the other collection.
    /// Matches C# Remove method with ICollection<T>
    fn remove_collection(&mut self, other: &[T])
    where
        T: Hash + Eq + Clone;
    
    /// Removes elements from the set that are keys in the other dictionary.
    /// Matches C# Remove method with IReadOnlyDictionary<T, V>
    fn remove_dict_keys<K, V>(&mut self, other: &HashMap<K, V>)
    where
        T: Hash + Eq + Clone,
        K: Hash + Eq + Clone + PartialEq<T>;
}

impl<T> HashSetExtensions<T> for HashSet<T>
where
    T: Hash + Eq + Clone,
{
    fn remove_set(&mut self, other: &HashSet<T>) {
        if self.len() > other.len() {
            self.retain(|item| !other.contains(item));
        } else {
            self.retain(|item| !other.contains(item));
        }
    }
    
    fn remove_collection(&mut self, other: &[T]) {
        if self.len() > other.len() {
            self.retain(|item| !other.contains(item));
        } else {
            self.retain(|item| !other.contains(item));
        }
    }
    
    fn remove_dict_keys<K, V>(&mut self, other: &HashMap<K, V>)
    where
        T: Hash + Eq + Clone,
        K: Hash + Eq + Clone + PartialEq<T>,
    {
        if self.len() > other.len() {
            self.retain(|item| !other.keys().any(|key| key == item));
        } else {
            self.retain(|item| !other.keys().any(|key| key == item));
        }
    }
}
