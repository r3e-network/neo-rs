//! Reference counter interface mirroring `Neo.VM/IReferenceCounter.cs`.

use crate::stack_item::StackItem;

/// Interface describing the behaviour required by the VM reference counter.
///
/// The C# VM wires this interface into all compound stack items so that they can
/// notify the reference counter whenever references are added or removed. We keep
/// the same high-level contract in Rust to preserve the architecture, even though
/// the implementation details differ.
pub trait IReferenceCounter {
    /// Returns the total number of references currently tracked.
    fn count(&self) -> usize;

    /// Adds an item to the zero-referred set so it can be checked for cleanup later.
    fn add_zero_referred(&self, item: &StackItem);

    /// Adds a parent/child reference relationship for compound items.
    fn add_reference(&self, item: &StackItem, parent: &StackItem);

    /// Adds `count` stack references for the supplied item (defaults to `1`).
    fn add_stack_reference(&self, item: &StackItem, count: usize);

    /// Removes a parent/child reference relationship.
    fn remove_reference(&self, item: &StackItem, parent: &StackItem);

    /// Removes a stack reference from the supplied item.
    fn remove_stack_reference(&self, item: &StackItem);

    /// Scans the zero-referred set looking for garbage-collectable components.
    fn check_zero_referred(&self) -> usize;
}
