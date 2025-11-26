#![allow(clippy::mutable_key_type)]

//! Map stack item implementation for the Neo Virtual Machine.
//!
//! This module provides the Map stack item implementation used in the Neo VM.

use crate::error::{VmError, VmResult};
use crate::reference_counter::{CompoundParent, ReferenceCounter};
use crate::stack_item::stack_item_type::StackItemType;
use crate::stack_item::stack_item_vertex::next_stack_item_id;
use crate::stack_item::StackItem;
use num_traits::Zero;
use std::collections::BTreeMap;

const MAX_KEY_SIZE: usize = 64;

/// Represents a map of stack items in the VM.
#[derive(Debug)]
pub struct Map {
    /// The items in the map.
    items: BTreeMap<StackItem, StackItem>,
    /// Unique identifier mirroring reference equality semantics.
    id: usize,
    /// Reference counter shared with the VM (mirrors C# CompoundType semantics).
    reference_counter: Option<ReferenceCounter>,
    /// Indicates whether the map is read-only.
    is_read_only: bool,
}

impl Map {
    /// Creates a new map with the specified items and reference counter.
    pub fn new(
        items: BTreeMap<StackItem, StackItem>,
        reference_counter: Option<ReferenceCounter>,
    ) -> Self {
        let map = Self {
            items,
            id: next_stack_item_id(),
            reference_counter,
            is_read_only: false,
        };

        if let Some(rc) = &map.reference_counter {
            map.add_reference_for_entries(rc);
        }

        map
    }

    /// Returns the reference counter assigned by the reference counter, if any.
    pub fn reference_counter(&self) -> Option<&ReferenceCounter> {
        self.reference_counter.as_ref()
    }

    /// Returns the unique identifier for this map (used for reference equality).
    pub fn id(&self) -> usize {
        self.id
    }

    /// Returns whether the map is marked as read-only.
    pub fn is_read_only(&self) -> bool {
        self.is_read_only
    }

    /// Sets the read-only state of the map.
    pub fn set_read_only(&mut self, read_only: bool) {
        self.is_read_only = read_only;
    }

    /// Gets the items in the map.
    pub fn items(&self) -> &BTreeMap<StackItem, StackItem> {
        &self.items
    }

    /// Gets a mutable reference to the items in the map.
    pub(crate) fn items_mut(&mut self) -> &mut BTreeMap<StackItem, StackItem> {
        &mut self.items
    }

    /// Gets the value for the specified key.
    pub fn get(&self, key: &StackItem) -> VmResult<&StackItem> {
        self.validate_key(key)?;
        self.items.get(key).ok_or_else(|| {
            VmError::catchable_exception_msg(format!("Key {:?} not found in Map.", key))
        })
    }

    /// Sets the value for the specified key.
    pub fn set(&mut self, key: StackItem, value: StackItem) -> VmResult<()> {
        self.ensure_mutable()?;
        self.validate_key(&key)?;

        if let Some(rc) = &self.reference_counter {
            self.validate_compound_reference(rc, &value)?;

            if let Some(old_value) = self.items.get(&key) {
                rc.remove_compound_reference(old_value, CompoundParent::Map(self.id));
            } else {
                rc.add_compound_reference(&key, CompoundParent::Map(self.id));
            }

            rc.add_compound_reference(&value, CompoundParent::Map(self.id));
        }

        self.items.insert(key, value);
        Ok(())
    }

    /// Removes the value for the specified key.
    pub fn remove(&mut self, key: &StackItem) -> VmResult<StackItem> {
        self.ensure_mutable()?;
        self.validate_key(key)?;

        let value = self
            .items
            .remove(key)
            .ok_or_else(|| VmError::invalid_operation_msg(format!("Key not found: {key:?}")))?;

        if let Some(rc) = &self.reference_counter {
            let parent = CompoundParent::Map(self.id);
            rc.remove_compound_reference(key, parent);
            rc.remove_compound_reference(&value, parent);
        }

        Ok(value)
    }

