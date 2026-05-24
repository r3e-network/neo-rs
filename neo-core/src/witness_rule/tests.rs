use super::helpers::{encode_hex, parse_group_bytes};
use super::*;
use crate::neo_config::ADDRESS_SIZE;
use crate::vm_runtime::StackItem;
use crate::UInt160;

#[test]
fn test_witness_rule_action_values() {
    assert_eq!(WitnessRuleAction::Deny as u8, 0);
    assert_eq!(WitnessRuleAction::Allow as u8, 1);
}

#[test]
fn protocol_enum_guard_rejects_unknown_witness_rule_action_bytes() {
    assert_eq!(WitnessRuleAction::from_byte(2), None);
    assert_eq!(WitnessRuleAction::from_byte(255), None);
    assert!(serde_json::from_str::<WitnessRuleAction>("2").is_err());
    assert!(serde_json::from_str::<WitnessRuleAction>("255").is_err());
}

#[test]
fn test_witness_condition_type_values() {
    assert_eq!(WitnessConditionType::Boolean as u8, 0x00);
    assert_eq!(WitnessConditionType::Not as u8, 0x01);
    assert_eq!(WitnessConditionType::And as u8, 0x02);
    assert_eq!(WitnessConditionType::Or as u8, 0x03);
    assert_eq!(WitnessConditionType::ScriptHash as u8, 0x18);
    assert_eq!(WitnessConditionType::Group as u8, 0x19);
    assert_eq!(WitnessConditionType::CalledByEntry as u8, 0x20);
    assert_eq!(WitnessConditionType::CalledByContract as u8, 0x28);
    assert_eq!(WitnessConditionType::CalledByGroup as u8, 0x29);
}

#[test]
fn protocol_enum_guard_rejects_unknown_witness_condition_type_bytes() {
    assert_eq!(WitnessConditionType::from_byte(0x04), None);
    assert_eq!(WitnessConditionType::from_byte(0x1a), None);
    assert_eq!(WitnessConditionType::from_byte(0xff), None);
    assert!(serde_json::from_str::<WitnessConditionType>("4").is_err());
    assert!(serde_json::from_str::<WitnessConditionType>("26").is_err());
    assert!(serde_json::from_str::<WitnessConditionType>("255").is_err());
}

#[test]
fn test_witness_condition_validation() {
    let boolean_condition = WitnessCondition::Boolean { value: true };
    assert!(boolean_condition.is_valid(WitnessCondition::MAX_NESTING_DEPTH));

    let called_by_entry = WitnessCondition::CalledByEntry;
    assert!(called_by_entry.is_valid(WitnessCondition::MAX_NESTING_DEPTH));

    let empty_and = WitnessCondition::And { conditions: vec![] };
    assert!(!empty_and.is_valid(WitnessCondition::MAX_NESTING_DEPTH));

    let valid_and = WitnessCondition::And {
        conditions: vec![
            WitnessCondition::Boolean { value: true },
            WitnessCondition::CalledByEntry,
        ],
    };
    assert!(valid_and.is_valid(WitnessCondition::MAX_NESTING_DEPTH));
}

#[test]
fn test_witness_rule_creation() {
    let condition = WitnessCondition::Boolean { value: true };
    let rule = WitnessRule::new(WitnessRuleAction::Allow, condition);
    assert_eq!(rule.action, WitnessRuleAction::Allow);
    assert!(rule.is_valid());
}

#[test]
fn boolean_condition_json_matches_csharp_structure() {
    let condition = WitnessCondition::Boolean { value: true };
    let json = condition.to_json();
    assert_eq!(json["type"], "Boolean");
    assert_eq!(json["expression"], true);
    assert_eq!(
        WitnessCondition::from_json(&json).unwrap(),
        WitnessCondition::Boolean { value: true }
    );
}

#[test]
fn group_condition_json_roundtrip_without_prefix() {
    let bytes =
        parse_group_bytes("03b209fd4f53a7170ea4444e0cb0a6bb6a53c2bd016926989cf85f9b0fba17a70c")
            .unwrap();
    let condition = WitnessCondition::Group {
        group: bytes.clone(),
    };
    let json = condition.to_json();
    assert_eq!(json["type"], "Group");
    assert_eq!(json["group"], encode_hex(&bytes));
    let decoded = WitnessCondition::from_json(&json).unwrap();
    assert_eq!(decoded, condition);
}

#[test]
fn witness_rule_projects_to_neo_vm_rs_stack_value() {
    let hash = UInt160::from_bytes(&[0x11; ADDRESS_SIZE]).unwrap();
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
        neo_vm_rs::StackValue::Array(vec![
            neo_vm_rs::StackValue::Integer(WitnessRuleAction::Allow.to_byte().into()),
            neo_vm_rs::StackValue::Array(vec![
                neo_vm_rs::StackValue::Integer(WitnessConditionType::And.to_byte().into()),
                neo_vm_rs::StackValue::Array(vec![
                    neo_vm_rs::StackValue::Array(vec![
                        neo_vm_rs::StackValue::Integer(
                            WitnessConditionType::Boolean.to_byte().into()
                        ),
                        neo_vm_rs::StackValue::Boolean(true),
                    ]),
                    neo_vm_rs::StackValue::Array(vec![
                        neo_vm_rs::StackValue::Integer(
                            WitnessConditionType::ScriptHash.to_byte().into()
                        ),
                        neo_vm_rs::StackValue::ByteString(hash.to_bytes()),
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
