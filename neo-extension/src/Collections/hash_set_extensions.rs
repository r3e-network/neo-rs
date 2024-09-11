use std::collections::{HashMap, HashSet};
use std::hash::Hash;

pub trait HashSetExtensions<T: Eq + Hash> {
    fn remove_set(&mut self, other: &HashSet<T>);
    fn remove_dict<V>(&mut self, other: &HashMap<T, V>);
}

impl<T: Eq + Hash> HashSetExtensions<T> for HashSet<T> {
    fn remove_set(&mut self, other: &HashSet<T>) {
        if self.len() > other.len() {
            self.retain(|item| !other.contains(item));
        } else {
            self.retain(|item| !other.contains(item));
        }
    }

    fn remove_dict<V>(&mut self, other: &HashMap<T, V>) {
        if self.len() > other.len() {
            self.retain(|item| !other.contains_key(item));
        } else {
            self.retain(|item| !other.contains_key(item));
        }
    }
}
