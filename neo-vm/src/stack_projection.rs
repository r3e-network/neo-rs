//! Witness-rule stack projection.
//!
//! Distributed from `neo-core::witness_rule::stack_projection`.
//!
//! `WitnessCondition` / `WitnessRule` are defined in `neo-io`, which must not
//! depend on the VM. The VM-facing projections therefore live here in `neo-vm`
//! (which sits above `neo-io`) as trait methods rather than inherent methods on
//! the `neo-io` types. This keeps `neo-io` free of any VM dependency and avoids
//! an `neo-io <-> neo-vm` dependency cycle.
//!
//! Layout matches C# `WitnessCondition.ToStackItem` / `WitnessRule.ToStackItem`.

use crate::{StackItem, StackValue};
use neo_io_crate::witness_rule::{WitnessCondition, WitnessRule};

/// Projects Neo witness-rule types onto VM stack values.
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

/// Projects a witness condition without extending the protocol data owner.
pub fn witness_condition_to_stack_value(condition: &WitnessCondition) -> StackValue {
    let mut items = vec![StackValue::Integer(i64::from(
        condition.condition_type().to_byte(),
    ))];

    match condition {
        WitnessCondition::Boolean { value } => {
            items.push(StackValue::Boolean(*value));
        }
        WitnessCondition::Not { condition } => {
            items.push(witness_condition_to_stack_value(condition));
        }
        WitnessCondition::And { conditions } | WitnessCondition::Or { conditions } => {
            let expressions = conditions
                .iter()
                .map(witness_condition_to_stack_value)
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

/// Projects a witness rule without extending the protocol data owner.
pub fn witness_rule_to_stack_value(rule: &WitnessRule) -> StackValue {
    StackValue::Array(vec![
        StackValue::Integer(i64::from(rule.action.to_byte())),
        witness_condition_to_stack_value(&rule.condition),
    ])
}

impl ToStackValue for WitnessCondition {
    fn to_stack_value(&self) -> StackValue {
        witness_condition_to_stack_value(self)
    }
}

impl ToStackValue for WitnessRule {
    fn to_stack_value(&self) -> StackValue {
        witness_rule_to_stack_value(self)
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

#[cfg(test)]
mod tests {
    use super::*;
    use neo_io_crate::witness_rule::{WitnessConditionType, WitnessRuleAction};
    use neo_primitives::{UINT160_SIZE, UInt160};

    #[test]
    fn witness_rule_projects_to_neo_vm_rs_stack_value() {
        let hash = UInt160::from_bytes(&[0x11; UINT160_SIZE]).unwrap();
        let rule = WitnessRule::new(
            WitnessRuleAction::Allow,
            WitnessCondition::And {
                conditions: vec![
                    WitnessCondition::Boolean { value: true },
                    WitnessCondition::ScriptHash { hash },
                ],
            },
        );

        assert_eq!(
            rule.to_stack_value(),
            StackValue::Array(vec![
                StackValue::Integer(WitnessRuleAction::Allow.to_byte().into()),
                StackValue::Array(vec![
                    StackValue::Integer(WitnessConditionType::And.to_byte().into()),
                    StackValue::Array(vec![
                        StackValue::Array(vec![
                            StackValue::Integer(WitnessConditionType::Boolean.to_byte().into()),
                            StackValue::Boolean(true),
                        ]),
                        StackValue::Array(vec![
                            StackValue::Integer(WitnessConditionType::ScriptHash.to_byte().into()),
                            StackValue::ByteString(hash.to_bytes()),
                        ]),
                    ]),
                ]),
            ])
        );
    }

    #[test]
    fn witness_rule_stack_item_projection_matches_stack_value_projection() {
        let rule = WitnessRule::new(
            WitnessRuleAction::Deny,
            WitnessCondition::Not {
                condition: Box::new(WitnessCondition::CalledByEntry),
            },
        );

        let expected = StackItem::try_from(rule.to_stack_value()).unwrap();
        assert_eq!(rule.to_stack_item(), expected);
    }
}
