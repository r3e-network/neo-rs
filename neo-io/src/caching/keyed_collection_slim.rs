#![feature(linked_list_retain)]

use std::cmp::Eq;
use std::collections::{HashMap, LinkedList};
use std::hash::Hash;

pub trait KeyedCollectionSlim<K, V>
where
    K: Eq + Hash,
    V: PartialEq + Ord + Clone,
{
    fn get_key_for_item(&self, item: &V) -> K;

    fn add(&mut self, item: V) -> Result<(), String>;
    fn contains(&self, key: &K) -> bool;
    fn remove(&mut self, key: &K);
    fn remove_first(&mut self);
}

pub struct KeyedCollectionSlimImpl<K, V>
where
    K: Eq + Hash,
    V: PartialEq + Ord + Clone,
{
    items: LinkedList<V>,
    dict: HashMap<K, V>,
}

impl<K, V> KeyedCollectionSlimImpl<K, V>
where
    K: Eq + Hash,
    V: PartialEq + Ord + Clone,
{
    pub fn new() -> Self {
        KeyedCollectionSlimImpl { items: LinkedList::new(), dict: HashMap::new() }
    }

    pub fn count(&self) -> usize {
        self.dict.len()
    }

    pub fn first(&self) -> Option<&V> {
        self.items.front()
    }
}

impl<K, V> KeyedCollectionSlim<K, V> for KeyedCollectionSlimImpl<K, V>
where
    K: Eq + Hash,
    V: PartialEq + Ord + Clone,
{
    fn get_key_for_item(&self, item: &V) -> K {
        // This method needs to be implemented by the user of this trait
        unimplemented!("get_key_for_item must be implemented")
    }

    fn add(&mut self, item: V) -> Result<(), String> {
        let key = self.get_key_for_item(&item);
        if self.dict.contains_key(&key) {
            return Err(
                "An element with the same key already exists in the collection.".to_string()
            );
        }
        self.items.push_back(item.clone());
        self.dict.insert(key, item);
        Ok(())
    }

    fn contains(&self, key: &K) -> bool {
        self.dict.contains_key(key)
    }

    fn remove(&mut self, key: &K) {
        if let Some(item_to_remove) = self.dict.remove(key) {
            let mut new_items = LinkedList::new();
            while let Some(item) = self.items.pop_front() {
                if item != item_to_remove {
                    new_items.push_back(item);
                }
            }
            self.items = new_items;
        }
    }

    fn remove_first(&mut self) {
        if let Some(first) = self.items.pop_front() {
            let key = self.get_key_for_item(&first);
            self.dict.remove(&key);
        }
    }
}
