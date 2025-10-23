//! Complete port of `Neo.SmartContract.IInteroperable` from the C# reference implementation.

use neo_vm::StackItem;

/// Represents the object that can be converted to and from [`StackItem`].
///
/// This trait mirrors the C# `Neo.SmartContract.IInteroperable` interface, enabling
/// smart-contract state to round-trip through VM stack items.
pub trait IInteroperable: std::fmt::Debug + Send + Sync {
    /// Convert a [`StackItem`] to the current object.
    fn from_stack_item(&mut self, stack_item: StackItem);

    /// Convert the current object to a [`StackItem`].
    fn to_stack_item(&self) -> StackItem;

    /// Create a boxed clone of the interoperable instance.
    fn clone_box(&self) -> Box<dyn IInteroperable>;

    /// Populate the current instance by cloning the provided interoperable value.
    fn from_replica(&mut self, replica: &dyn IInteroperable) {
        self.from_stack_item(replica.to_stack_item());
    }
}

/// Re-export the VM [`StackItem`] so callers can depend on the smart-contract module
/// without importing the VM crate directly.
pub type SmartContractStackItem = StackItem;
