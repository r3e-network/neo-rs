//! Smart-contract interoperable projection helpers.
//!
//! Neo native state types expose an `IInteroperable`-style projection to and
//! from the canonical VM [`StackItem`](crate::StackItem). This trait stays in
//! `neo-vm` because payload, manifest, and execution crates all project values
//! directly onto the local VM stack.

use crate::StackItem;

/// Error raised while projecting a host type to or from a VM stack item.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum InteroperableError {
    /// The stack item had the wrong shape or type.
    #[error("invalid type: {0}")]
    InvalidType(String),
    /// The stack item had the right general shape but invalid contents.
    #[error("invalid data: {0}")]
    InvalidData(String),
    /// This projection direction is intentionally unsupported.
    #[error("not supported: {0}")]
    NotSupported(String),
}

neo_error::impl_error_from_struct!(neo_error::CoreError, InteroperableError => InvalidOperation);

/// Host type that can project itself to and from a NeoVM stack item.
// Rationale: `from_stack_item` follows the C# host-object mutation naming
// rather than Rust's usual associated-constructor convention.
#[allow(clippy::wrong_self_convention)]
pub trait Interoperable: Send + Sync {
    /// Updates this value from a VM stack item.
    fn from_stack_item(&mut self, value: StackItem) -> Result<(), InteroperableError>;

    /// Converts this value into a VM stack item.
    fn to_stack_item(&self) -> Result<StackItem, InteroperableError>;
}

/// Implement [`Interoperable`] for a type that already has
/// `to_stack_item()` / `from_stack_item()` inherent methods.
#[macro_export]
macro_rules! impl_interoperable_via_stack_item {
    ($ty:ty) => {
        impl $crate::Interoperable for $ty {
            fn from_stack_item(
                &mut self,
                value: $crate::StackItem,
            ) -> ::std::result::Result<(), $crate::InteroperableError> {
                *self = <$ty>::from_stack_item(&value)
                    .map_err(|error| $crate::InteroperableError::InvalidData(format!("{error}")))?;
                Ok(())
            }

            fn to_stack_item(
                &self,
            ) -> ::std::result::Result<$crate::StackItem, $crate::InteroperableError> {
                Ok(<$ty>::to_stack_item(&*self))
            }
        }
    };
}
