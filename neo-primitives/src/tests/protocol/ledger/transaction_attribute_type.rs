use super::*;

#[test]
fn test_transaction_attribute_type_values() {
    assert_eq!(TransactionAttributeType::HighPriority.to_byte(), 0x01);
    assert_eq!(TransactionAttributeType::OracleResponse.to_byte(), 0x11);
    assert_eq!(TransactionAttributeType::NotValidBefore.to_byte(), 0x20);
    assert_eq!(TransactionAttributeType::Conflicts.to_byte(), 0x21);
    assert_eq!(TransactionAttributeType::NotaryAssisted.to_byte(), 0x22);
}

#[test]
fn test_transaction_attribute_type_from_byte() {
    assert_eq!(
        TransactionAttributeType::from_byte(0x01),
        Some(TransactionAttributeType::HighPriority)
    );
    assert_eq!(
        TransactionAttributeType::from_byte(0x11),
        Some(TransactionAttributeType::OracleResponse)
    );
    assert_eq!(
        TransactionAttributeType::from_byte(0x20),
        Some(TransactionAttributeType::NotValidBefore)
    );
    assert_eq!(
        TransactionAttributeType::from_byte(0x21),
        Some(TransactionAttributeType::Conflicts)
    );
    assert_eq!(
        TransactionAttributeType::from_byte(0x22),
        Some(TransactionAttributeType::NotaryAssisted)
    );
    assert_eq!(TransactionAttributeType::from_byte(0xFF), None);
}

#[test]
fn test_transaction_attribute_type_roundtrip() {
    for attr_type in [
        TransactionAttributeType::HighPriority,
        TransactionAttributeType::OracleResponse,
        TransactionAttributeType::NotValidBefore,
        TransactionAttributeType::Conflicts,
        TransactionAttributeType::NotaryAssisted,
    ] {
        let byte = attr_type.to_byte();
        let recovered = TransactionAttributeType::from_byte(byte);
        assert_eq!(recovered, Some(attr_type));
    }
}

#[test]
fn test_transaction_attribute_type_display() {
    assert_eq!(
        TransactionAttributeType::HighPriority.to_string(),
        "HighPriority"
    );
    assert_eq!(
        TransactionAttributeType::OracleResponse.to_string(),
        "OracleResponse"
    );
    assert_eq!(
        TransactionAttributeType::NotValidBefore.to_string(),
        "NotValidBefore"
    );
    assert_eq!(TransactionAttributeType::Conflicts.to_string(), "Conflicts");
    assert_eq!(
        TransactionAttributeType::NotaryAssisted.to_string(),
        "NotaryAssisted"
    );
}

#[test]
fn test_transaction_attribute_type_allows_multiple() {
    assert!(!TransactionAttributeType::HighPriority.allows_multiple());
    assert!(!TransactionAttributeType::OracleResponse.allows_multiple());
    assert!(!TransactionAttributeType::NotValidBefore.allows_multiple());
    assert!(TransactionAttributeType::Conflicts.allows_multiple());
    assert!(!TransactionAttributeType::NotaryAssisted.allows_multiple());
}

#[test]
fn test_transaction_attribute_type_serde() {
    let attr_type = TransactionAttributeType::OracleResponse;
    let serialized = serde_json::to_string(&attr_type).unwrap();
    assert_eq!(serialized, "17"); // 0x11 = 17

    let deserialized: TransactionAttributeType = serde_json::from_str(&serialized).unwrap();
    assert_eq!(deserialized, attr_type);
}
