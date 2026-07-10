//! Smart-contract interoperable projection helpers.
//!
//! Neo native state types expose an `IInteroperable`-style projection to and
//! from VM stack values. This trait stays in `neo-vm` because it is used by
//! payload, manifest, and execution crates without depending on the shared
//! `neo-vm-rs` interpreter crate.

use neo_vm_rs::StackValue;

/// Error raised while projecting a host type to or from a VM stack value.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum InteroperableError {
    /// The stack value had the wrong shape or type.
    #[error("invalid type: {0}")]
    InvalidType(String),
    /// The stack value had the right general shape but invalid contents.
    #[error("invalid data: {0}")]
    InvalidData(String),
    /// This projection direction is intentionally unsupported.
    #[error("not supported: {0}")]
    NotSupported(String),
}

neo_error::impl_error_from_struct!(neo_error::CoreError, InteroperableError => InvalidOperation);

/// Host type that can project itself to and from a NeoVM stack value.
// Rationale: `from_stack_value` follows the C# host-object mutation naming
// rather than Rust's usual associated-constructor convention.
#[allow(clippy::wrong_self_convention)]
pub trait Interoperable: Send + Sync {
    /// Updates this value from a VM stack value.
    fn from_stack_value(&mut self, value: StackValue) -> Result<(), InteroperableError>;

    /// Converts this value into a VM stack value.
    fn to_stack_value(&self) -> Result<StackValue, InteroperableError>;
}

/// Implement [`Interoperable`] for a type that already has
/// `to_stack_value()` / `from_stack_value()` inherent methods.
#[macro_export]
macro_rules! impl_interoperable_via_stack_value {
    ($ty:ty) => {
        impl $crate::Interoperable for $ty {
            fn from_stack_value(
                &mut self,
                value: ::neo_vm_rs::StackValue,
            ) -> ::std::result::Result<(), $crate::InteroperableError> {
                *self = <$ty>::from_stack_value(value)
                    .map_err(|error| $crate::InteroperableError::InvalidData(format!("{error}")))?;
                Ok(())
            }

            fn to_stack_value(
                &self,
            ) -> ::std::result::Result<::neo_vm_rs::StackValue, $crate::InteroperableError> {
                Ok(<$ty>::to_stack_value(&*self))
            }
        }
    };
}

/// Re-export the VM [`crate::StackItem`] so callers can depend on this module
/// without importing the stateful VM stack type directly.
pub type SmartContractStackItem = crate::StackItem;
