use super::{WitnessCondition, WitnessRule};
use neo_vm::{Interoperable, InteroperableError, StackItem};

/// Projects witness-rule types to the local neo-vm [`StackItem`] form.
///
/// This lives in neo-core (which depends on the VM crate) rather than in neo-io
/// so that neo-io — a Layer-1 serialization crate — stays free of any VM
/// dependency. The projection is provided here as inherent methods on the
/// witness-rule types (defined in this crate).
impl WitnessCondition {
    /// Converts to a neo-vm stack item (matches C# `WitnessCondition.ToStackItem` layout).
    pub fn to_stack_item(&self) -> StackItem {
        let mut items = vec![StackItem::from_i64(i64::from(
            self.condition_type().to_byte(),
        ))];

        match self {
            WitnessCondition::Boolean { value } => {
                items.push(StackItem::from_bool(*value));
            }
            WitnessCondition::Not { condition } => {
                items.push(condition.to_stack_item());
            }
            WitnessCondition::And { conditions } | WitnessCondition::Or { conditions } => {
                let expressions = conditions
                    .iter()
                    .map(WitnessCondition::to_stack_item)
                    .collect::<Vec<_>>();
                items.push(StackItem::from_array(expressions));
            }
            WitnessCondition::ScriptHash { hash } | WitnessCondition::CalledByContract { hash } => {
                items.push(StackItem::from_byte_string(hash.to_bytes()));
            }
            WitnessCondition::Group { group } | WitnessCondition::CalledByGroup { group } => {
                items.push(StackItem::from_byte_string(group.clone()));
            }
            WitnessCondition::CalledByEntry => {}
        }

        StackItem::from_array(items)
    }
}

impl WitnessRule {
    /// Converts to a neo-vm stack item (matches C# `WitnessRule.ToStackItem` layout).
    pub fn to_stack_item(&self) -> StackItem {
        StackItem::from_array(vec![
            StackItem::from_i64(i64::from(self.action.to_byte())),
            self.condition.to_stack_item(),
        ])
    }
}

impl Interoperable for WitnessCondition {
    fn from_stack_item(&mut self, _value: StackItem) -> Result<(), InteroperableError> {
        Err(InteroperableError::NotSupported(
            "WitnessCondition::from_stack_item is not supported".into(),
        ))
    }

    fn to_stack_item(&self) -> Result<StackItem, InteroperableError> {
        Ok(WitnessCondition::to_stack_item(self))
    }
}

impl Interoperable for WitnessRule {
    fn from_stack_item(&mut self, _value: StackItem) -> Result<(), InteroperableError> {
        Err(InteroperableError::NotSupported(
            "WitnessRule::from_stack_item is not supported".into(),
        ))
    }

    fn to_stack_item(&self) -> Result<StackItem, InteroperableError> {
        Ok(WitnessRule::to_stack_item(self))
    }
}
