//! Evaluation stack module for the Neo Virtual Machine.
//!
//! This module represents a stack used by the Neo VM for executing scripts.

use crate::error::{VmError, VmResult};
use crate::execution_profile::StackProfileHandle;
use crate::reference_counter::ReferenceCounter;
use crate::stack_item::StackItem;

/// Represents the evaluation stack in the VM.
#[derive(Clone)]
pub struct EvaluationStack {
    /// The underlying storage for the stack. The top of the stack is the
    /// element at the end of the vector, matching the C# implementation.
    stack: Vec<StackItem>,

    /// Reference counter responsible for tracking stack references.
    reference_counter: ReferenceCounter,

    /// Present only for explicitly profiled engines.
    profile: Option<StackProfileHandle>,
}

impl EvaluationStack {
    /// Creates a new evaluation stack with the specified reference counter.
    #[inline]
    #[must_use]
    pub fn new(reference_counter: ReferenceCounter) -> Self {
        Self {
            stack: Vec::with_capacity(32), // Pre-allocate for typical stack usage
            reference_counter,
            profile: None,
        }
    }

    /// Attaches this stack to an explicitly enabled execution profile.
    pub(crate) fn set_profile(&mut self, profile: StackProfileHandle) {
        profile.observe_depth(self.stack.len());
        self.profile = Some(profile);
    }

    /// Returns the reference counter for this evaluation stack.
    #[inline]
    #[must_use]
    pub const fn reference_counter(&self) -> &ReferenceCounter {
        &self.reference_counter
    }

    /// Returns the number of items on the stack.
    #[inline]
    #[must_use]
    pub fn len(&self) -> usize {
        self.stack.len()
    }

