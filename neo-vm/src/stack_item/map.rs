#![allow(clippy::mutable_key_type)]

//! Map stack item implementation for the Neo Virtual Machine.
//!
//! This module provides the Map stack item implementation used in the Neo VM.

use crate::collections::VmOrderedDictionary;
use crate::error::{VmError, VmResult};
use crate::reference_counter::{CompoundParent, ReferenceCounter};
use crate::stack_item::stack_item_type::StackItemType;
use crate::stack_item::stack_item_vertex::next_stack_item_id;
use crate::stack_item::StackItem;
use parking_lot::Mutex;
use std::sync::Arc;

const MAX_KEY_SIZE: usize = 64;

/// Represents a map of stack items in the VM.
#[derive(Debug, Clone)]
pub struct Map {
    inner: Arc<Mutex<MapInner>>,
}

#[derive(Debug)]
struct MapInner {
    /// The items in the map.
    items: VmOrderedDictionary<StackItem, StackItem>,
    /// Unique identifier mirroring reference equality semantics.
    id: usize,
    /// Reference counter shared with the VM (mirrors C# CompoundType semantics).
    reference_counter: Option<ReferenceCounter>,
    /// Indicates whether the map is read-only.
    is_read_only: bool,
}

impl Map {
    /// Creates a new map with the specified items and reference counter.
    pub fn new<T>(items: T, reference_counter: Option<ReferenceCounter>) -> VmResult<Self>
    where
        T: Into<VmOrderedDictionary<StackItem, StackItem>>,
    {
        let map = Self {
            inner: Arc::new(Mutex::new(MapInner {
                items: items.into(),
                id: next_stack_item_id(),
                reference_counter,
                is_read_only: false,
            })),
        };

        if let Some(rc) = map.reference_counter() {
            map.add_reference_for_entries(&rc)?;
        }

        Ok(map)
    }

    /// Creates a map without a reference counter.
    pub fn new_untracked<T>(items: T) -> Self
    where
        T: Into<VmOrderedDictionary<StackItem, StackItem>>,
    {
        Self {
            inner: Arc::new(Mutex::new(MapInner {
                items: items.into(),
                id: next_stack_item_id(),
                reference_counter: None,
                is_read_only: false,
            })),
        }
    }

    /// Returns the reference counter assigned by the reference counter, if any.
    pub fn reference_counter(&self) -> Option<ReferenceCounter> {
        self.inner.lock().reference_counter.clone()
    }

    /// Returns the unique identifier for this map (used for reference equality).
    pub fn id(&self) -> usize {
        self.inner.lock().id
    }

    /// Returns whether the map is marked as read-only.
    pub fn is_read_only(&self) -> bool {
        self.inner.lock().is_read_only
    }

    /// Sets the read-only state of the map.
    pub fn set_read_only(&self, read_only: bool) {
        self.inner.lock().is_read_only = read_only;
    }

    /// Gets the items in the map.
    pub fn items(&self) -> VmOrderedDictionary<StackItem, StackItem> {
        self.inner.lock().items.clone()
    }

    /// Gets the value for the specified key.
    pub fn get(&self, key: &StackItem) -> VmResult<StackItem> {
        self.validate_key(key)?;
        self.inner
            .lock()
            .items
            .get(key)
            .cloned()
            .ok_or_else(|| {
                VmError::catchable_exception_msg(format!("Key {:?} not found in Map.", key))
            })
    }

    /// Sets the value for the specified key.
    pub fn set(&self, key: StackItem, value: StackItem) -> VmResult<()> {
        let mut inner = self.inner.lock();
        Self::ensure_mutable(&inner)?;
        self.validate_key(&key)?;

        if let Some(rc) = &inner.reference_counter {
            Self::validate_compound_reference(rc, &value)?;

            let parent = CompoundParent::Map(inner.id);
            if let Some(old_value) = inner.items.get(&key) {
                rc.remove_compound_reference(old_value, parent);
            } else {
                rc.add_compound_reference(&key, parent);
            }

            rc.add_compound_reference(&value, parent);
        }

        inner.items.insert(key, value);
        Ok(())
    }

    /// Removes the value for the specified key.
    pub fn remove(&self, key: &StackItem) -> VmResult<StackItem> {
        let mut inner = self.inner.lock();
        Self::ensure_mutable(&inner)?;
        self.validate_key(key)?;

        let value = inner
            .items
            .remove(key)
            .ok_or_else(|| VmError::invalid_operation_msg(format!("Key not found: {key:?}")))?;

        if let Some(rc) = &inner.reference_counter {
            let parent = CompoundParent::Map(inner.id);
            rc.remove_compound_reference(key, parent);
            rc.remove_compound_reference(&value, parent);
        }

        Ok(value)
    }