    /// Gets the number of items in the map.
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Returns true if the map is empty.
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Returns true if the map contains the given key.
    pub fn contains_key(&self, key: &StackItem) -> VmResult<bool> {
        self.validate_key(key)?;
        Ok(self.items.contains_key(key))
    }

    /// Removes all items from the map.
    pub fn clear(&mut self) -> VmResult<()> {
        self.ensure_mutable()?;
        if let Some(rc) = &self.reference_counter {
            let parent = CompoundParent::Map(self.id);
            for (key, value) in &self.items {
                rc.remove_compound_reference(key, parent);
                rc.remove_compound_reference(value, parent);
            }
        }
        self.items.clear();
        Ok(())
    }

    /// Consumes the map and returns the underlying entries.
    pub fn into_map(self) -> BTreeMap<StackItem, StackItem> {
        self.items
    }

    /// Returns an iterator over the key/value pairs.
    pub fn iter(&self) -> std::collections::btree_map::Iter<'_, StackItem, StackItem> {
        self.items.iter()
    }

    /// Returns a mutable iterator over the key/value pairs.
    pub fn iter_mut(&mut self) -> std::collections::btree_map::IterMut<'_, StackItem, StackItem> {
        self.items.iter_mut()
    }

    /// Creates a deep copy of the map.
    pub fn deep_copy(&self, reference_counter: Option<ReferenceCounter>) -> Self {
        let mut items = BTreeMap::new();
        for (k, v) in &self.items {
            items.insert(k.deep_clone(), v.deep_clone());
        }
        let mut copy = Self::new(items, reference_counter);
        copy.set_read_only(true);
        copy
    }

    /// Gets the type of the stack item.
    pub fn stack_item_type(&self) -> StackItemType {
        StackItemType::Map
    }

    /// Converts the map to a boolean.
    pub fn to_boolean(&self) -> bool {
        !self.items.is_empty()
    }

    fn ensure_mutable(&self) -> VmResult<()> {
        if self.is_read_only {
            Err(VmError::invalid_operation_msg(
                "The map is readonly, can not modify.",
            ))
        } else {
            Ok(())
        }
    }

    fn add_reference_for_entries(&self, rc: &ReferenceCounter) {
        let parent = CompoundParent::Map(self.id);
        for (key, value) in &self.items {
            if let Err(err) = self.validate_key(key) {
                panic!("{err}");
            }
            if let Err(err) = self.validate_compound_reference(rc, value) {
                panic!("{err}");
            }
            rc.add_compound_reference(key, parent);
            rc.add_compound_reference(value, parent);
        }
    }

    fn validate_key(&self, key: &StackItem) -> VmResult<()> {
        match key {
            StackItem::Boolean(_) => Ok(()),
            StackItem::Integer(value) => {
                let size = if value.is_zero() {
                    0
                } else {
                    value.to_signed_bytes_le().len()
                };
                if size > MAX_KEY_SIZE {
                    return Err(VmError::invalid_operation_msg(format!(
                        "Key size {size} bytes exceeds maximum allowed size of {MAX_KEY_SIZE} bytes."
                    )));
                }
                Ok(())
            }
            StackItem::ByteString(bytes) => {
                if bytes.len() > MAX_KEY_SIZE {
                    return Err(VmError::invalid_operation_msg(format!(
                        "Key size {} bytes exceeds maximum allowed size of {MAX_KEY_SIZE} bytes.",
                        bytes.len()
                    )));
                }
                Ok(())
            }
            StackItem::Buffer(buffer) => {
                if buffer.len() > MAX_KEY_SIZE {
                    return Err(VmError::invalid_operation_msg(format!(
                        "Key size {} bytes exceeds maximum allowed size of {MAX_KEY_SIZE} bytes.",
                        buffer.len()
                    )));
                }
                Ok(())
            }
            _ => Err(VmError::invalid_operation_msg(
                "Map keys must be primitive types.".to_string(),
            )),
        }
    }

    fn validate_compound_reference(&self, rc: &ReferenceCounter, item: &StackItem) -> VmResult<()> {
        match item {
            StackItem::Array(inner) => match inner.reference_counter() {
                Some(child_rc) if child_rc.ptr_eq(rc) => Ok(()),
                Some(_) => Err(VmError::invalid_operation_msg(
                    "Can not set a Map value without a ReferenceCounter.".to_string(),
                )),
                None => Err(VmError::invalid_operation_msg(
                    "Can not set a Map value without a ReferenceCounter.".to_string(),
                )),
            },
            StackItem::Struct(inner) => match inner.reference_counter() {
                Some(child_rc) if child_rc.ptr_eq(rc) => Ok(()),
                Some(_) => Err(VmError::invalid_operation_msg(
                    "Can not set a Map value without a ReferenceCounter.".to_string(),
                )),
                None => Err(VmError::invalid_operation_msg(
                    "Can not set a Map value without a ReferenceCounter.".to_string(),
                )),
            },
            StackItem::Map(inner) => match inner.reference_counter() {
                Some(child_rc) if child_rc.ptr_eq(rc) => Ok(()),
                Some(_) => Err(VmError::invalid_operation_msg(
                    "Can not set a Map value without a ReferenceCounter.".to_string(),
                )),
                None => Err(VmError::invalid_operation_msg(
                    "Can not set a Map value without a ReferenceCounter.".to_string(),
                )),
            },
            _ => Ok(()),
        }
    }
}

