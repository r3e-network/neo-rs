//! UInt160 and UInt256 Tests
//! Converted from C# Neo.UnitTests.IO.UT_UInt160.cs and UT_UInt256.cs

use neo_core::neo_io::{BinaryWriter, MemoryReader, Serializable};
use neo_primitives::{UINT160_SIZE, UINT256_SIZE, UInt160, UInt256};

// ============================================================================
// UInt160 Tests (from C# UT_UInt160.cs)
// ============================================================================

/// Test converted from C# TestFail
#[test]
fn test_uint160_fail_wrong_length() {
    // Should fail with wrong length (21 bytes instead of 20)
    let wrong_length_bytes = vec![0u8; UINT160_SIZE + 1];
    let result = UInt160::from_bytes(&wrong_length_bytes);
    assert!(result.is_err(), "Should fail with wrong length bytes");
}

/// Test converted from C# TestGernerator1
#[test]
fn test_uint160_generator1() {
    let uint160 = UInt160::new();
    assert_eq!(uint160, UInt160::zero());
}

/// Test converted from C# TestGernerator2
#[test]
fn test_uint160_generator2() {
    let bytes = [0u8; 20];
    let uint160 = UInt160::from_bytes(&bytes).unwrap();
    assert_eq!(uint160, UInt160::zero());
}

/// Test converted from C# TestGernerator3
#[test]
fn test_uint160_generator3() {
    let uint160 = UInt160::parse("0xff00000000000000000000000000000000000001").unwrap();
    assert_eq!(
        "0xff00000000000000000000000000000000000001",
        uint160.to_string()
    );

    let value = UInt160::parse("0x0102030405060708090a0b0c0d0e0f1011121314").unwrap();
    assert_eq!(
        "0x0102030405060708090a0b0c0d0e0f1011121314",
        value.to_string()
    );
}

/// Test converted from C# TestCompareTo
#[test]
fn test_uint160_compare_to() {
    let mut temp = [0u8; 20];
    temp[19] = 0x01;
    let result = UInt160::from_bytes(&temp).unwrap();

    assert_eq!(
        std::cmp::Ordering::Equal,
        UInt160::zero().cmp(&UInt160::zero())
    );
    assert_eq!(std::cmp::Ordering::Less, UInt160::zero().cmp(&result));
    assert_eq!(std::cmp::Ordering::Greater, result.cmp(&UInt160::zero()));
}

/// Test converted from C# TestEquals
#[test]
fn test_uint160_equals() {
    let mut temp = [0u8; 20];
    temp[19] = 0x01;
    let result = UInt160::from_bytes(&temp).unwrap();

    assert!(UInt160::zero() == UInt160::zero());
    assert!(UInt160::zero() != result);

    // String comparison
    let from_string = UInt160::parse("0x0000000000000000000000000000000000000000").unwrap();
    assert_eq!(UInt160::zero(), from_string);

    let from_string2 = UInt160::parse("0x0000000000000000000000000000000000000001").unwrap();
    assert_ne!(UInt160::zero(), from_string2);
}

/// Test converted from C# TestParse
#[test]
fn test_uint160_parse() {
    // Parse with 0x prefix
    let result = UInt160::parse("0x0000000000000000000000000000000000000000").unwrap();
    assert_eq!(UInt160::zero(), result);

    // Parse without 0x prefix
    let result1 = UInt160::parse("0000000000000000000000000000000000000000").unwrap();
    assert_eq!(UInt160::zero(), result1);

    // Invalid length should fail
    let result_short = UInt160::parse("000000000000000000000000000000000000000"); // 39 chars
    assert!(result_short.is_err(), "Should fail with short input");
}

/// Test converted from C# TestTryParse
#[test]
fn test_uint160_try_parse() {
    // Valid parse
    let temp = UInt160::parse("0x0000000000000000000000000000000000000000");
    assert!(temp.is_ok());
    assert_eq!(
        "0x0000000000000000000000000000000000000000",
        temp.unwrap().to_string()
    );

    let temp2 = UInt160::parse("0x1230000000000000000000000000000000000000");
    assert!(temp2.is_ok());
    assert_eq!(
        "0x1230000000000000000000000000000000000000",
        temp2.unwrap().to_string()
    );

    // Invalid characters should fail
    assert!(UInt160::parse("0xKK00000000000000000000000000000000000000").is_err());

    // Invalid length should fail
    assert!(UInt160::parse("000000000000000000000000000000000000000").is_err());
}

