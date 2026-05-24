use super::{WitnessCondition, WitnessRule};
use crate::vm_runtime::StackItem;
use neo_vm_rs::StackValue;

impl WitnessCondition {
    /// Converts the witness condition to a neo-vm-rs stack value (matches C# `WitnessCondition.ToStackItem` layout).
    pub fn to_stack_value(&self) -> StackValue {
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

    /// Converts the witness condition to a VM stack item (matches C# `WitnessCondition.ToStackItem`).
    pub fn to_stack_item(&self) -> StackItem {
        StackItem::try_from(self.to_stack_value()).expect(
            "witness condition StackValue projection uses only VM StackItem-compatible values",
        )
    }
}

impl WitnessRule {
    /// Converts the witness rule to a neo-vm-rs stack value (matches C# `WitnessRule.ToStackItem` layout).
    pub fn to_stack_value(&self) -> StackValue {
        StackValue::Array(vec![
            StackValue::Integer(i64::from(self.action.to_byte())),
            self.condition.to_stack_value(),
        ])
    }

    /// Converts the witness rule to a VM stack item (matches C# `WitnessRule.ToStackItem`).
    pub fn to_stack_item(&self) -> StackItem {
        StackItem::try_from(self.to_stack_value())
            .expect("witness rule StackValue projection uses only VM StackItem-compatible values")
    }
}
