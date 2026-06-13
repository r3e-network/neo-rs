//! Complete port of `Neo.SmartContract.Interoperable` from the C# reference implementation.

use crate::StackItem;
use crate::error::VmError;

/// Represents the object that can be converted to and from [`StackItem`].
///
/// This trait mirrors the C# `Neo.SmartContract.Interoperable` interface, enabling
/// smart-contract state to round-trip through VM stack items.
///
/// # Errors
///
/// Methods return `Result` so that unsupported conversions and invalid data
/// propagate gracefully instead of crashing the node via `panic!`.
#[allow(clippy::wrong_self_convention)]
pub trait Interoperable: std::fmt::Debug + Send + Sync {
    /// Convert a [`StackItem`] to the current object.
    fn from_stack_item(&mut self, stack_item: StackItem) -> Result<(), VmError>;

    /// Convert the current object to a [`StackItem`].
    fn to_stack_item(&self) -> Result<StackItem, VmError>;

    /// Create a boxed clone of the interoperable instance.
    fn clone_box(&self) -> Box<dyn Interoperable>;

    /// Populate the current instance by cloning the provided interoperable value.
    fn from_replica(&mut self, replica: &dyn Interoperable) -> Result<(), VmError> {
        self.from_stack_item(replica.to_stack_item()?)
    }
}

/// Re-export the VM [`StackItem`] so callers can depend on the smart-contract module
/// without importing the VM crate directly.
pub type SmartContractStackItem = StackItem;