    /// Gets the number of items in the map.
    pub fn len(&self) -> usize {
        self.inner.lock().items.len()
    }

    /// Returns true if the map is empty.
    pub fn is_empty(&self) -> bool {
        self.inner.lock().items.is_empty()
    }

    /// Returns true if the map contains the given key.
    pub fn contains_key(&self, key: &StackItem) -> VmResult<bool> {
        self.validate_key(key)?;
        Ok(self.inner.lock().items.contains_key(key))
    }

    /// Removes all items from the map.
    pub fn clear(&self) -> VmResult<()> {
        let mut inner = self.inner.lock();
        Self::ensure_mutable(&inner)?;
        if let Some(rc) = &inner.reference_counter {
            let parent = CompoundParent::Map(inner.id);
            for (key, value) in inner.items.iter() {
                rc.remove_compound_reference(key, parent);
                rc.remove_compound_reference(value, parent);
            }
        }
        inner.items.clear();
        Ok(())
    }

    /// Consumes the map and returns the underlying entries.
    pub fn into_map(self) -> VmOrderedDictionary<StackItem, StackItem> {
        self.items()
    }

    /// Returns an iterator over the key/value pairs.
    pub fn iter(&self) -> std::vec::IntoIter<(StackItem, StackItem)> {
        self.items()
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect::<Vec<_>>()
            .into_iter()
    }

    /// Creates a deep copy of the map.
    pub fn deep_copy(&self, reference_counter: Option<ReferenceCounter>) -> VmResult<Self> {
        let mut items = VmOrderedDictionary::new();
        for (k, v) in self.items().iter() {
            items.insert(k.deep_clone(), v.deep_clone());
        }
        let copy = Self::new(items, reference_counter)?;
        copy.set_read_only(true);
        Ok(copy)
    }

    /// Gets the type of the stack item.
    pub fn stack_item_type(&self) -> StackItemType {
        StackItemType::Map
    }

    fn ensure_mutable(inner: &MapInner) -> VmResult<()> {
        if inner.is_read_only {
            Err(VmError::invalid_operation_msg(
                "The map is readonly, can not modify.".to_string(),
            ))
        } else {
            Ok(())
        }
    }

    fn add_reference_for_entries(&self, rc: &ReferenceCounter) -> VmResult<()> {
        let items = self.items();
        let parent = CompoundParent::Map(self.id());
        for (key, value) in items.iter() {
            Self::validate_compound_reference(rc, value)?;
            rc.add_compound_reference(key, parent);
            rc.add_compound_reference(value, parent);
        }
        Ok(())
    }

    fn validate_key(&self, key: &StackItem) -> VmResult<()> {
        let bytes = key.as_bytes()?;
        if bytes.len() > MAX_KEY_SIZE {
            return Err(VmError::invalid_operation_msg(format!(
                "The key length exceed the max value. {MAX_KEY_SIZE} at most."
            )));
        }
        Ok(())
    }

    fn validate_compound_reference(rc: &ReferenceCounter, item: &StackItem) -> VmResult<()> {
        match item {
            StackItem::Array(inner) => match inner.reference_counter() {
                Some(child_rc) if child_rc.ptr_eq(rc) => Ok(()),
                Some(_) | None => Err(VmError::invalid_operation_msg(
                    "Can not set a Map without a ReferenceCounter.".to_string(),
                )),
            },
            StackItem::Struct(inner) => match inner.reference_counter() {
                Some(child_rc) if child_rc.ptr_eq(rc) => Ok(()),
                Some(_) | None => Err(VmError::invalid_operation_msg(
                    "Can not set a Map without a ReferenceCounter.".to_string(),
                )),
            },
            StackItem::Map(inner) => match inner.reference_counter() {
                Some(child_rc) if child_rc.ptr_eq(rc) => Ok(()),
                Some(_) | None => Err(VmError::invalid_operation_msg(
                    "Can not set a Map without a ReferenceCounter.".to_string(),
                )),
            },
            _ => Ok(()),
        }
    }

    /// Ensures the map and its children share the provided reference counter.
    pub(crate) fn attach_reference_counter(&self, rc: &ReferenceCounter) -> VmResult<()> {
        {
            let mut inner = self.inner.lock();
            if let Some(existing) = &inner.reference_counter {
                if existing.ptr_eq(rc) {
                    return Ok(());
                }
                return Err(VmError::invalid_operation_msg(
                    "Map has mismatched reference counter.",
                ));
            }

            for (_, value) in inner.items.iter_mut() {
                value.attach_reference_counter(rc)?;
            }

            inner.reference_counter = Some(rc.clone());
        }

        self.add_reference_for_entries(rc)?;
        Ok(())
    }
}

impl From<Map> for VmOrderedDictionary<StackItem, StackItem> {
    fn from(map: Map) -> Self {
        map.items()
    }
}
