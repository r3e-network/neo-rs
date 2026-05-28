//! InteroperableList - matches C# Neo.SmartContract.Native.InteroperableList exactly

use crate::smart_contract::interoperable::Interoperable;
use crate::neo_vm::StackItem;
use std::ops::{Deref, DerefMut};

/// A list that can be converted to/from StackItem (matches C# InteroperableList\<T>)
#[derive(Clone, Debug)]
pub struct InteroperableList<T: Interoperable + Clone> {
    items: Vec<T>,
}

impl<T: Interoperable + Clone> InteroperableList<T> {
    /// Creates a new empty list
    pub fn new() -> Self {
        Self { items: Vec::new() }
    }
    
    /// Creates a list with initial capacity
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            items: Vec::with_capacity(capacity),
        }
    }
    
    /// Adds an item to the list
    pub fn add(&mut self, item: T) {
        self.items.push(item);
    }
    
    /// Removes an item at the specified index
    pub fn remove_at(&mut self, index: usize) -> Option<T> {
        if index < self.items.len() {
            Some(self.items.remove(index))
        } else {
            None
        }
    }
    
    /// Clears the list
    pub fn clear(&mut self) {
        self.items.clear();
    }
    
    /// Gets the count of items
    pub fn count(&self) -> usize {
        self.items.len()
    }
    
    /// Checks if the list contains an item
    pub fn contains(&self, item: &T) -> bool 
    where
        T: PartialEq,
    {
        self.items.contains(item)
    }
}

impl<T: Interoperable + Clone> Default for InteroperableList<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Interoperable + Clone> Deref for InteroperableList<T> {
    type Target = Vec<T>;
    
    fn deref(&self) -> &Self::Target {
        &self.items
    }
}

impl<T: Interoperable + Clone> DerefMut for InteroperableList<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.items
    }
}

impl<T: Interoperable + Clone + Default> Interoperable for InteroperableList<T> {
    fn from_stack_item(&mut self, stack_item: StackItem) -> Result<(), crate::neo_vm::VmError> {
        self.items.clear();

        if let StackItem::Array(array) = stack_item {
            for element in array.items() {
                let mut value = T::default();
                value.from_stack_item(element)?;
                self.items.push(value);
            }
        }
        Ok(())
    }

    fn to_stack_item(&self) -> Result<StackItem, crate::neo_vm::VmError> {
        let items: Vec<StackItem> = self.items
            .iter()
            .map(|item| item.to_stack_item())
            .collect::<Result<Vec<_>, _>>()?;

        Ok(StackItem::from_array(items))
    }
    
    fn clone_box(&self) -> Box<dyn Interoperable> {
        Box::new(self.clone())
    }
}
