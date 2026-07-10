use super::{WitnessCondition, WitnessRule};
use neo_vm::{Interoperable, InteroperableError};
use neo_vm_rs::StackValue;

/// Projects witness-rule types to the lean neo-vm-rs [`StackValue`] form.
///
/// This lives in neo-core (which depends on the VM crate) rather than in neo-io
/// so that neo-io — a Layer-1 serialization crate — stays free of any VM
/// dependency. The projection is provided here as inherent methods on the
/// witness-rule types (defined in this crate).
impl WitnessCondition {
    /// Converts to a neo-vm-rs stack value (matches C# `WitnessCondition.ToStackItem` layout).
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
}

impl WitnessRule {
    /// Converts to a neo-vm-rs stack value (matches C# `WitnessRule.ToStackItem` layout).
    pub fn to_stack_value(&self) -> StackValue {
        StackValue::Array(vec![
            StackValue::Integer(i64::from(self.action.to_byte())),
            self.condition.to_stack_value(),
        ])
    }
}

impl Interoperable for WitnessCondition {
    fn from_stack_value(&mut self, _value: StackValue) -> Result<(), InteroperableError> {
        Err(InteroperableError::NotSupported(
            "WitnessCondition::from_stack_value is not supported".into(),
        ))
    }

    fn to_stack_value(&self) -> Result<StackValue, InteroperableError> {
        Ok(self.to_stack_value())
    }
}

impl Interoperable for WitnessRule {
    fn from_stack_value(&mut self, _value: StackValue) -> Result<(), InteroperableError> {
        Err(InteroperableError::NotSupported(
            "WitnessRule::from_stack_value is not supported".into(),
        ))
    }

    fn to_stack_value(&self) -> Result<StackValue, InteroperableError> {
        Ok(self.to_stack_value())
    }
}
