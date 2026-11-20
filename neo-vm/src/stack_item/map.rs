#![allow(clippy::mutable_key_type)]

//! Map stack item implementation for the Neo Virtual Machine.
//!
//! This module provides the Map stack item implementation used in the Neo VM.

use crate::StackItemType;
use crate::VmOrderedDictionary;
use crate::error::{VmError, VmResult};
use crate::next_stack_item_id;
use crate::reference_counter::{StackReferenceRelease, SubItems};
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
    /// Number of stack references held on this map (C# `CompoundType.StackReferences`).
    stack_references: usize,
    /// Indicates whether the map is read-only.
    is_read_only: bool,
}

impl Map {
    /// Creates a new map with the specified items.
    ///
    /// As in neo-vm v3.10.1, a map holds no reference counter and none of its
    /// methods touch one: the opcode handlers do the counting.
    pub fn new<T>(items: T) -> Self
    where
        T: Into<VmOrderedDictionary<StackItem, StackItem>>,
    {
        Self {
            inner: Arc::new(Mutex::new(MapInner {
                items: items.into(),
                id: next_stack_item_id(),
                stack_references: 0,
                is_read_only: false,
            })),
        }
    }

    /// Returns the number of stack references held on this map.
    #[must_use]
    pub fn stack_references(&self) -> usize {
        self.inner.lock().stack_references
    }

    /// Returns whether this map is stack referenced (C# `IsStackReferenced`).
    #[must_use]
    pub fn is_stack_referenced(&self) -> bool {
        self.inner.lock().stack_references != 0
    }

    /// Adds `count` stack references. Returns the sub items when this call
    /// made the map stack referenced.
    pub(crate) fn add_stack_references(&self, count: usize) -> Option<SubItems> {
        let mut inner = self.inner.lock();
        inner.stack_references += count;
        (inner.stack_references == count).then(|| Self::collect_sub_items(&inner))
    }

    /// Removes one stack reference, if the map is stack referenced.
    pub(crate) fn remove_stack_reference(&self) -> StackReferenceRelease {
        let mut inner = self.inner.lock();
        if inner.stack_references == 0 {
            return StackReferenceRelease::NotReferenced;
        }
        inner.stack_references -= 1;
        if inner.stack_references == 0 {
            StackReferenceRelease::Unreferenced(Self::collect_sub_items(&inner))
        } else {
            StackReferenceRelease::StillReferenced
        }
    }

    /// Takes a snapshot of the sub items (C# `SubItems`).
    pub(crate) fn sub_items(&self) -> SubItems {
        Self::collect_sub_items(&self.inner.lock())
    }

    /// C# `Map.SubItems`: every key, then every value.
    fn collect_sub_items(inner: &MapInner) -> SubItems {
        SubItems::collect(inner.items.keys().chain(inner.items.values()))
    }

    /// Returns the unique identifier for this map (used for reference equality).
    #[must_use]
    pub fn id(&self) -> usize {
        self.inner.lock().id
    }

    /// Returns whether the map is marked as read-only.
    #[must_use]
    pub fn is_read_only(&self) -> bool {
        self.inner.lock().is_read_only
    }

    /// Sets the read-only state of the map.
    pub fn set_read_only(&self, read_only: bool) {
        self.inner.lock().is_read_only = read_only;
    }

    /// Gets the items in the map.
    #[must_use]
    pub fn items(&self) -> VmOrderedDictionary<StackItem, StackItem> {
        self.inner.lock().items.clone()
    }

    /// Provides zero-copy read access to the map entries under the lock.
    #[inline]
    pub fn with_items<R>(
        &self,
        f: impl FnOnce(&VmOrderedDictionary<StackItem, StackItem>) -> R,
    ) -> R {
        let inner = self.inner.lock();
        f(&inner.items)
    }

    /// Gets the value for the specified key.
    pub fn get(&self, key: &StackItem) -> VmResult<StackItem> {
        self.validate_key(key)?;
        self.inner.lock().items.get(key).cloned().ok_or_else(|| {
            VmError::catchable_exception_msg(format!("Key {key:?} not found in Map."))
        })
    }

    /// Gets the value for the specified key, or `None` if the key is absent
    /// (C# `Map.TryGetValue`).
    pub fn try_get(&self, key: &StackItem) -> VmResult<Option<StackItem>> {
        self.validate_key(key)?;
        Ok(self.inner.lock().items.get(key).cloned())
    }