/// Test converted from C# TestOperatorLarger
#[test]
fn test_uint160_operator_larger() {
    assert!(UInt160::zero() <= UInt160::zero());

    let from_string = UInt160::parse("0x0000000000000000000000000000000000000000").unwrap();
    assert!(UInt160::zero() <= from_string);
}

/// Test converted from C# TestOperatorLargerAndEqual
#[test]
fn test_uint160_operator_larger_and_equal() {
    assert!(UInt160::zero() >= UInt160::zero());

    let from_string = UInt160::parse("0x0000000000000000000000000000000000000000").unwrap();
    assert!(UInt160::zero() >= from_string);
}

/// Test converted from C# TestOperatorSmaller
#[test]
fn test_uint160_operator_smaller() {
    assert!(UInt160::zero() >= UInt160::zero());

    let from_string = UInt160::parse("0x0000000000000000000000000000000000000000").unwrap();
    assert!(UInt160::zero() >= from_string);
}

/// Test converted from C# TestOperatorSmallerAndEqual
#[test]
fn test_uint160_operator_smaller_and_equal() {
    assert!(UInt160::zero() <= UInt160::zero());

    let from_string = UInt160::parse("0x0000000000000000000000000000000000000000").unwrap();
    assert!(UInt160::zero() <= from_string);
}

/// Test converted from C# TestSpanAndSerialize
#[test]
fn test_uint160_span_and_serialize() {
    // Create random-ish data
    let data: [u8; 20] = [
        0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0A, 0x0B, 0x0C, 0x0D, 0x0E, 0x0F,
        0x10, 0x11, 0x12, 0x13, 0x14,
    ];

    let value = UInt160::from_bytes(&data).unwrap();

    // Test serialization round-trip
    let mut writer = BinaryWriter::new();
    value.serialize(&mut writer).unwrap();
    let serialized = writer.to_bytes();

    let mut reader = MemoryReader::new(&serialized);
    let deserialized = UInt160::deserialize(&mut reader).unwrap();

    assert_eq!(value, deserialized);
}

// ============================================================================
// UInt256 Tests (from C# UT_UInt256.cs)
// ============================================================================

/// Test UInt256 fail with wrong length
#[test]
fn test_uint256_fail_wrong_length() {
    let wrong_length_bytes = vec![0u8; UINT256_SIZE + 1];
    let result = UInt256::from_bytes(&wrong_length_bytes);
    assert!(result.is_err(), "Should fail with wrong length bytes");
}

/// Test UInt256 creation
#[test]
fn test_uint256_generator() {
    let uint256 = UInt256::new();
    assert_eq!(uint256, UInt256::zero());

    let bytes = [0u8; 32];
    let uint256_from_bytes = UInt256::from_bytes(&bytes).unwrap();
    assert_eq!(uint256_from_bytes, UInt256::zero());
}

/// Test UInt256 parse
#[test]
fn test_uint256_parse() {
    let uint256 =
        UInt256::parse("0xff00000000000000000000000000000000000000000000000000000000000001")
            .unwrap();
    assert_eq!(
        "0xff00000000000000000000000000000000000000000000000000000000000001",
        uint256.to_string()
    );

    // Parse without prefix
    let result =
        UInt256::parse("0000000000000000000000000000000000000000000000000000000000000000").unwrap();
    assert_eq!(UInt256::zero(), result);
}

/// Test UInt256 compare
#[test]
fn test_uint256_compare_to() {
    let mut temp = [0u8; 32];
    temp[31] = 0x01;
    let result = UInt256::from_bytes(&temp).unwrap();

    assert_eq!(
        std::cmp::Ordering::Equal,
        UInt256::zero().cmp(&UInt256::zero())
    );
    assert_eq!(std::cmp::Ordering::Less, UInt256::zero().cmp(&result));
    assert_eq!(std::cmp::Ordering::Greater, result.cmp(&UInt256::zero()));
}

/// Test UInt256 equals
#[test]
fn test_uint256_equals() {
    let mut temp = [0u8; 32];
    temp[31] = 0x01;
    let result = UInt256::from_bytes(&temp).unwrap();

    assert!(UInt256::zero() == UInt256::zero());
    assert!(UInt256::zero() != result);
}