impl Clone for Map {
    fn clone(&self) -> Self {
        let clone = Self {
            items: self.items.clone(),
            id: next_stack_item_id(),
            reference_counter: self.reference_counter.clone(),
            is_read_only: self.is_read_only,
        };

        if let Some(rc) = &clone.reference_counter {
            clone.add_reference_for_entries(rc);
        }

        clone
    }
}

impl IntoIterator for Map {
    type Item = (StackItem, StackItem);
    type IntoIter = std::collections::btree_map::IntoIter<StackItem, StackItem>;

    fn into_iter(self) -> Self::IntoIter {
        self.items.into_iter()
    }
}

impl<'a> IntoIterator for &'a Map {
    type Item = (&'a StackItem, &'a StackItem);
    type IntoIter = std::collections::btree_map::Iter<'a, StackItem, StackItem>;

    fn into_iter(self) -> Self::IntoIter {
        self.items.iter()
    }
}

impl<'a> IntoIterator for &'a mut Map {
    type Item = (&'a StackItem, &'a mut StackItem);
    type IntoIter = std::collections::btree_map::IterMut<'a, StackItem, StackItem>;

    fn into_iter(self) -> Self::IntoIter {
        self.items.iter_mut()
    }
}

impl From<Map> for BTreeMap<StackItem, StackItem> {
    fn from(map: Map) -> Self {
        map.items
    }
}

#[cfg(test)]
#[allow(dead_code)]
mod tests {
    use super::*;
    use num_traits::ToPrimitive;
    use std::collections::BTreeMap;

    #[test]
    fn test_map_creation() {
        let mut items = BTreeMap::new();
        items.insert(StackItem::from_int(1), StackItem::from_int(10));
        items.insert(StackItem::from_int(2), StackItem::from_int(20));

        let map = Map::new(items.clone(), None);

        assert_eq!(map.len(), 2);
        assert_eq!(map.items(), &items);
        assert_eq!(map.stack_item_type(), StackItemType::Map);
    }

