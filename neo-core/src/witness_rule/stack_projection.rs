use super::{WitnessCondition, WitnessRule};
use crate::neo_vm::StackItem;
use neo_vm_rs::StackValue;

/// Extension trait for types that can be projected to VM stack items.
///
/// This is needed because `WitnessCondition` and `WitnessRule` are now defined
/// in `neo-io`, so we cannot add inherent `to_stack_item()` methods from `neo-core`.
/// The `to_stack_value()` methods are inherent on the types (defined in `neo-io`).
pub trait ToStackItem {
    /// Converts to a VM stack item.
    fn to_stack_item(&self) -> StackItem;
}

/// Converts a `StackValue` to a `StackItem`, panicking on conversion failure.
///
/// This is safe because witness rule StackValue projections only use
/// VM StackItem-compatible values.
fn stack_value_to_item(value: StackValue) -> StackItem {
    StackItem::try_from(value)
        .expect("witness rule StackValue projection uses only VM StackItem-compatible values")
}

impl ToStackItem for WitnessCondition {
    /// Converts the witness condition to a VM stack item (matches C# `WitnessCondition.ToStackItem`).
    fn to_stack_item(&self) -> StackItem {
        stack_value_to_item(self.to_stack_value())
    }
}

impl ToStackItem for WitnessRule {
    /// Converts the witness rule to a VM stack item (matches C# `WitnessRule.ToStackItem`).
    fn to_stack_item(&self) -> StackItem {
        stack_value_to_item(self.to_stack_value())
    }
}
