use neo_primitives::WitnessConditionType;

#[test]
fn witness_condition_type_logical_operations() {
    assert_eq!(WitnessConditionType::Boolean.to_byte(), 0x00);
    assert_eq!(WitnessConditionType::Not.to_byte(), 0x01);
    assert_eq!(WitnessConditionType::And.to_byte(), 0x02);
    assert_eq!(WitnessConditionType::Or.to_byte(), 0x03);
}

#[test]
fn witness_condition_type_hash_based_conditions() {
    assert_eq!(WitnessConditionType::ScriptHash.to_byte(), 0x18);
    assert_eq!(WitnessConditionType::Group.to_byte(), 0x19);
}

#[test]
fn witness_condition_type_call_conditions() {
    assert_eq!(WitnessConditionType::CalledByEntry.to_byte(), 0x20);
    assert_eq!(WitnessConditionType::CalledByContract.to_byte(), 0x28);
    assert_eq!(WitnessConditionType::CalledByGroup.to_byte(), 0x29);
}

#[test]
fn witness_condition_type_from_byte_valid_values() {
    assert_eq!(
        WitnessConditionType::from_byte(0x00),
        Some(WitnessConditionType::Boolean)
    );
    assert_eq!(
        WitnessConditionType::from_byte(0x01),
        Some(WitnessConditionType::Not)
    );
    assert_eq!(
        WitnessConditionType::from_byte(0x02),
        Some(WitnessConditionType::And)
    );
    assert_eq!(
        WitnessConditionType::from_byte(0x03),
        Some(WitnessConditionType::Or)
    );
}

#[test]
fn witness_condition_type_from_byte_invalid_values() {
    assert_eq!(WitnessConditionType::from_byte(0x04), None);
    assert_eq!(WitnessConditionType::from_byte(0x05), None);
    assert_eq!(WitnessConditionType::from_byte(0x10), None);
    assert_eq!(WitnessConditionType::from_byte(0x1a), None);
    assert_eq!(WitnessConditionType::from_byte(0x21), None);
    assert_eq!(WitnessConditionType::from_byte(0xff), None);
}

#[test]
fn witness_condition_type_roundtrip_all_variants() {
    for condition_type in [
        WitnessConditionType::Boolean,
        WitnessConditionType::Not,
        WitnessConditionType::And,
        WitnessConditionType::Or,
        WitnessConditionType::ScriptHash,
        WitnessConditionType::Group,
        WitnessConditionType::CalledByEntry,
        WitnessConditionType::CalledByContract,
        WitnessConditionType::CalledByGroup,
    ] {
        let byte = condition_type.to_byte();
        let parsed = WitnessConditionType::from_byte(byte);
        assert_eq!(parsed, Some(condition_type));
    }
}

#[test]
fn witness_condition_type_display() {
    assert_eq!(WitnessConditionType::Boolean.to_string(), "Boolean");
    assert_eq!(WitnessConditionType::Not.to_string(), "Not");
    assert_eq!(WitnessConditionType::And.to_string(), "And");
    assert_eq!(WitnessConditionType::Or.to_string(), "Or");
    assert_eq!(WitnessConditionType::ScriptHash.to_string(), "ScriptHash");
    assert_eq!(WitnessConditionType::Group.to_string(), "Group");
    assert_eq!(
        WitnessConditionType::CalledByEntry.to_string(),
        "CalledByEntry"
    );
    assert_eq!(
        WitnessConditionType::CalledByContract.to_string(),
        "CalledByContract"
    );
    assert_eq!(
        WitnessConditionType::CalledByGroup.to_string(),
        "CalledByGroup"
    );
}

#[test]
fn witness_condition_type_serde_serialization() {
    let condition = WitnessConditionType::ScriptHash;
    let serialized = serde_json::to_string(&condition).unwrap();
    assert_eq!(serialized, "24"); // 0x18 = 24
}

#[test]
fn witness_condition_type_serde_deserialization() {
    let deserialized: WitnessConditionType = serde_json::from_str("24").unwrap();
    assert_eq!(deserialized, WitnessConditionType::ScriptHash);
}

#[test]
fn witness_condition_type_serde_roundtrip() {
    for condition_type in [
        WitnessConditionType::Boolean,
        WitnessConditionType::Not,
        WitnessConditionType::And,
        WitnessConditionType::Or,
        WitnessConditionType::ScriptHash,
        WitnessConditionType::Group,
        WitnessConditionType::CalledByEntry,
        WitnessConditionType::CalledByContract,
        WitnessConditionType::CalledByGroup,
    ] {
        let serialized = serde_json::to_string(&condition_type).unwrap();
        let deserialized: WitnessConditionType = serde_json::from_str(&serialized).unwrap();
        assert_eq!(deserialized, condition_type);
    }
}

#[test]
fn witness_condition_type_serde_rejects_invalid() {
    assert!(serde_json::from_str::<WitnessConditionType>("4").is_err());
    assert!(serde_json::from_str::<WitnessConditionType>("10").is_err());
    assert!(serde_json::from_str::<WitnessConditionType>("255").is_err());
}

#[test]
fn witness_condition_type_clone() {
    let condition = WitnessConditionType::CalledByContract;
    let cloned = condition;
    assert_eq!(condition, cloned);
}

#[test]
fn witness_condition_type_copy() {
    let condition = WitnessConditionType::Group;
    let copied = condition;
    assert_eq!(condition, copied);
}

#[test]
fn witness_condition_type_debug_format() {
    let condition = WitnessConditionType::And;
    let debug = format!("{condition:?}");
    assert_eq!(debug, "And");
}

#[test]
fn witness_condition_type_equality() {
    assert_eq!(WitnessConditionType::Boolean, WitnessConditionType::Boolean);
    assert_ne!(WitnessConditionType::Boolean, WitnessConditionType::Not);
}

#[test]
fn witness_condition_type_hash() {
    use std::collections::HashSet;

    let mut set = HashSet::new();
    set.insert(WitnessConditionType::Boolean);
    set.insert(WitnessConditionType::Not);
    set.insert(WitnessConditionType::Boolean); // Duplicate

    assert_eq!(set.len(), 2);
    assert!(set.contains(&WitnessConditionType::Boolean));
    assert!(set.contains(&WitnessConditionType::Not));
}