/// Test UInt256 operators
#[test]
fn test_uint256_operators() {
    assert!(UInt256::zero() <= UInt256::zero());
    assert!(UInt256::zero() >= UInt256::zero());
    assert!(UInt256::zero() >= UInt256::zero());
    assert!(UInt256::zero() <= UInt256::zero());
}

/// Test UInt256 serialization
#[test]
fn test_uint256_span_and_serialize() {
    let data: [u8; 32] = [
        0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0A, 0x0B, 0x0C, 0x0D, 0x0E, 0x0F,
        0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18, 0x19, 0x1A, 0x1B, 0x1C, 0x1D, 0x1E,
        0x1F, 0x20,
    ];

    let value = UInt256::from_bytes(&data).unwrap();

    let mut writer = BinaryWriter::new();
    value.serialize(&mut writer).unwrap();
    let serialized = writer.to_bytes();

    let mut reader = MemoryReader::new(&serialized);
    let deserialized = UInt256::deserialize(&mut reader).unwrap();

    assert_eq!(value, deserialized);
}

// ============================================================================
// Additional edge case tests
// ============================================================================

/// Test UInt160 from_script
#[test]
fn test_uint160_from_script() {
    let script = vec![0x01, 0x02, 0x03, 0x04, 0x05];
    let hash = UInt160::from_script(&script);

    // Should produce a non-zero hash
    assert_ne!(hash, UInt160::zero());

    // Same script should produce same hash
    let hash2 = UInt160::from_script(&script);
    assert_eq!(hash, hash2);

    // Different script should produce different hash
    let script2 = vec![0x06, 0x07, 0x08, 0x09, 0x0A];
    let hash3 = UInt160::from_script(&script2);
    assert_ne!(hash, hash3);
}

/// Test UInt160 to_array
#[test]
fn test_uint160_to_array() {
    let data: [u8; 20] = [
        0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0A, 0x0B, 0x0C, 0x0D, 0x0E, 0x0F,
        0x10, 0x11, 0x12, 0x13, 0x14,
    ];

    let uint160 = UInt160::from_bytes(&data).unwrap();
    let array = uint160.to_array();

    // Round-trip should preserve data
    let uint160_2 = UInt160::from_bytes(&array).unwrap();
    assert_eq!(uint160, uint160_2);
}

/// Test UInt256 to_array
#[test]
fn test_uint256_to_array() {
    let data: [u8; 32] = [
        0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0A, 0x0B, 0x0C, 0x0D, 0x0E, 0x0F,
        0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18, 0x19, 0x1A, 0x1B, 0x1C, 0x1D, 0x1E,
        0x1F, 0x20,
    ];

    let uint256 = UInt256::from_bytes(&data).unwrap();
    let array = uint256.to_array();

    let uint256_2 = UInt256::from_bytes(&array).unwrap();
    assert_eq!(uint256, uint256_2);
}

/// Test UInt160 hash implementation
#[test]
fn test_uint160_hash() {
    use std::collections::HashSet;

    let mut set = HashSet::new();
    let uint1 = UInt160::parse("0x0000000000000000000000000000000000000001").unwrap();
    let uint2 = UInt160::parse("0x0000000000000000000000000000000000000002").unwrap();
    let uint1_copy = UInt160::parse("0x0000000000000000000000000000000000000001").unwrap();

    set.insert(uint1);
    set.insert(uint2);
    set.insert(uint1_copy); // Should not increase count

    assert_eq!(set.len(), 2);
}

/// Test UInt256 hash implementation
#[test]
fn test_uint256_hash() {
    use std::collections::HashSet;

    let mut set = HashSet::new();
    let uint1 =
        UInt256::parse("0x0000000000000000000000000000000000000000000000000000000000000001")
            .unwrap();
    let uint2 =
        UInt256::parse("0x0000000000000000000000000000000000000000000000000000000000000002")
            .unwrap();
    let uint1_copy =
        UInt256::parse("0x0000000000000000000000000000000000000000000000000000000000000001")
            .unwrap();

    set.insert(uint1);
    set.insert(uint2);
    set.insert(uint1_copy);

    assert_eq!(set.len(), 2);
}
