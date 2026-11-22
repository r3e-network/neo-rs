//! Struct stack item implementation for the Neo Virtual Machine.
//!
//! This module provides the Struct stack item implementation used in the Neo VM.

use crate::StackItemType;
use crate::error::{VmError, VmResult};
use crate::reference_counter::{StackReferenceRelease, SubItems};
use crate::stack_item::StackItem;
use crate::stack_item::compound::CompoundInner;
use parking_lot::Mutex;
use std::collections::HashSet;
use std::sync::Arc;

/// Diagnostic reported when a struct is mutated after being frozen.
const READ_ONLY: &str = "The struct is readonly, can not modify.";
/// Diagnostic reported when an item is popped from an empty struct.
const EMPTY: &str = "Struct is empty";

/// Represents a struct of stack items in the VM.
#[derive(Debug, Clone)]
pub struct Struct {
    inner: Arc<Mutex<CompoundInner>>,
}

impl Struct {
    /// Creates a new struct with the specified items.
    ///
    /// As in neo-vm v3.10.1, a struct holds no reference counter and none of
    /// its methods touch one: the opcode handlers do the counting.
    #[must_use]
    pub fn new(items: Vec<StackItem>) -> Self {
        Self {
            inner: Arc::new(Mutex::new(CompoundInner::new(items))),
        }
    }

    /// Returns the unique identifier for this struct (used for reference equality).
    #[must_use]
    pub fn id(&self) -> usize {
        self.inner.lock().id
    }

    /// Returns the number of stack references held on this struct.
    #[must_use]
    pub fn stack_references(&self) -> isize {
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
        self.inner.lock().add_stack_references(count)
    }

    /// Adds `count` stack references without touching any sub item
    /// (C# `CompoundType.StackReferences++`).
    pub(crate) fn increment_stack_references(&self, count: usize) {
        self.inner.lock().inc_stack_references(count);
    }

    /// Removes `count` stack references without touching any sub item
    /// (C# `CompoundType.StackReferences--`).
    pub(crate) fn decrement_stack_references(&self, count: usize) {
        self.inner.lock().dec_stack_references(count);
    }

    /// Removes one stack reference, if the struct is stack referenced.
    pub(crate) fn remove_stack_reference(&self) -> StackReferenceRelease {
        self.inner.lock().remove_stack_reference()
    }

    /// Takes a snapshot of the sub items (C# `SubItems`).
    pub(crate) fn sub_items(&self) -> SubItems {
        self.inner.lock().sub_items()
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
            .get(index)
            .ok_or_else(|| VmError::invalid_operation_msg(format!("Index out of range: {index}")))
    }

    /// Sets the item at the specified index.
    pub fn set(&self, index: usize, item: StackItem) -> VmResult<()> {
        // The replaced item is dropped after the lock is released.
        let replaced = self.inner.lock().set(index, item, READ_ONLY)?;
        drop(replaced);
        Ok(())
    }

    /// Adds an item to the end of the struct.
    pub fn push(&self, item: StackItem) -> VmResult<()> {
        self.inner.lock().push(item, READ_ONLY)
    }

    /// Removes and returns the last item in the struct.
    pub fn pop(&self) -> VmResult<StackItem> {
        self.inner.lock().pop(EMPTY, READ_ONLY)
    }

    /// Removes the item at the specified index.
    pub fn remove(&self, index: usize) -> VmResult<StackItem> {
        self.inner.lock().remove(index, READ_ONLY)
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
        let removed = self.inner.lock().clear(READ_ONLY)?;
        drop(removed);
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
    ///
    /// Nested structs are cloned, everything else is shared, and one unit of
    /// the budget is spent per sub item, so the walk is bounded by
    /// `max_stack_size` instead of the native stack.
    pub fn clone_with_limits(&self, limits: &crate::ExecutionEngineLimits) -> VmResult<Self> {
        Ok(self.clone_with_limits_counted(limits)?.0)
    }

    /// Clones the struct like [`clone_with_limits`](Self::clone_with_limits),
    /// also reporting how many clone-budget units were spent.
    ///
    /// neo-vm v3.11.2 `Struct.Clone(limits, out nClonedItems)`. The dynamic
    /// price of SETITEM/APPEND/VALUES charges those units.
    pub fn clone_with_limits_counted(
        &self,
        limits: &crate::ExecutionEngineLimits,
    ) -> VmResult<(Self, i64)> {
        let budget = i64::from(limits.max_stack_size);
        let mut remaining = budget - 1;
        let mut visited = HashSet::new();
        let cloned = self.clone_with_remaining(&mut remaining, &mut visited)?;
        Ok((cloned, budget - remaining))
    }

    /// The traversal of [`clone_with_limits`](Self::clone_with_limits) on an
    /// explicit work stack: a struct nested as deep as `MaxStackSize` is
    /// copied without adding one native frame per level.
    fn clone_with_remaining(
        &self,
        remaining: &mut i64,
        visited: &mut HashSet<usize>,
    ) -> VmResult<Self> {
        /// One struct that is being copied: the rest of its sub items and the
        /// struct they are copied into.
        struct Frame {
            source_id: usize,
            target: Struct,
            items: std::vec::IntoIter<StackItem>,
        }

        let root_id = self.id();
        if !visited.insert(root_id) {
            return Err(VmError::invalid_operation_msg(
                "Beyond struct subitem clone limits!",
            ));
        }

        let clone = Self::new(Vec::new());
        let mut frames = vec![Frame {
            source_id: root_id,
            target: clone.clone(),
            items: self.items().into_iter(),
        }];

        while !frames.is_empty() {
            let last = frames.len() - 1;
            let Some(item) = frames[last].items.next() else {
                visited.remove(&frames[last].source_id);
                frames.pop();
                continue;
            };

            *remaining -= 1;
            if *remaining < 0 {
                return Err(VmError::invalid_operation_msg(
                    "Beyond struct subitem clone limits!",
                ));
            }

            let nested = match item {
                StackItem::Struct(inner) => {
                    if !visited.insert(inner.id()) {
                        return Err(VmError::invalid_operation_msg(
                            "Beyond struct subitem clone limits!",
                        ));
                    }
                    Some(inner)
                }
                shared => {
                    frames[last].target.push(shared)?;
                    None
                }
            };

            if let Some(inner) = nested {
                let target = Self::new(Vec::new());
                frames[last]
                    .target
                    .push(StackItem::Struct(target.clone()))?;
                frames.push(Frame {
                    source_id: inner.id(),
                    target,
                    items: inner.items().into_iter(),
                });
            }
        }

        Ok(clone)
    }

    /// One-lock snapshot of the fields the `Debug` renderer prints.
    pub(crate) fn debug_snapshot(&self) -> (Vec<StackItem>, usize, isize, bool) {
        self.inner.lock().debug_snapshot()
    }

    /// Gets the type of the stack item.
    #[must_use]
    pub const fn stack_item_type(&self) -> StackItemType {
        StackItemType::Struct
    }

    /// Reverses the order of items in the struct.
    pub fn reverse_items(&self) -> VmResult<()> {
        self.inner.lock().reverse(READ_ONLY)
    }
}

impl From<Struct> for Vec<StackItem> {
    fn from(structure: Struct) -> Self {
        structure.items()
    }
}
