#![feature(linked_list_retain)]

use std::cmp::Eq;
use std::collections::{HashMap, LinkedList};
use std::hash::Hash;
use std::hash::Hash;

pub trait KeyedCollectionSlim<K, V>
where
    K: Eq + Hash,
    V: PartialEq + Ord,
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
    V: PartialEq + Ord,
{
    items: LinkedList<V>,
    dict:  HashMap<K, *mut LinkedList<V>>,
}

impl<K, V> KeyedCollectionSlimImpl<K, V>
where
    K: Eq + Hash,
    V: PartialEq + Ord,
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
    V: PartialEq + Ord,
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
        self.items.push_back(item);
        let node = self.items.back_mut().unwrap() as *mut V;
        self.dict.insert(key, node);
        Ok(())
    }

    fn contains(&self, key: &K) -> bool {
        self.dict.contains_key(key)
    }

    fn remove(&mut self, key: &K) {
        if let Some(node) = self.dict.remove(key) {
            unsafe {
                self.items.retain(|item| item as *const _ != node);
            }
        }
    }

    fn remove_first(&mut self) {
        if let Some(first) = self.items.pop_front() {
            let key = self.get_key_for_item(&first);
            self.dict.remove(&key);
        }
    }
}
