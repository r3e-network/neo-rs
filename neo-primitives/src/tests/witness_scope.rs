use super::*;

#[test]
fn test_witness_scope_values() {
    assert_eq!(WitnessScope::NONE.to_byte(), 0x00);
    assert_eq!(WitnessScope::CALLED_BY_ENTRY.to_byte(), 0x01);
    assert_eq!(WitnessScope::CUSTOM_CONTRACTS.to_byte(), 0x10);
    assert_eq!(WitnessScope::CUSTOM_GROUPS.to_byte(), 0x20);
    assert_eq!(WitnessScope::WITNESS_RULES.to_byte(), 0x40);
    assert_eq!(WitnessScope::GLOBAL.to_byte(), 0x80);
}

#[test]
fn test_witness_scope_has_flag() {
    let scope = WitnessScope::CALLED_BY_ENTRY;
    assert!(scope.has_flag(WitnessScope::CALLED_BY_ENTRY));
    assert!(!scope.has_flag(WitnessScope::CUSTOM_CONTRACTS));

    let combined = WitnessScope::CALLED_BY_ENTRY.combine(WitnessScope::CUSTOM_CONTRACTS);
    assert!(combined.has_flag(WitnessScope::CALLED_BY_ENTRY));
    assert!(combined.has_flag(WitnessScope::CUSTOM_CONTRACTS));
}

#[test]
fn test_witness_scope_from_byte() {
    assert_eq!(WitnessScope::from_byte(0x00), Some(WitnessScope::NONE));
    assert_eq!(
        WitnessScope::from_byte(0x01),
        Some(WitnessScope::CALLED_BY_ENTRY)
    );
    assert_eq!(WitnessScope::from_byte(0x80), Some(WitnessScope::GLOBAL));
    assert_eq!(WitnessScope::from_byte(0xFF), Option::None);
}

#[test]
fn protocol_enum_guard_rejects_invalid_witness_scope_bytes() {
    let combined = WitnessScope::CALLED_BY_ENTRY | WitnessScope::CUSTOM_CONTRACTS;
    assert_eq!(WitnessScope::from_byte(0x11), Some(combined));
    assert_eq!(WitnessScope::from_bits(0x11), Some(combined));
    assert_eq!(WitnessScope::try_from(0x11), Ok(combined));
    assert_eq!(WitnessScope::from_byte(0x02), None);
    assert_eq!(WitnessScope::from_byte(0x81), None);
    assert_eq!(WitnessScope::from_bits(0x81), None);
    assert_eq!(
        WitnessScope::try_from(0x81),
        Err(InvalidWitnessScopeError(0x81))
    );
}

#[test]
fn test_witness_scope_is_valid() {
    assert!(WitnessScope::NONE.is_valid());
    assert!(WitnessScope::CALLED_BY_ENTRY.is_valid());
    assert!(WitnessScope::GLOBAL.is_valid());

    // Combined flags (non-global) should be valid
    let combined = WitnessScope::CALLED_BY_ENTRY | WitnessScope::CUSTOM_CONTRACTS;
    assert!(combined.is_valid());

    let invalid_global = WitnessScope::GLOBAL | WitnessScope::CALLED_BY_ENTRY;
    assert!(!invalid_global.is_valid());
}

#[test]
fn test_witness_scope_display() {
    assert_eq!(format!("{}", WitnessScope::NONE), "None");
    assert_eq!(
        format!("{}", WitnessScope::CALLED_BY_ENTRY),
        "CalledByEntry"
    );
    assert_eq!(format!("{}", WitnessScope::GLOBAL), "Global");
    assert_eq!(
        format!(
            "{}",
            WitnessScope::CALLED_BY_ENTRY | WitnessScope::CUSTOM_CONTRACTS
        ),
        "CalledByEntry, CustomContracts"
    );
}

#[test]
fn test_witness_scope_from_str() {
    assert_eq!(WitnessScope::from_str("None").unwrap(), WitnessScope::NONE);
    assert_eq!(
        WitnessScope::from_str("CalledByEntry").unwrap(),
        WitnessScope::CALLED_BY_ENTRY
    );
    assert_eq!(
        WitnessScope::from_str("Global").unwrap(),
        WitnessScope::GLOBAL
    );
    assert_eq!(
        WitnessScope::from_str("CalledByEntry, CustomContracts").unwrap(),
        WitnessScope::CALLED_BY_ENTRY | WitnessScope::CUSTOM_CONTRACTS
    );
    assert!(WitnessScope::from_str("Invalid").is_err());
    assert!(WitnessScope::from_str("calledbyentry").is_err());
    assert!(WitnessScope::from_str("Global | CalledByEntry").is_err());
    assert!(WitnessScope::from_str("").is_err());
}

#[test]
fn test_witness_scope_conversions() {
    let scope = WitnessScope::CALLED_BY_ENTRY;
    let byte_value: u8 = scope.into();
    assert_eq!(byte_value, 0x01);
    // Use TryFrom instead of From for safe conversion (returns error for invalid values)
    let converted_scope = WitnessScope::try_from(byte_value).unwrap();
    assert_eq!(converted_scope, scope);
}

#[test]
fn test_witness_scope_default() {
    assert_eq!(WitnessScope::default(), WitnessScope::NONE);
}

#[test]
fn test_witness_scope_bitwise_ops() {
    let mut scope = WitnessScope::CALLED_BY_ENTRY;
    scope |= WitnessScope::CUSTOM_CONTRACTS;
    assert!(scope.has_flag(WitnessScope::CALLED_BY_ENTRY));
    assert!(scope.has_flag(WitnessScope::CUSTOM_CONTRACTS));

    let masked = scope & WitnessScope::CALLED_BY_ENTRY;
    assert_eq!(masked, WitnessScope::CALLED_BY_ENTRY);
}

#[test]
fn serde_roundtrip_uses_validated_numeric_scope() {
    let scope = WitnessScope::CALLED_BY_ENTRY | WitnessScope::CUSTOM_CONTRACTS;

    assert_eq!(serde_json::to_string(&scope).unwrap(), "17");
    assert_eq!(serde_json::from_str::<WitnessScope>("17").unwrap(), scope);
    assert!(serde_json::from_str::<WitnessScope>("2").is_err());
    assert!(serde_json::from_str::<WitnessScope>("129").is_err());
}
