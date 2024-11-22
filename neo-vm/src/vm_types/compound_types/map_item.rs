use alloc::rc::Rc;
use std::cell::RefCell;
use std::hash::{Hash, Hasher};
use crate::compound_types::compound_trait::CompoundTrait;
use crate::stack_item::{SharedItem, StackItem};

/// Map is a reference type that holds a thread-safe collection of MapElements
#[derive(Default, Clone, Debug)]
pub struct MapItem {
    items:Vec<MapElement>,
    ref_count:usize,
    read_only:bool,
}

#[derive(Debug, Clone)]
pub struct MapElement {
    key: SharedItem,
    value: SharedItem,
}

impl CompoundTrait for MapItem {
    fn ref_count(&self) -> usize {
        self.ref_count
    }

    fn ref_inc(&mut self, count:usize) -> usize {
        self.ref_count += count;
        self.ref_count
    }

    fn ref_dec(&mut self, count:usize) -> usize {
        self.ref_count -= count;
        self.ref_count
    }

    fn sub_items(&self) -> Vec<SharedItem> {
        let mut items = Vec::with_capacity(self.items.len() * 2);
        // Add all keys first
        for element in &self.items {
            items.push(element.key.clone());
        }
        // Then add all values
        for element in &self.items {
            items.push(element.value.clone());
        }
        items
    }

    fn read_only(&mut self) {
        self.read_only = true;
    }

    fn clear(&mut self) {
        self.items.clear();
    }
}

impl MapItem {
    /// Creates a new Map with the specified capacity
    #[inline]
    pub fn with_capacity(n: usize) -> Self {
        MapItem {
            items: Vec::with_capacity(n),
            ref_count: 0,
            read_only: false,
        }
    }

    /// Inserts a key-value pair into the map
    /// Returns true if the key was new, false if it replaced an existing value
    pub fn insert(&mut self, key: SharedItem, value: SharedItem) -> bool {
        if self.read_only {
            return false;
        }

        // Check if key already exists
        for element in &mut self.items {
            if element.key == key {
                element.value = value;
                return false;
            }
        }

        // Insert new key-value pair
        self.items.push(MapElement::with_ref(key, value));
        true
    }

    /// Returns a reference to the value associated with the key
    pub fn get(&self, key: &SharedItem) -> Option<&SharedItem> {
        self.items.iter()
            .find(|element| &element.key == key)
            .map(|element| &element.value)
    }

    /// Updates the value associated with the key if it exists
    /// Returns true if the value was updated, false if the key wasn't found or map is read-only
    pub fn update(&mut self, key: &SharedItem, value: SharedItem) -> bool {
        if self.read_only {
            return false;
        }

        if let Some(element) = self.items.iter_mut().find(|element| &element.key == key) {
            element.value = value;
            true
        } else {
            false
        }
    }

    /// Adds a new key-value pair or updates existing value if key exists
    /// Returns true if key was new, false if value was updated
    pub fn add_or_update(&mut self, key: SharedItem, value: SharedItem) -> bool {
        if self.read_only {
            return false;
        }

        for element in &mut self.items {
            if element.key == key {
                element.value = value;
                return false;
            }
        }

        self.items.push(MapElement::with_ref(key, value));
        true
    }

    /// Returns true if the map contains the given key
    pub fn contains(&self, key: &SharedItem) -> bool {
        self.items.iter().any(|element| &element.key == key)
    }

    /// Returns an iterator over the keys in the map
    pub fn keys(&self) -> impl Iterator<Item = &SharedItem> {
        self.items.iter().map(|element| &element.key)
    }

    /// Returns an iterator over the values in the map
    pub fn values(&self) -> impl Iterator<Item = &SharedItem> {
        self.items.iter().map(|element| &element.value)
    }

    /// Returns an iterator over key-value pairs in the map
    pub fn iter(&self) -> impl Iterator<Item = (&SharedItem, &SharedItem)> {
        self.items.iter().map(|element| (&element.key, &element.value))
    }

    /// Returns the number of key-value pairs in the map
    #[inline]
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Returns true if the map is empty
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Clears all key-value pairs from the map
    pub fn clear(&mut self) {
        if !self.read_only {
            self.items.clear();
        }
    }

    /// Removes the key-value pair associated with the key
    /// Returns true if the key was found and removed, false if not found or map is read-only
    pub fn remove(&mut self, key: &SharedItem) -> bool {
        if self.read_only {
            return false;
        }

        if let Some(index) = self.items.iter().position(|element| &element.key == key) {
            self.items.remove(index);
            true
        } else {
            false
        }
    }

    /// Returns the raw pointer to the underlying RefCell
    /// This should only be used for comparison operations
    #[inline]
    pub(crate) fn as_ptr(&self) -> *const RefCell<Vec<MapElement>> {
        Rc::as_ptr(&self.items)
    }
}

impl Eq for MapItem {}

impl PartialEq for MapItem {
    /// Maps are equal only if they point to the same underlying vector
    #[inline]
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.items, &other.items)
    }
}

impl Hash for MapItem {
    #[inline]
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.as_ptr().hash(state);
    }
}

impl MapElement {
    /// Creates a new MapElement with the given key and value
    pub fn new(key: StackItem, value: StackItem) -> Self {
        Self {
            key: Rc::new(RefCell::new(key)),
            value: Rc::new(RefCell::new(value)),
        }
    }

    pub fn with_ref(key: SharedItem, value: SharedItem) -> Self {
        Self { key, value }
    }

    /// Returns a reference to the key
    pub fn key(&self) -> &SharedItem {
        &self.key
    }

    /// Returns a reference to the value
    pub fn value(&self) -> &SharedItem {
        &self.value
    }
}