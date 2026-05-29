use super::{WitnessCondition, WitnessRule};
use crate::neo_vm::StackItem;
use neo_vm_rs::StackValue;

/// Projects witness-rule types to the lean neo-vm-rs [`StackValue`] form.
///
/// This lives in neo-core (which depends on the VM crate) rather than in neo-io
/// so that neo-io — a Layer-1 serialization crate — stays free of any VM
/// dependency. `WitnessCondition`/`WitnessRule` are defined in neo-io, so the
/// projection is provided here via an extension trait.
pub trait WitnessStackValue {
    /// Converts to a neo-vm-rs stack value (matches C# `*.ToStackItem` layout).
    fn to_stack_value(&self) -> StackValue;
}

impl WitnessStackValue for WitnessCondition {
    fn to_stack_value(&self) -> StackValue {
        let mut items = vec![StackValue::Integer(i64::from(
            self.condition_type().to_byte(),
        ))];

        match self {
            WitnessCondition::Boolean { value } => {
                items.push(StackValue::Boolean(*value));
            }
            WitnessCondition::Not { condition } => {
                items.push(condition.to_stack_value());
            }
            WitnessCondition::And { conditions } | WitnessCondition::Or { conditions } => {
                let expressions = conditions
                    .iter()
                    .map(WitnessCondition::to_stack_value)
                    .collect::<Vec<_>>();
                items.push(StackValue::Array(expressions));
            }
            WitnessCondition::ScriptHash { hash } | WitnessCondition::CalledByContract { hash } => {
                items.push(StackValue::ByteString(hash.to_bytes()));
            }
            WitnessCondition::Group { group } | WitnessCondition::CalledByGroup { group } => {
                items.push(StackValue::ByteString(group.clone()));
            }
            WitnessCondition::CalledByEntry => {}
        }

        StackValue::Array(items)
    }
}

impl WitnessStackValue for WitnessRule {
    fn to_stack_value(&self) -> StackValue {
        StackValue::Array(vec![
            StackValue::Integer(i64::from(self.action.to_byte())),
            self.condition.to_stack_value(),
        ])
    }
}

/// Extension trait for projecting witness-rule types to a VM [`StackItem`].
pub trait ToStackItem {
    /// Converts to a VM stack item.
    fn to_stack_item(&self) -> StackItem;
}

/// Converts a `StackValue` to a `StackItem`.
///
/// Safe because witness-rule `StackValue` projections only use values that are
/// representable as VM `StackItem`s.
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
