//! Struct stack item implementation for the Neo Virtual Machine.
//!
//! This module provides the Struct stack item implementation used in the Neo VM.

use crate::StackItemType;
use crate::error::{VmError, VmResult};
use crate::next_stack_item_id;
use crate::reference_counter::{StackReferenceRelease, SubItems};
use crate::stack_item::StackItem;
use parking_lot::Mutex;
use std::collections::HashSet;
use std::sync::Arc;

/// Represents a struct of stack items in the VM.
#[derive(Debug, Clone)]
pub struct Struct {
    inner: Arc<Mutex<StructInner>>,
}

#[derive(Debug)]
struct StructInner {
    /// The items in the struct.
    items: Vec<StackItem>,
    /// Unique identifier mirroring reference equality semantics.
    id: usize,
    /// Number of stack references held on this struct (C# `CompoundType.StackReferences`).
    stack_references: usize,
    /// Indicates whether the struct is read-only.
    is_read_only: bool,
}

impl Struct {
    /// Creates a new struct with the specified items.
    ///
    /// As in neo-vm v3.10.1, a struct holds no reference counter and none of
    /// its methods touch one: the opcode handlers do the counting.
    #[must_use]
    pub fn new(items: Vec<StackItem>) -> Self {
        Self {
            inner: Arc::new(Mutex::new(StructInner {
                items,
                id: next_stack_item_id(),
                stack_references: 0,
                is_read_only: false,
            })),
        }
    }

    /// Returns the unique identifier for this struct (used for reference equality).
    #[must_use]
    pub fn id(&self) -> usize {
        self.inner.lock().id
    }

    /// Returns the number of stack references held on this struct.
    #[must_use]
    pub fn stack_references(&self) -> usize {
        self.inner.lock().stack_references
    }

    /// Returns whether this struct is stack referenced (C# `IsStackReferenced`).
    #[must_use]
    pub fn is_stack_referenced(&self) -> bool {
        self.inner.lock().stack_references != 0
    }

    /// Adds `count` stack references. Returns the sub items when this call
    /// made the struct stack referenced.
    pub(crate) fn add_stack_references(&self, count: usize) -> Option<SubItems> {
        let mut inner = self.inner.lock();
        inner.stack_references += count;
        (inner.stack_references == count).then(|| SubItems::collect(&inner.items))
    }

    /// Removes one stack reference, if the struct is stack referenced.
    pub(crate) fn remove_stack_reference(&self) -> StackReferenceRelease {
        let mut inner = self.inner.lock();
        if inner.stack_references == 0 {
            return StackReferenceRelease::NotReferenced;
        }
        inner.stack_references -= 1;
        if inner.stack_references == 0 {
            StackReferenceRelease::Unreferenced(SubItems::collect(&inner.items))
        } else {
            StackReferenceRelease::StillReferenced
        }
    }

    /// Takes a snapshot of the sub items (C# `SubItems`).
    pub(crate) fn sub_items(&self) -> SubItems {
        SubItems::collect(&self.inner.lock().items)
    }

    /// Returns whether the struct is marked as read-only.
    #[must_use]
    pub fn is_read_only(&self) -> bool {
        self.inner.lock().is_read_only
    }

    /// Sets the read-only state of the struct.
    pub fn set_read_only(&self, read_only: bool) {
        self.inner.lock().is_read_only = read_only;
    }

    /// Gets the items in the struct.
    #[must_use]
    pub fn items(&self) -> Vec<StackItem> {
        self.inner.lock().items.clone()
    }

    /// Returns a stable pointer used for identity tracking.
    #[must_use]
    pub fn as_ptr(&self) -> *const StackItem {
        self.inner.lock().items.as_ptr()
    }

    /// Gets the item at the specified index.
    pub fn get(&self, index: usize) -> VmResult<StackItem> {
        self.inner
            .lock()
            .items
            .get(index)
            .cloned()
            .ok_or_else(|| VmError::invalid_operation_msg(format!("Index out of range: {index}")))
    }

    /// Sets the item at the specified index.
    pub fn set(&self, index: usize, item: StackItem) -> VmResult<()> {
        // The replaced item is dropped after the lock is released.
        let _replaced = {
            let mut inner = self.inner.lock();
            if index >= inner.items.len() {
                return Err(VmError::invalid_operation_msg(format!(
                    "Index out of range: {index}"
                )));
            }

            Self::ensure_mutable(&inner)?;
            std::mem::replace(&mut inner.items[index], item)
        };
        Ok(())
    }

