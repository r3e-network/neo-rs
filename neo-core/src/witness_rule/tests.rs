use super::*;

#[test]
fn test_witness_rule_action_values() {
    assert_eq!(WitnessRuleAction::Deny as u8, 0);
    assert_eq!(WitnessRuleAction::Allow as u8, 1);
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
fn test_witness_condition_validation() {
    // Boolean condition should be valid
    let boolean_condition = WitnessCondition::Boolean { value: true };
    assert!(boolean_condition.is_valid(WitnessCondition::MAX_NESTING_DEPTH));
    // CalledByEntry condition should be valid
    let called_by_entry = WitnessCondition::CalledByEntry;
    assert!(called_by_entry.is_valid(WitnessCondition::MAX_NESTING_DEPTH));
    // Empty And condition should be invalid
    let empty_and = WitnessCondition::And { conditions: vec![] };
    assert!(!empty_and.is_valid(WitnessCondition::MAX_NESTING_DEPTH));
    // Valid And condition
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
    assert_eq!(json["group"], hex_encode(&bytes));
    let decoded = WitnessCondition::from_json(&json).unwrap();
    assert_eq!(decoded, condition);
}
