//! Interoperable trait re-exported from neo-vm.

pub use crate::neo_vm::Interoperable;

/// Re-export the VM [`StackItem`] so callers can depend on the smart-contract module
/// without importing the VM crate directly.
pub type SmartContractStackItem = crate::neo_vm::StackItem;
