use neo_io::witness_rule::{WitnessCondition, WitnessRule};
use neo_vm::{StackItem, StackValue};

/// Projects canonical witness-rule data onto VM stack values.
///
/// The trait is owned here so `neo-io` stays independent of the VM.
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

/// Converts the witness condition projection to a VM stack item.
pub fn witness_condition_to_stack_item(condition: &WitnessCondition) -> StackItem {
    stack_value_to_item(witness_condition_to_stack_value(condition))
}

/// Converts the witness rule projection to a VM stack item.
pub fn witness_rule_to_stack_item(rule: &WitnessRule) -> StackItem {
    stack_value_to_item(witness_rule_to_stack_value(rule))
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
        witness_condition_to_stack_item(self)
    }
}

impl ToStackItem for WitnessRule {
    /// Converts the witness rule to a VM stack item (matches C# `WitnessRule.ToStackItem`).
    fn to_stack_item(&self) -> StackItem {
        witness_rule_to_stack_item(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use neo_io::witness_rule::{WitnessConditionType, WitnessRuleAction};
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
            neo_vm::StackValue::Array(vec![
                neo_vm::StackValue::Integer(WitnessRuleAction::Allow.to_byte().into()),
                neo_vm::StackValue::Array(vec![
                    neo_vm::StackValue::Integer(WitnessConditionType::And.to_byte().into()),
                    neo_vm::StackValue::Array(vec![
                        neo_vm::StackValue::Array(vec![
                            neo_vm::StackValue::Integer(
                                WitnessConditionType::Boolean.to_byte().into()
                            ),
                            neo_vm::StackValue::Boolean(true),
                        ]),
                        neo_vm::StackValue::Array(vec![
                            neo_vm::StackValue::Integer(
                                WitnessConditionType::ScriptHash.to_byte().into()
                            ),
                            neo_vm::StackValue::ByteString(hash.to_bytes()),
                        ]),
                    ]),
                ]),
            ])
        );
        assert_eq!(rule.to_stack_value(), witness_rule_to_stack_value(&rule));
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
        assert_eq!(witness_rule_to_stack_item(&rule), expected);
    }

    #[test]
    fn witness_condition_projection_preserves_all_condition_layouts() {
        let hash = UInt160::from_bytes(&[0x22; UINT160_SIZE]).unwrap();
        let group = vec![0x03; 33];
        let boolean =
            StackValue::Array(vec![StackValue::Integer(0x00), StackValue::Boolean(false)]);
        let cases = [
            (WitnessCondition::Boolean { value: false }, boolean.clone()),
            (
                WitnessCondition::Not {
                    condition: Box::new(WitnessCondition::Boolean { value: false }),
                },
                StackValue::Array(vec![StackValue::Integer(0x01), boolean.clone()]),
            ),
            (
                WitnessCondition::And {
                    conditions: vec![WitnessCondition::Boolean { value: false }],
                },
                StackValue::Array(vec![
                    StackValue::Integer(0x02),
                    StackValue::Array(vec![boolean.clone()]),
                ]),
            ),
            (
                WitnessCondition::Or {
                    conditions: vec![WitnessCondition::Boolean { value: false }],
                },
                StackValue::Array(vec![
                    StackValue::Integer(0x03),
                    StackValue::Array(vec![boolean]),
                ]),
            ),
            (
                WitnessCondition::ScriptHash { hash },
                StackValue::Array(vec![
                    StackValue::Integer(0x18),
                    StackValue::ByteString(hash.to_bytes()),
                ]),
            ),
            (
                WitnessCondition::Group {
                    group: group.clone(),
                },
                StackValue::Array(vec![
                    StackValue::Integer(0x19),
                    StackValue::ByteString(group.clone()),
                ]),
            ),
            (
                WitnessCondition::CalledByEntry,
                StackValue::Array(vec![StackValue::Integer(0x20)]),
            ),
            (
                WitnessCondition::CalledByContract { hash },
                StackValue::Array(vec![
                    StackValue::Integer(0x28),
                    StackValue::ByteString(hash.to_bytes()),
                ]),
            ),
            (
                WitnessCondition::CalledByGroup {
                    group: group.clone(),
                },
                StackValue::Array(vec![
                    StackValue::Integer(0x29),
                    StackValue::ByteString(group),
                ]),
            ),
        ];
        for (condition, expected) in cases {
            assert_eq!(witness_condition_to_stack_value(&condition), expected);
            assert_eq!(condition.to_stack_value(), expected);
            let item = StackItem::try_from(expected.clone()).unwrap();
            assert_eq!(witness_condition_to_stack_item(&condition), item);
            assert_eq!(condition.to_stack_item(), item);
            for action in [WitnessRuleAction::Deny, WitnessRuleAction::Allow] {
                let rule = WitnessRule::new(action, condition.clone());
                assert_eq!(
                    witness_rule_to_stack_value(&rule),
                    StackValue::Array(vec![
                        StackValue::Integer(i64::from(action.to_byte())),
                        expected.clone(),
                    ])
                );
            }
        }
    }

    #[test]
    fn witness_condition_projection_preserves_nested_expression_order() {
        let condition = WitnessCondition::Or {
            conditions: vec![
                WitnessCondition::Not {
                    condition: Box::new(WitnessCondition::Boolean { value: true }),
                },
                WitnessCondition::And {
                    conditions: vec![
                        WitnessCondition::CalledByEntry,
                        WitnessCondition::Boolean { value: false },
                    ],
                },
            ],
        };
        let expected = StackValue::Array(vec![
            StackValue::Integer(0x03),
            StackValue::Array(vec![
                StackValue::Array(vec![
                    StackValue::Integer(0x01),
                    StackValue::Array(vec![StackValue::Integer(0x00), StackValue::Boolean(true)]),
                ]),
                StackValue::Array(vec![
                    StackValue::Integer(0x02),
                    StackValue::Array(vec![
                        StackValue::Array(vec![StackValue::Integer(0x20)]),
                        StackValue::Array(vec![
                            StackValue::Integer(0x00),
                            StackValue::Boolean(false),
                        ]),
                    ]),
                ]),
            ]),
        ]);
        assert_eq!(witness_condition_to_stack_value(&condition), expected);
        assert_eq!(
            witness_condition_to_stack_item(&condition),
            StackItem::try_from(expected).unwrap()
        );
    }
}