    /// Indicates whether the stack is empty.
    #[inline]
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.stack.is_empty()
    }

    /// Pushes an item onto the top of the stack.
    #[inline(always)]
    pub fn push(&mut self, mut item: StackItem) -> VmResult<()> {
        item.attach_reference_counter(&self.reference_counter)?;
        self.reference_counter.add_stack_reference(&item, 1);
        self.stack.push(item);
        if let Some(profile) = &self.profile {
            profile.record_push(self.stack.len());
        }
        Ok(())
    }

    /// Removes and returns the item at the top of the stack.
    #[inline(always)]
    pub fn pop(&mut self) -> VmResult<StackItem> {
        let item = self.remove_internal(0)?;
        if let Some(profile) = &self.profile {
            profile.record_pop();
        }
        Ok(item)
    }

    /// Returns the item at the specified index counting from the top of the
    /// stack (0-based) without removing it.
    // Rationale: VM stack peeks are hot-path operations; bounds are checked
    // explicitly before the unchecked access.
    #[allow(unsafe_code)]
    #[inline(always)]
    pub fn peek(&self, index_from_top: usize) -> VmResult<&StackItem> {
        // Fast path: bounds check and index calculation
        if index_from_top >= self.stack.len() {
            return Err(VmError::stack_underflow_msg(0, 0));
        }
        if let Some(profile) = &self.profile {
            profile.record_peek();
        }
        // SAFETY: We just verified the index is within bounds
        unsafe {
            Ok(self
                .stack
                .get_unchecked(self.stack.len() - index_from_top - 1))
        }
    }

    /// Mutable version of [`Self::peek`].
    // Rationale: VM stack mutable peeks are hot-path operations; bounds are
    // checked explicitly before the unchecked access.
    #[allow(unsafe_code)]
    #[inline(always)]
    pub fn peek_mut(&mut self, index_from_top: usize) -> VmResult<&mut StackItem> {
        // Fast path: bounds check and index calculation
        if index_from_top >= self.stack.len() {
            return Err(VmError::stack_underflow_msg(0, 0));
        }
        if let Some(profile) = &self.profile {
            profile.record_mutable_peek();
        }
        let idx = self.stack.len() - index_from_top - 1;
        // SAFETY: We just verified the index is within bounds
        unsafe { Ok(self.stack.get_unchecked_mut(idx)) }
    }

    /// Inserts an item at the specified index counting from the top of the
    /// stack (0-based). Passing `0` is equivalent to `push`, while passing
    /// `len()` inserts the item at the bottom of the stack.
    pub fn insert(&mut self, index_from_top: usize, mut item: StackItem) -> VmResult<()> {
        if index_from_top > self.stack.len() {
            return Err(VmError::invalid_operation_msg("Insert index out of range"));
        }

        item.attach_reference_counter(&self.reference_counter)?;
        self.reference_counter.add_stack_reference(&item, 1);
        let insert_pos = self.stack.len().saturating_sub(index_from_top);
        self.stack.insert(insert_pos, item);
        if let Some(profile) = &self.profile {
            profile.record_insert(self.stack.len());
        }
        Ok(())
    }

    /// Swaps the items located at the supplied top-based indices.
    pub fn swap(&mut self, index_a: usize, index_b: usize) -> VmResult<()> {
        if index_a >= self.stack.len() || index_b >= self.stack.len() {
            return Err(VmError::stack_underflow_msg(0, 0));
        }
        if let Some(profile) = &self.profile {
            profile.record_swap();
        }
        if index_a == index_b {
            return Ok(());
        }

        let a = self.resolve_top_index(index_a)?;
        let b = self.resolve_top_index(index_b)?;
        self.stack.swap(a, b);
        Ok(())
    }

    /// Removes and returns the item at the specified index counting from the
    /// top of the stack (0-based).
    pub fn remove(&mut self, index_from_top: usize) -> VmResult<StackItem> {
        let item = self.remove_internal(index_from_top)?;
        if let Some(profile) = &self.profile {
            profile.record_remove();
        }
        Ok(item)
    }

    /// Reverses the order of the `count` items at the top of the stack.
    pub fn reverse(&mut self, count: usize) -> VmResult<()> {
        if count > self.stack.len() {
            return Err(VmError::invalid_operation_msg("Reverse count out of range"));
        }
        if let Some(profile) = &self.profile {
            profile.record_reverse();
        }
        if count <= 1 {
            return Ok(());
        }

        let start = self.stack.len() - count;
        self.stack[start..].reverse();
        Ok(())
    }

    /// Copies `count` items (default: all) from the top of this stack to the
    /// target stack without removing them from the source stack.
    pub fn copy_to(&self, target: &mut Self, count: Option<usize>) -> VmResult<()> {
        let count = count.unwrap_or(self.stack.len());
        if count > self.stack.len() {
            return Err(VmError::invalid_operation_msg("Copy count out of range"));
        }
        if count == 0 {
            if let Some(profile) = &self.profile {
                profile.record_copy(0);
            }
            return Ok(());
        }

        let start = self.stack.len() - count;
        for item in &self.stack[start..] {
            ensure_reference_counter_compatible(item, target.reference_counter())?;
        }
        for item in &self.stack[start..] {
            target.push(item.clone())?;
        }
        if let Some(profile) = &self.profile {
            profile.record_copy(count);
        }
        Ok(())
    }

    /// Moves `count` items (default: all) from the top of this stack to the
    /// target stack.
    pub fn move_to(&mut self, target: &mut Self, count: Option<usize>) -> VmResult<()> {
        let count = count.unwrap_or(self.stack.len());
        if count > self.stack.len() {
            return Err(VmError::invalid_operation_msg("Move count out of range"));
        }
        if count == 0 {
            if let Some(profile) = &self.profile {
                profile.record_move(0);
            }
            return Ok(());
        }

        let start = self.stack.len() - count;
        for item in &self.stack[start..] {
            ensure_reference_counter_compatible(item, target.reference_counter())?;
        }

        // Transfer ownership of the tail slice to the target stack.
        let mut moved = self.stack.split_off(start);
        for item in &moved {
            self.reference_counter.remove_stack_reference(item);
        }
        for item in moved.drain(..) {
            target.push(item)?;
        }
        if let Some(profile) = &self.profile {
            profile.record_move(count);
        }
        Ok(())
    }

    /// Clears the stack, removing all elements and releasing their references.
    pub fn clear(&mut self) {
        if let Some(profile) = &self.profile {
            profile.record_clear(self.stack.len());
        }
        for item in &self.stack {
            self.reference_counter.remove_stack_reference(item);
        }
        self.stack.clear();
    }

    /// Iterates over the stack items from bottom to top.
    pub fn iter(&self) -> std::slice::Iter<'_, StackItem> {
        self.stack.iter()
    }

    /// Mutable iterator over the stack items from bottom to top.
    pub fn iter_mut(&mut self) -> std::slice::IterMut<'_, StackItem> {
        self.stack.iter_mut()
    }

    /// Returns a cloned vector of the stack contents in bottom-to-top order.
    #[must_use]
    pub fn to_vec(&self) -> Vec<StackItem> {
        self.stack.clone()
    }

    #[inline(always)]
    fn resolve_top_index(&self, index_from_top: usize) -> VmResult<usize> {
        if index_from_top >= self.stack.len() {
            return Err(VmError::stack_underflow_msg(0, 0));
        }
        Ok(self.stack.len() - index_from_top - 1)
    }

    #[inline(always)]
    fn remove_internal(&mut self, index_from_top: usize) -> VmResult<StackItem> {
        let idx = self.resolve_top_index(index_from_top)?;
        let item = self.stack.remove(idx);
        self.reference_counter.remove_stack_reference(&item);
        Ok(item)
    }
}

fn ensure_reference_counter_compatible(item: &StackItem, rc: &ReferenceCounter) -> VmResult<()> {
    match item {
        StackItem::Array(array) => match array.reference_counter() {
            Some(existing) if existing.ptr_eq(rc) => Ok(()),
            Some(_) => Err(VmError::invalid_operation_msg(
                "Array has mismatched reference counter.",
            )),
            None => {
                for child in array.items() {
                    ensure_reference_counter_compatible(&child, rc)?;
                }
                Ok(())
            }
        },
        StackItem::Struct(structure) => match structure.reference_counter() {
            Some(existing) if existing.ptr_eq(rc) => Ok(()),
            Some(_) => Err(VmError::invalid_operation_msg(
                "Struct has mismatched reference counter.",
            )),
            None => {
                for child in structure.items() {
                    ensure_reference_counter_compatible(&child, rc)?;
                }
                Ok(())
            }
        },
        StackItem::Map(map) => match map.reference_counter() {
            Some(existing) if existing.ptr_eq(rc) => Ok(()),
            Some(_) => Err(VmError::invalid_operation_msg(
                "Map has mismatched reference counter.",
            )),
            None => {
                for (key, value) in map.items().iter() {
                    ensure_reference_counter_compatible(key, rc)?;
                    ensure_reference_counter_compatible(value, rc)?;
                }
                Ok(())
            }
        },
        _ => Ok(()),
    }
}

impl Drop for EvaluationStack {
    fn drop(&mut self) {
        self.clear();
    }
}

#[cfg(test)]
#[path = "../tests/runtime/evaluation_stack.rs"]
mod tests;
