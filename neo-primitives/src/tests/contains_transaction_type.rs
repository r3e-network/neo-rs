use super::ContainsTransactionType;

#[test]
fn contains_transaction_type_matches_neo_values() {
    assert_eq!(ContainsTransactionType::NotExist.to_byte(), 0);
    assert_eq!(ContainsTransactionType::ExistsInPool.to_byte(), 1);
    assert_eq!(ContainsTransactionType::ExistsInLedger.to_byte(), 2);

    assert_eq!(
        ContainsTransactionType::from_byte(0),
        Some(ContainsTransactionType::NotExist)
    );
    assert_eq!(
        ContainsTransactionType::from_byte(1),
        Some(ContainsTransactionType::ExistsInPool)
    );
    assert_eq!(
        ContainsTransactionType::from_byte(2),
        Some(ContainsTransactionType::ExistsInLedger)
    );
    assert_eq!(ContainsTransactionType::from_byte(3), None);
}

#[test]
fn contains_transaction_type_exists_only_for_present_transactions() {
    assert!(!ContainsTransactionType::NotExist.exists());
    assert!(ContainsTransactionType::ExistsInPool.exists());
    assert!(ContainsTransactionType::ExistsInLedger.exists());
}

#[test]
fn contains_transaction_type_serde_uses_wire_byte() {
    let serialized = serde_json::to_string(&ContainsTransactionType::ExistsInPool).unwrap();
    assert_eq!(serialized, "1");

    let deserialized: ContainsTransactionType = serde_json::from_str("1").unwrap();
    assert_eq!(deserialized, ContainsTransactionType::ExistsInPool);

    assert!(serde_json::from_str::<ContainsTransactionType>("3").is_err());
}
