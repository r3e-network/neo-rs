//! Struct stack item implementation for the Neo Virtual Machine.
//!
//! This module provides the Struct stack item implementation used in the Neo VM.

use crate::StackItemType;
use crate::error::{VmError, VmResult};
use crate::next_stack_item_id;
use crate::reference_counter::{CompoundId, ReferenceCounter};
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
    /// Reference counter shared with the VM (mirrors C# `CompoundType` semantics).
    reference_counter: Option<ReferenceCounter>,
    /// Indicates whether the struct is read-only.
    is_read_only: bool,
}

impl Struct {
    /// Creates a new struct with the specified items and reference counter.
    pub fn new(
        mut items: Vec<StackItem>,
        reference_counter: Option<ReferenceCounter>,
    ) -> VmResult<Self> {
        if let Some(rc) = &reference_counter {
            for item in &mut items {
                item.attach_reference_counter(rc)?;
            }
        }

        // C# v3.10.1: no reference counting on construction (see Array::new).
        let structure = Self {
            inner: Arc::new(Mutex::new(StructInner {
                items,
                id: next_stack_item_id() as usize,
                reference_counter,
                is_read_only: false,
            })),
        };

        Ok(structure)
    }

    /// Creates a struct without a reference counter.
    #[must_use]
    pub fn new_untracked(items: Vec<StackItem>) -> Self {
        Self::new_untracked_with_id(items, next_stack_item_id() as usize)
    }

    pub(crate) fn new_untracked_with_id(items: Vec<StackItem>, id: usize) -> Self {
        Self {
            inner: Arc::new(Mutex::new(StructInner {
                items,
                id,
                reference_counter: None,
                is_read_only: false,
            })),
        }
    }

    /// Returns the unique identifier for this struct (used for reference equality).
    #[must_use]
    pub fn id(&self) -> usize {
        self.inner.lock().id
    }

    /// Returns the reference counter assigned by the reference counter, if any.
    #[must_use]
    pub fn reference_counter(&self) -> Option<ReferenceCounter> {
        self.inner.lock().reference_counter.clone()
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
    pub fn set(&self, index: usize, mut item: StackItem) -> VmResult<()> {
        let (rc_opt, referenced, old_item, new_item) = {
            let inner = self.inner.lock();
            if index >= inner.items.len() {
                return Err(VmError::invalid_operation_msg(format!(
                    "Index out of range: {index}"
                )));
            }
            Self::ensure_mutable(&inner)?;
            let rc_opt = inner.reference_counter.clone();
            let referenced = rc_opt
                .as_ref()
                .is_some_and(|rc| rc.is_stack_referenced_id(CompoundId::Struct(inner.id)));
            (rc_opt, referenced, inner.items[index].clone(), item.clone())
        };
        if let Some(rc) = &rc_opt {
            item.attach_reference_counter(rc)?;
            Self::validate_compound_reference(rc, &item)?;
        }
        {
            let mut inner = self.inner.lock();
            if index >= inner.items.len() {
                return Err(VmError::invalid_operation_msg(format!(
                    "Index out of range: {index}"
                )));
            }
            Self::ensure_mutable(&inner)?;
            inner.items[index] = item;
        }
        if let Some(rc) = &rc_opt {
            if referenced {
                rc.remove_stack_reference(&old_item);
                rc.add_stack_reference(&new_item, 1);
            }
        }
        Ok(())
    }

    /// Adds an item to the end of the struct.
    pub fn push(&self, mut item: StackItem) -> VmResult<()> {
        let (rc_opt, referenced) = {
            let inner = self.inner.lock();
            Self::ensure_mutable(&inner)?;
            let rc_opt = inner.reference_counter.clone();
            let referenced = rc_opt
                .as_ref()
                .is_some_and(|rc| rc.is_stack_referenced_id(CompoundId::Struct(inner.id)));
            (rc_opt, referenced)
        };
        if let Some(rc) = &rc_opt {
            item.attach_reference_counter(rc)?;
            Self::validate_compound_reference(rc, &item)?;
        }
        {
            let mut inner = self.inner.lock();
            Self::ensure_mutable(&inner)?;
            inner.items.push(item.clone());
        }
        if let Some(rc) = &rc_opt {
            if referenced {
                rc.add_stack_reference(&item, 1);
            }
        }
        Ok(())
    }

    /// Removes and returns the last item in the struct.
    pub fn pop(&self) -> VmResult<StackItem> {
        let (item, rc_opt, referenced) = {
            let mut inner = self.inner.lock();
            Self::ensure_mutable(&inner)?;
            let item = inner
                .items
                .pop()
                .ok_or_else(|| VmError::invalid_operation_msg("Struct is empty"))?;
            let rc_opt = inner.reference_counter.clone();
            let referenced = rc_opt
                .as_ref()
                .is_some_and(|rc| rc.is_stack_referenced_id(CompoundId::Struct(inner.id)));
            (item, rc_opt, referenced)
        };
        if let Some(rc) = &rc_opt {
            if referenced {
                rc.remove_stack_reference(&item);
            }
        }

        Ok(item)
    }

    /// Removes the item at the specified index.
    pub fn remove(&self, index: usize) -> VmResult<StackItem> {
        let (removed, rc_opt, referenced) = {
            let mut inner = self.inner.lock();
            if index >= inner.items.len() {
                return Err(VmError::invalid_operation_msg(format!(
                    "Index out of range: {index}"
                )));
            }

            Self::ensure_mutable(&inner)?;
            let removed = inner.items.remove(index);
            let rc_opt = inner.reference_counter.clone();
            let referenced = rc_opt
                .as_ref()
                .is_some_and(|rc| rc.is_stack_referenced_id(CompoundId::Struct(inner.id)));
            (removed, rc_opt, referenced)
        };
        if let Some(rc) = &rc_opt {
            if referenced {
                rc.remove_stack_reference(&removed);
            }
        }

        Ok(removed)
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
        let (rc, sub_items) = {
            let mut inner = self.inner.lock();
            Self::ensure_mutable(&inner)?;
            let id = inner.id;
            let rc = inner.reference_counter.clone();
            let sub_items = if rc
                .as_ref()
                .is_some_and(|rc| rc.is_stack_referenced_id(CompoundId::Struct(id)))
            {
                inner.items.clone()
            } else {
                Vec::new()
            };
            inner.items.clear();
            (rc, sub_items)
        };
        if let Some(rc) = rc {
            for item in sub_items {
                rc.remove_stack_reference(&item);
            }
        }
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
    pub fn deep_copy(&self, reference_counter: Option<ReferenceCounter>) -> VmResult<Self> {
        let items = self.with_items(|items| items.iter().map(|item| item.deep_clone()).collect());
        let copy = Self::new(items, reference_counter)?;
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

        let clone = Self::new(Vec::new(), self.reference_counter())?;

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
        let children = {
            let mut inner = self.inner.lock();
            if let Some(existing) = &inner.reference_counter {
                if existing.ptr_eq(rc) {
                    return Ok(());
                }
                return Err(VmError::invalid_operation_msg(
                    "Struct has mismatched reference counter.",
                ));
            }

            let children = inner.items.clone();
            inner.reference_counter = Some(rc.clone());
            children
        };

        for mut item in children {
            item.attach_reference_counter(rc)?;
        }

        // No reference counting on attach (see Array::attach_reference_counter).
        Ok(())
    }
}

impl From<Struct> for Vec<StackItem> {
    fn from(structure: Struct) -> Self {
        structure.items()
    }
}