    #[test]
    fn test_map_get() -> Result<(), Box<dyn std::error::Error>> {
        let mut items = BTreeMap::new();
        items.insert(StackItem::from_int(1), StackItem::from_int(10));
        items.insert(StackItem::from_int(2), StackItem::from_int(20));

        let map = Map::new(items, None);

        assert_eq!(
            map.get(&StackItem::from_int(1))?
                .as_int()
                .unwrap()
                .to_i32()
                .unwrap(),
            10
        );
        assert_eq!(
            map.get(&StackItem::from_int(2))?
                .as_int()
                .unwrap()
                .to_i32()
                .unwrap(),
            20
        );
        assert!(map.get(&StackItem::from_int(3)).is_err());
        Ok(())
    }

    #[test]
    fn test_map_set() -> Result<(), Box<dyn std::error::Error>> {
        let mut items = BTreeMap::new();
        items.insert(StackItem::from_int(1), StackItem::from_int(10));

        let mut map = Map::new(items, None);

        // Update existing key
        map.set(StackItem::from_int(1), StackItem::from_int(100))?;
        assert_eq!(
            map.get(&StackItem::from_int(1))?
                .as_int()
                .unwrap()
                .to_i32()
                .unwrap(),
            100
        );

        // Add new key
        map.set(StackItem::from_int(2), StackItem::from_int(20))?;
        assert_eq!(
            map.get(&StackItem::from_int(2))?
                .as_int()
                .unwrap()
                .to_i32()
                .unwrap(),
            20
        );

        assert_eq!(map.len(), 2);
        Ok(())
    }

    #[test]
    fn test_map_remove() -> Result<(), Box<dyn std::error::Error>> {
        let mut items = BTreeMap::new();
        items.insert(StackItem::from_int(1), StackItem::from_int(10));
        items.insert(StackItem::from_int(2), StackItem::from_int(20));

        let mut map = Map::new(items, None);

        let removed = map.remove(&StackItem::from_int(1))?;
        assert_eq!(
            removed
                .as_int()
                .expect("intermediate value should exist")
                .to_i32()
                .unwrap(),
            10
        );
        assert_eq!(map.len(), 1);
        assert!(map.get(&StackItem::from_int(1)).is_err());

        assert!(map.remove(&StackItem::from_int(3)).is_err());
        Ok(())
    }

    #[test]
    fn test_map_clear() {
        let mut items = BTreeMap::new();
        items.insert(StackItem::from_int(1), StackItem::from_int(10));
        items.insert(StackItem::from_int(2), StackItem::from_int(20));

        let mut map = Map::new(items, None);

        map.clear().unwrap();

        assert_eq!(map.len(), 0);
        assert!(map.is_empty());
    }

    #[test]
    fn test_map_deep_copy() -> Result<(), Box<dyn std::error::Error>> {
        let mut items = BTreeMap::new();
        items.insert(StackItem::from_int(1), StackItem::from_int(10));
        items.insert(
            StackItem::from_int(2),
            StackItem::from_array(vec![StackItem::from_int(20), StackItem::from_int(30)]),
        );

        let map = Map::new(items, None);
        let copied = map.deep_copy(None);

        assert_eq!(copied.len(), map.len());

        // Check that the nested array was deep copied
        let nested_original = map.get(&StackItem::from_int(2))?.as_array().unwrap();
        let nested_copied = copied.get(&StackItem::from_int(2))?.as_array().unwrap();

        assert_eq!(nested_copied.len(), nested_original.len());
        assert_eq!(
            nested_copied[0].as_int().unwrap(),
            nested_original[0].as_int().unwrap()
        );
        assert_eq!(
            nested_copied[1].as_int().unwrap(),
            nested_original[1].as_int().unwrap()
        );
        Ok(())
    }

    #[test]
    fn test_map_to_boolean() {
        // Test empty map
        let empty_map = Map::new(BTreeMap::new(), None);
        assert!(!empty_map.to_boolean());

        // Test non-empty map
        let mut items = BTreeMap::new();
        items.insert(StackItem::from_int(1), StackItem::from_int(10));
        let non_empty_map = Map::new(items, None);
        assert!(non_empty_map.to_boolean());
    }
}
