//! Struct stack item implementation for the Neo Virtual Machine.
//!
//! This module provides the Struct stack item implementation used in the Neo VM.

use crate::error::{VmError, VmResult};
use crate::reference_counter::{CompoundParent, ReferenceCounter};
use crate::stack_item::stack_item_type::StackItemType;
use crate::stack_item::stack_item_vertex::next_stack_item_id;
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
    /// Reference counter shared with the VM (mirrors C# CompoundType semantics).
    reference_counter: Option<ReferenceCounter>,
    /// Indicates whether the struct is read-only.
    is_read_only: bool,
}

impl Struct {
    /// Creates a new struct with the specified items and reference counter.
    pub fn new(
        items: Vec<StackItem>,
        reference_counter: Option<ReferenceCounter>,
    ) -> VmResult<Self> {
        let structure = Self {
            inner: Arc::new(Mutex::new(StructInner {
                items,
                id: next_stack_item_id(),
                reference_counter,
                is_read_only: false,
            })),
        };

        if let Some(rc) = structure.reference_counter() {
            structure.add_reference_for_items(&rc)?;
        }

        Ok(structure)
    }

    /// Creates a struct without a reference counter.
    pub fn new_untracked(items: Vec<StackItem>) -> Self {
        Self {
            inner: Arc::new(Mutex::new(StructInner {
                items,
                id: next_stack_item_id(),
                reference_counter: None,
                is_read_only: false,
            })),
        }
    }

    /// Returns the unique identifier for this struct (used for reference equality).
    pub fn id(&self) -> usize {
        self.inner.lock().id
    }

    /// Returns the reference counter assigned by the reference counter, if any.
    pub fn reference_counter(&self) -> Option<ReferenceCounter> {
        self.inner.lock().reference_counter.clone()
    }

    /// Returns whether the struct is marked as read-only.
    pub fn is_read_only(&self) -> bool {
        self.inner.lock().is_read_only
    }

    /// Sets the read-only state of the struct.
    pub fn set_read_only(&self, read_only: bool) {
        self.inner.lock().is_read_only = read_only;
    }

    /// Gets the items in the struct.
    pub fn items(&self) -> Vec<StackItem> {
        self.inner.lock().items.clone()
    }

    /// Returns a stable pointer used for identity tracking.
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
        let mut inner = self.inner.lock();
        if index >= inner.items.len() {
            return Err(VmError::invalid_operation_msg(format!(
                "Index out of range: {index}"
            )));
        }

        Self::ensure_mutable(&inner)?;
        if let Some(rc) = &inner.reference_counter {
            Self::validate_compound_reference(rc, &item)?;
            let parent = CompoundParent::Struct(inner.id);
            rc.remove_compound_reference(&inner.items[index], parent);
            rc.add_compound_reference(&item, parent);
        }

