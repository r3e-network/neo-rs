//! Re-export of `Interoperable` from `neo-vm-rs`.
//!
//! The canonical `Interoperable` trait lives in `neo-vm-rs` and operates on
//! [`neo_vm_rs::StackValue`]. This module re-exports it for backward
//! compatibility and provides a convenience macro for the common pattern
//! where a type has `to_stack_value()` / `from_stack_value()` methods.

// Re-export the canonical trait and error type from neo-vm-rs.
pub use neo_vm_rs::{Interoperable, InteroperableError};

/// Re-export the VM [`StackItem`] so callers can depend on the smart-contract module
/// without importing the VM crate directly.
pub type SmartContractStackItem = crate::StackItem;

/// Implement [`Interoperable`] for a type that already has
/// `to_stack_value(&self) -> StackValue` and
/// `from_stack_value(StackValue) -> Result<Self, E>`.
///
/// The error type `E` must implement `Display` so it can be converted
/// to [`InteroperableError::InvalidData`].
///
/// # Requirements
///
/// The type must:
/// - Be `Clone + Debug + Send + Sync + 'static`
/// - Have `fn to_stack_value(&self) -> neo_vm_rs::StackValue`
/// - Have `fn from_stack_value(sv: neo_vm_rs::StackValue) -> Result<Self, E>` where `E: Display`
#[macro_export]
macro_rules! impl_interoperable_via_stack_value {
    ($ty:ty) => {
        impl $crate::Interoperable for $ty {
            fn from_stack_value(
                &mut self,
                value: ::neo_vm_rs::StackValue,
            ) -> ::std::result::Result<(), $crate::InteroperableError> {
                // Use `<$ty>::from_stack_value` to call the inherent method,
                // not the trait method (which would be infinite recursion).
                *self = <$ty>::from_stack_value(value)
                    .map_err(|error| $crate::InteroperableError::InvalidData(format!("{error}")))?;
                Ok(())
            }

            fn to_stack_value(
                &self,
            ) -> ::std::result::Result<::neo_vm_rs::StackValue, $crate::InteroperableError> {
                // Use `<$ty>::to_stack_value` with an explicit reference to
                // call the inherent method, not the trait method (which would
                // be infinite recursion). We use `&*self` to get `&$ty` from
                // the `&self` parameter.
                Ok(<$ty>::to_stack_value(&*self))
            }

            fn clone_box(&self) -> ::std::boxed::Box<dyn $crate::Interoperable> {
                ::std::boxed::Box::new(self.clone())
            }
        }
    };
}
