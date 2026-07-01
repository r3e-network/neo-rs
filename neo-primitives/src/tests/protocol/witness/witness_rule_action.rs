use super::*;

#[test]
fn test_witness_rule_action_values() {
    assert_eq!(WitnessRuleAction::Deny.to_byte(), 0);
    assert_eq!(WitnessRuleAction::Allow.to_byte(), 1);
}

#[test]
fn test_witness_rule_action_from_byte() {
    assert_eq!(
        WitnessRuleAction::from_byte(0),
        Some(WitnessRuleAction::Deny)
    );
    assert_eq!(
        WitnessRuleAction::from_byte(1),
        Some(WitnessRuleAction::Allow)
    );
    assert_eq!(WitnessRuleAction::from_byte(2), None);
}

#[test]
fn test_witness_rule_action_all_values() {
    assert_eq!(WitnessRuleAction::COUNT, 2);
    assert_eq!(
        WitnessRuleAction::all(),
        [WitnessRuleAction::Deny, WitnessRuleAction::Allow]
    );
    assert_eq!(WitnessRuleAction::ALL, WitnessRuleAction::all());
}

#[test]
fn protocol_enum_guard_rejects_unknown_witness_rule_action_bytes() {
    assert_eq!(WitnessRuleAction::from_byte(2), None);
    assert_eq!(WitnessRuleAction::from_byte(255), None);
    assert!(serde_json::from_str::<WitnessRuleAction>("2").is_err());
    assert!(serde_json::from_str::<WitnessRuleAction>("255").is_err());
}

#[test]
fn test_witness_rule_action_from_str() {
    assert_eq!(
        "Deny".parse::<WitnessRuleAction>().unwrap(),
        WitnessRuleAction::Deny
    );
    assert_eq!(
        "Allow".parse::<WitnessRuleAction>().unwrap(),
        WitnessRuleAction::Allow
    );
    assert!("allow".parse::<WitnessRuleAction>().is_err());
    assert!("Invalid".parse::<WitnessRuleAction>().is_err());
}

#[test]
fn test_witness_rule_action_display() {
    assert_eq!(WitnessRuleAction::Deny.to_string(), "Deny");
    assert_eq!(WitnessRuleAction::Allow.to_string(), "Allow");
}

#[test]
fn test_witness_rule_action_default() {
    assert_eq!(WitnessRuleAction::default(), WitnessRuleAction::Deny);
}