        inner.items[index] = item;
        Ok(())
    }

    /// Adds an item to the end of the struct.
    pub fn push(&self, item: StackItem) -> VmResult<()> {
        let mut inner = self.inner.lock();
        Self::ensure_mutable(&inner)?;
        if let Some(rc) = &inner.reference_counter {
            Self::validate_compound_reference(rc, &item)?;
            rc.add_compound_reference(&item, CompoundParent::Struct(inner.id));
        }
        inner.items.push(item);
        Ok(())
    }

    /// Removes and returns the last item in the struct.
    pub fn pop(&self) -> VmResult<StackItem> {
        let mut inner = self.inner.lock();
        Self::ensure_mutable(&inner)?;
        let item = inner
            .items
            .pop()
            .ok_or_else(|| VmError::invalid_operation_msg("Struct is empty"))?;

        if let Some(rc) = &inner.reference_counter {
            rc.remove_compound_reference(&item, CompoundParent::Struct(inner.id));
        }

        Ok(item)
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
        let removed = inner.items.remove(index);

        if let Some(rc) = &inner.reference_counter {
            rc.remove_compound_reference(&removed, CompoundParent::Struct(inner.id));
        }

        Ok(removed)
    }

    /// Gets the number of items in the struct.
    pub fn len(&self) -> usize {
        self.inner.lock().items.len()
    }

    /// Returns true if the struct is empty.
    pub fn is_empty(&self) -> bool {
        self.inner.lock().items.is_empty()
    }

    /// Removes all items from the struct.
    pub fn clear(&self) -> VmResult<()> {
        let mut inner = self.inner.lock();
        Self::ensure_mutable(&inner)?;
        if let Some(rc) = &inner.reference_counter {
            let parent = CompoundParent::Struct(inner.id);
            for item in &inner.items {
                rc.remove_compound_reference(item, parent);
            }
        }
        inner.items.clear();
        Ok(())
    }

    /// Returns an iterator over the items.
    pub fn iter(&self) -> std::vec::IntoIter<StackItem> {
        self.items().into_iter()
    }

    /// Creates a deep copy of the struct.
    pub fn deep_copy(&self, reference_counter: Option<ReferenceCounter>) -> VmResult<Self> {
        let copy = Self::new(
            self.items()
                .into_iter()
                .map(|item| item.deep_clone())
                .collect(),
            reference_counter,
        )?;
        copy.set_read_only(true);
        Ok(copy)
    }

    /// Clones the struct respecting execution limits (mirrors C# Struct.Clone).
    pub fn clone_with_limits(
        &self,
        limits: &crate::execution_engine_limits::ExecutionEngineLimits,
    ) -> VmResult<Self> {
        let mut remaining = (limits.max_stack_size as i64) - 1;
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

        let clone = Struct::new(Vec::new(), self.reference_counter())?;

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
    pub fn stack_item_type(&self) -> StackItemType {
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

    fn add_reference_for_items(&self, rc: &ReferenceCounter) -> VmResult<()> {
        let items = self.items();
        let parent = CompoundParent::Struct(self.id());
        for item in &items {
            Self::validate_compound_reference(rc, item)?;
            rc.add_compound_reference(item, parent);
        }
        Ok(())
    }

    fn validate_compound_reference(rc: &ReferenceCounter, item: &StackItem) -> VmResult<()> {
        match item {
            StackItem::Array(inner) => match inner.reference_counter() {
                Some(child_rc) if child_rc.ptr_eq(rc) => Ok(()),
                Some(_) | None => Err(VmError::invalid_operation_msg(
                    "Can not set a Struct without a ReferenceCounter.".to_string(),
                )),
            },
            StackItem::Struct(inner) => match inner.reference_counter() {
                Some(child_rc) if child_rc.ptr_eq(rc) => Ok(()),
                Some(_) | None => Err(VmError::invalid_operation_msg(
                    "Can not set a Struct without a ReferenceCounter.".to_string(),
                )),
            },
            StackItem::Map(inner) => match inner.reference_counter() {
                Some(child_rc) if child_rc.ptr_eq(rc) => Ok(()),
                Some(_) | None => Err(VmError::invalid_operation_msg(
                    "Can not set a Struct without a ReferenceCounter.".to_string(),
                )),
            },
            _ => Ok(()),
        }
    }

    /// Ensures the struct and its children share the provided reference counter.
    pub(crate) fn attach_reference_counter(&self, rc: &ReferenceCounter) -> VmResult<()> {
        {
            let mut inner = self.inner.lock();
            if let Some(existing) = &inner.reference_counter {
                if existing.ptr_eq(rc) {
                    return Ok(());
                }
                return Err(VmError::invalid_operation_msg(
                    "Struct has mismatched reference counter.",
                ));
            }

            for item in &mut inner.items {
                item.attach_reference_counter(rc)?;
            }

            inner.reference_counter = Some(rc.clone());
        }

        self.add_reference_for_items(rc)?;
        Ok(())
    }
}

impl From<Struct> for Vec<StackItem> {
    fn from(structure: Struct) -> Self {
        structure.items()
    }
}
