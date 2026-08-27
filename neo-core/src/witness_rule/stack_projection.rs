use super::{WitnessCondition, WitnessRule};
use crate::neo_vm::{StackItem, StackValue};

/// Projects Neo witness-rule types onto VM stack values / stack items.
///
/// `WitnessCondition` / `WitnessRule` are defined in `neo-io`, which must not
/// depend on the VM. The VM-facing projections therefore live here in
/// `neo-core` (which sits above `neo-io` and `neo-vm`) as trait methods rather
/// than inherent methods on the `neo-io` types. This keeps `neo-io` free of any
/// VM dependency and avoids an `neo-io <-> neo-vm` dependency cycle.
///
/// Layout matches C# `WitnessCondition.ToStackItem` / `WitnessRule.ToStackItem`.
pub trait ToStackValue {
    /// Converts to a VM stack value.
    fn to_stack_value(&self) -> StackValue;
}

/// Extension trait for types that can be projected to VM stack items.
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

impl ToStackValue for WitnessCondition {
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
                    .map(ToStackValue::to_stack_value)
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

impl ToStackValue for WitnessRule {
    fn to_stack_value(&self) -> StackValue {
        StackValue::Array(vec![
            StackValue::Integer(i64::from(self.action.to_byte())),
            self.condition.to_stack_value(),
        ])
    }
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
