use super::*;

#[test]
fn test_inventory_type_values() {
    assert_eq!(InventoryType::Transaction.to_byte(), 0x2b);
    assert_eq!(InventoryType::Block.to_byte(), 0x2c);
    assert_eq!(InventoryType::Extensible.to_byte(), 0x2e);
}

#[test]
fn test_inventory_type_from_byte() {
    assert_eq!(
        InventoryType::from_byte(0x2b),
        Some(InventoryType::Transaction)
    );
    assert_eq!(InventoryType::from_byte(0x2c), Some(InventoryType::Block));
    assert_eq!(InventoryType::from_byte(0x2d), None);
    assert_eq!(
        InventoryType::from_byte(0x2e),
        Some(InventoryType::Extensible)
    );
    assert_eq!(InventoryType::from_byte(0x00), None);
}

#[test]
fn protocol_enum_guard_rejects_unknown_inventory_bytes() {
    assert_eq!(InventoryType::from_byte(0x2a), None);
    assert_eq!(InventoryType::from_byte(0x2f), None);
    assert_eq!(InventoryType::from_byte(0xff), None);
}

#[test]
fn test_inventory_type_display() {
    assert_eq!(InventoryType::Transaction.to_string(), "TX");
    assert_eq!(InventoryType::Block.to_string(), "Block");
}

#[test]
fn inventory_type_serde_keeps_derived_enum_shape() {
    let serialized = serde_json::to_string(&InventoryType::Transaction).unwrap();
    assert_eq!(serialized, "\"Transaction\"");

    let deserialized: InventoryType = serde_json::from_str("\"Block\"").unwrap();
    assert_eq!(deserialized, InventoryType::Block);
}