    /// Adds an item to the end of the struct.
    pub fn push(&self, item: StackItem) -> VmResult<()> {
        let mut inner = self.inner.lock();
        Self::ensure_mutable(&inner)?;
        inner.items.push(item);
        Ok(())
    }

    /// Removes and returns the last item in the struct.
    pub fn pop(&self) -> VmResult<StackItem> {
        let mut inner = self.inner.lock();
        Self::ensure_mutable(&inner)?;
        inner
            .items
            .pop()
            .ok_or_else(|| VmError::invalid_operation_msg("Struct is empty"))
    }

    /// Removes the item at the specified index.
    pub fn remove(&self, index: usize) -> VmResult<StackItem> {
        let mut inner = self.inner.lock();
        if index >= inner.items.len() {
            return Err(VmError::invalid_operation_msg(format!(
                "Index out of range: {index}"
            )));
        }

        Self::ensure_mutable(&inner)?;
        Ok(inner.items.remove(index))
    }

    /// Gets the number of items in the struct.
    #[must_use]
    pub fn len(&self) -> usize {
        self.inner.lock().items.len()
    }

    /// Returns true if the struct is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.inner.lock().items.is_empty()
    }

    /// Removes all items from the struct.
    pub fn clear(&self) -> VmResult<()> {
        // The removed items are dropped after the lock is released.
        let _removed = {
            let mut inner = self.inner.lock();
            Self::ensure_mutable(&inner)?;
            std::mem::take(&mut inner.items)
        };
        Ok(())
    }

    /// Returns an iterator over the items.
    #[must_use]
    pub fn iter(&self) -> std::vec::IntoIter<StackItem> {
        self.items().into_iter()
    }

    /// Provides zero-copy read access to the items under the lock.
    #[inline]
    pub fn with_items<R>(&self, f: impl FnOnce(&[StackItem]) -> R) -> R {
        let inner = self.inner.lock();
        f(&inner.items)
    }

    /// Provides zero-copy mutable access to the items under the lock.
    #[inline]
    pub fn with_items_mut<R>(&self, f: impl FnOnce(&mut Vec<StackItem>) -> R) -> R {
        let mut inner = self.inner.lock();
        f(&mut inner.items)
    }

    /// Creates a deep copy of the struct.
    pub fn deep_copy(&self) -> VmResult<Self> {
        // Snapshot first: cloning a sub item may need this struct's lock.
        let items = self
            .items()
            .iter()
            .map(super::stack_item::StackItem::deep_clone)
            .collect();
        let copy = Self::new(items);
        copy.set_read_only(true);
        Ok(copy)
    }

    /// Clones the struct respecting execution limits (mirrors C# Struct.Clone).
    pub fn clone_with_limits(&self, limits: &crate::ExecutionEngineLimits) -> VmResult<Self> {
        let mut remaining = i64::from(limits.max_stack_size) - 1;
        let mut visited = HashSet::new();
        self.clone_with_remaining(&mut remaining, &mut visited)
    }

    fn clone_with_remaining(
        &self,
        remaining: &mut i64,
        visited: &mut HashSet<usize>,
    ) -> VmResult<Self> {
        let id = self.id();
        if !visited.insert(id) {
            return Err(VmError::invalid_operation_msg(
                "Beyond struct subitem clone limits!",
            ));
        }

        let clone = Self::new(Vec::new());

        for item in self.items() {
            *remaining -= 1;
            if *remaining < 0 {
                visited.remove(&id);
                return Err(VmError::invalid_operation_msg(
                    "Beyond struct subitem clone limits!",
                ));
            }

            let cloned_item = match item {
                StackItem::Struct(inner) => {
                    StackItem::Struct(inner.clone_with_remaining(remaining, visited)?)
                }
                _ => item.clone(),
            };

            clone.push(cloned_item)?;
        }

        visited.remove(&id);
        Ok(clone)
    }

    /// Gets the type of the stack item.
    #[must_use]
    pub const fn stack_item_type(&self) -> StackItemType {
        StackItemType::Struct
    }

    /// Reverses the order of items in the struct.
    pub fn reverse_items(&self) -> VmResult<()> {
        let mut inner = self.inner.lock();
        Self::ensure_mutable(&inner)?;
        inner.items.reverse();
        Ok(())
    }

    fn ensure_mutable(inner: &StructInner) -> VmResult<()> {
        if inner.is_read_only {
            Err(VmError::invalid_operation_msg(
                "The struct is readonly, can not modify.".to_string(),
            ))
        } else {
            Ok(())
        }
    }
}

impl From<Struct> for Vec<StackItem> {
    fn from(structure: Struct) -> Self {
        structure.items()
    }
}
