//! `Interoperable` implementation macro.
//!
//! Seven manifest/state types bridge [`neo_vm::Interoperable`] to their JSON-form
//! `from_stack_value`/`to_stack_value` with exactly the same control flow: convert
//! the `StackItem`, map the VM error, delegate, map again, and box a clone. Only
//! the type name and the diagnostic text differ, so the flow is stated once here
//! and the diagnostics stay at the call site.
//!
//! Types with different semantics keep hand-written impls — `ContractGroup`
//! logs a failed conversion and keeps the previous value, and the protocol
//! projections need their own error mapping.

/// Implements [`neo_vm::Interoperable`] for a type that already exposes
/// `from_stack_value`/`to_stack_value`.
///
/// Both message literals are `format!` templates and must interpolate the
/// `error` binding, e.g. `"…: {error}"`. The binding is passed explicitly
/// (`format!(…, error = error)`) because a `macro_rules!` metavariable is not a
/// literal token, so implicit named-argument capture does not apply.
macro_rules! impl_interoperable {
    ($ty:ty, $from_msg:literal, $to_msg:literal) => {
        impl Interoperable for $ty {
            fn from_stack_item(
                &mut self,
                stack_item: StackItem,
            ) -> std::result::Result<(), neo_vm::VmError> {
                let stack_value = StackValue::try_from(stack_item).map_err(|error| {
                    neo_vm::VmError::invalid_operation_msg(format!($from_msg, error = error))
                })?;
                self.from_stack_value(stack_value)
                    .map_err(|error| neo_vm::VmError::invalid_operation_msg(error.to_string()))
            }

            fn to_stack_item(&self) -> std::result::Result<StackItem, neo_vm::VmError> {
                StackItem::try_from(self.to_stack_value()).map_err(|error| {
                    neo_vm::VmError::invalid_operation_msg(format!($to_msg, error = error))
                })
            }

            fn clone_box(&self) -> Box<dyn Interoperable> {
                Box::new(self.clone())
            }
        }
    };
}