    /// Sets the value for the specified key.
    pub fn set(&self, key: StackItem, value: StackItem) -> VmResult<()> {
        // A replaced value is dropped after the lock is released.
        let _replaced = {
            let mut inner = self.inner.lock();
            Self::ensure_mutable(&inner)?;
            self.validate_key(&key)?;
            inner.items.insert(key, value)
        };
        Ok(())
    }

    /// Removes the value for the specified key, returning `None` if the key is absent.
    pub fn remove_entry(&self, key: &StackItem) -> VmResult<Option<StackItem>> {
        let mut inner = self.inner.lock();
        Self::ensure_mutable(&inner)?;
        self.validate_key(key)?;
        Ok(inner.items.remove(key))
    }

    /// Removes the value for the specified key.
    pub fn remove(&self, key: &StackItem) -> VmResult<StackItem> {
        self.remove_entry(key)?
            .ok_or_else(|| VmError::invalid_operation_msg(format!("Key not found: {key:?}")))
    }

    /// Gets the number of items in the map.
    #[must_use]
    pub fn len(&self) -> usize {
        self.inner.lock().items.len()
    }

    /// Returns true if the map is empty.
    #[must_use]
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
        // The removed entries are dropped after the lock is released.
        let _removed = {
            let mut inner = self.inner.lock();
            Self::ensure_mutable(&inner)?;
            std::mem::take(&mut inner.items)
        };
        Ok(())
    }

    /// Consumes the map and returns the underlying entries.
    #[must_use]
    pub fn into_map(self) -> VmOrderedDictionary<StackItem, StackItem> {
        self.items()
    }

    /// Returns an iterator over the key/value pairs.
    #[must_use]
    pub fn iter(&self) -> std::vec::IntoIter<(StackItem, StackItem)> {
        self.with_items(|items| {
            items
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect::<Vec<_>>()
        })
        .into_iter()
    }

    /// Creates a deep copy of the map.
    pub fn deep_copy(&self) -> VmResult<Self> {
        // Snapshot first: cloning a value may need this map's lock.
        let mut items = VmOrderedDictionary::new();
        for (k, v) in self.iter() {
            items.insert(k.deep_clone(), v.deep_clone());
        }
        let copy = Self::new(items);
        copy.set_read_only(true);
        Ok(copy)
    }

    /// Gets the type of the stack item.
    #[must_use]
    pub const fn stack_item_type(&self) -> StackItemType {
        StackItemType::Map
    }

    /// One-lock snapshot of the fields the `Debug` renderer prints, as
    /// `(items, id, stack_references, is_read_only)`.
    pub(crate) fn debug_snapshot(
        &self,
    ) -> (
        VmOrderedDictionary<StackItem, StackItem>,
        usize,
        usize,
        bool,
    ) {
        let inner = self.inner.lock();
        (
            inner.items.clone(),
            inner.id,
            inner.stack_references,
            inner.is_read_only,
        )
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

    fn validate_key(&self, key: &StackItem) -> VmResult<()> {
        if !matches!(
            key,
            StackItem::Boolean(_) | StackItem::Integer(_) | StackItem::ByteString(_)
        ) {
            return Err(VmError::invalid_type_simple(
                "Map key must be a PrimitiveType (Boolean, Integer, or ByteString)",
            ));
        }
        // Fast path: avoid allocation for ByteString keys (the common case).
        let len = if let Some(slice) = key.as_bytes_ref() {
            slice.len()
        } else {
            key.as_bytes()?.len()
        };
        if len > MAX_KEY_SIZE {
            return Err(VmError::invalid_operation_msg(format!(
                "The key length exceed the max value. {MAX_KEY_SIZE} at most."
            )));
        }
        Ok(())
    }
}

impl From<Map> for VmOrderedDictionary<StackItem, StackItem> {
    fn from(map: Map) -> Self {
        map.items()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use num_bigint::BigInt;

    #[test]
    fn map_remove_entry_returns_none_for_missing_key() {
        let map = Map::new(Vec::new());
        let missing = StackItem::from_int(BigInt::from(42));
        assert!(
            map.remove_entry(&missing)
                .expect("remove_entry succeeds")
                .is_none()
        );
        assert!(map.remove(&missing).is_err());
    }

    #[test]
    fn map_rejects_null_and_buffer_keys() {
        let map = Map::new(Vec::new());
        assert!(
            map.set(StackItem::Null, StackItem::from_bool(true))
                .is_err()
        );
        assert!(
            map.set(
                StackItem::from_buffer(vec![1, 2, 3]),
                StackItem::from_bool(true)
            )
            .is_err()
        );
        assert!(
            map.set(
                StackItem::from_byte_string(vec![1, 2, 3]),
                StackItem::from_bool(true),
            )
            .is_ok()
        );
    }
}
