use super::*;
use crate::persistence::DataCache;
use crate::protocol_settings::ProtocolSettings;
use crate::smart_contract::trigger_type::TriggerType;
use num_bigint::BigInt;
use std::sync::Arc;

fn create_stdlib() -> StdLib {
    StdLib::new()
}

fn make_engine() -> ApplicationEngine {
    ApplicationEngine::new(
        TriggerType::Application,
        None,
        Arc::new(DataCache::new(false)),
        None,
        ProtocolSettings::default(),
        400_000_000,
        None,
    )
    .expect("engine")
}

#[test]
fn test_memory_compare() {
    let stdlib = create_stdlib();

    // Equal arrays
    let result = stdlib
        .memory_compare(&[vec![1, 2, 3], vec![1, 2, 3]])
        .unwrap();
    assert_eq!(
        i32::from_le_bytes([result[0], result[1], result[2], result[3]]),
        0
    );

    // First less than second
    let result = stdlib
        .memory_compare(&[vec![1, 2, 3], vec![1, 2, 4]])
        .unwrap();
    assert_eq!(
        i32::from_le_bytes([result[0], result[1], result[2], result[3]]),
        -1
    );

    // First greater than second
    let result = stdlib
        .memory_compare(&[vec![1, 2, 4], vec![1, 2, 3]])
        .unwrap();
    assert_eq!(
        i32::from_le_bytes([result[0], result[1], result[2], result[3]]),
        1
    );

    // Different lengths
    let result = stdlib.memory_compare(&[vec![1, 2], vec![1, 2, 3]]).unwrap();
    assert_eq!(
        i32::from_le_bytes([result[0], result[1], result[2], result[3]]),
        -1
    );
}

#[test]
fn test_memory_search_basic() {
    let stdlib = create_stdlib();

    // Basic forward search
    let mem = vec![1, 2, 3, 4, 5, 6, 7, 8];
    let pattern = vec![4, 5, 6];
    let result = stdlib.memory_search(&[mem.clone(), pattern]).unwrap();
    assert_eq!(
        i32::from_le_bytes([result[0], result[1], result[2], result[3]]),
        3
    );

    // Pattern not found
    let pattern = vec![9, 10];
    let result = stdlib.memory_search(&[mem.clone(), pattern]).unwrap();
    assert_eq!(
        i32::from_le_bytes([result[0], result[1], result[2], result[3]]),
        -1
    );

    // Empty pattern
    let pattern = vec![];
    let result = stdlib.memory_search(&[mem.clone(), pattern]).unwrap();
    assert_eq!(
        i32::from_le_bytes([result[0], result[1], result[2], result[3]]),
        0
    );
}

#[test]
fn test_memory_search_with_start() {
    let stdlib = create_stdlib();

    let mem = vec![1, 2, 3, 4, 5, 4, 5, 6];
    let pattern = vec![4, 5];

    // Search from start=0
    let start = 0i32.to_le_bytes().to_vec();
    let result = stdlib
        .memory_search(&[mem.clone(), pattern.clone(), start])
        .unwrap();
    assert_eq!(
        i32::from_le_bytes([result[0], result[1], result[2], result[3]]),
        3
    );

    // Search from start=4 (should find second occurrence)
    let start = 4i32.to_le_bytes().to_vec();
    let result = stdlib
        .memory_search(&[mem.clone(), pattern.clone(), start])
        .unwrap();
    assert_eq!(
        i32::from_le_bytes([result[0], result[1], result[2], result[3]]),
        5
    );

    // Search from start=6 (should not find)
    let start = 6i32.to_le_bytes().to_vec();
    let result = stdlib
        .memory_search(&[mem.clone(), pattern.clone(), start])
        .unwrap();
    assert_eq!(
        i32::from_le_bytes([result[0], result[1], result[2], result[3]]),
        -1
    );
}

#[test]
fn test_memory_search_backward() {
    let stdlib = create_stdlib();

    let mem = vec![1, 2, 3, 4, 5, 4, 5, 6];
    let pattern = vec![4, 5];

    // Backward search from start=8 (search in [0..8])
    let start = 8i32.to_le_bytes().to_vec();
    let backward = vec![1u8]; // true
    let result = stdlib
        .memory_search(&[mem.clone(), pattern.clone(), start, backward])
        .unwrap();
    assert_eq!(
        i32::from_le_bytes([result[0], result[1], result[2], result[3]]),
        5
    );

    // Backward search from start=5 (search in [0..5], should find first occurrence)
    let start = 5i32.to_le_bytes().to_vec();
    let backward = vec![1u8];
    let result = stdlib
        .memory_search(&[mem.clone(), pattern.clone(), start, backward])
        .unwrap();
    assert_eq!(
        i32::from_le_bytes([result[0], result[1], result[2], result[3]]),
        3
    );

    // Backward search from start=3 (search in [0..3], should not find)
    let start = 3i32.to_le_bytes().to_vec();
    let backward = vec![1u8];
    let result = stdlib
        .memory_search(&[mem.clone(), pattern.clone(), start, backward])
        .unwrap();
    assert_eq!(
        i32::from_le_bytes([result[0], result[1], result[2], result[3]]),
        -1
    );
}

#[test]
fn test_string_split_basic() {
    let stdlib = create_stdlib();
    let engine = make_engine();

    let string = "hello,world,test".as_bytes().to_vec();
    let separator = ",".as_bytes().to_vec();
    let result = stdlib.string_split(&engine, &[string, separator]).unwrap();
    let item = BinarySerializer::deserialize(&result, engine.execution_limits(), None).unwrap();
    let parts = item.as_array().unwrap();
    assert_eq!(parts.len(), 3);
    assert_eq!(
        String::from_utf8(parts[0].as_bytes().unwrap()).unwrap(),
        "hello"
    );
    assert_eq!(
        String::from_utf8(parts[1].as_bytes().unwrap()).unwrap(),
        "world"
    );
    assert_eq!(
        String::from_utf8(parts[2].as_bytes().unwrap()).unwrap(),
        "test"
    );
}

#[test]
fn test_string_split_with_empty_entries() {
    let stdlib = create_stdlib();
    let engine = make_engine();

    let string = "hello,,world,,test".as_bytes().to_vec();
    let separator = ",".as_bytes().to_vec();

    // Without removeEmptyEntries (default: false)
    let result = stdlib
        .string_split(&engine, &[string.clone(), separator.clone()])
        .unwrap();
    let item = BinarySerializer::deserialize(&result, engine.execution_limits(), None).unwrap();
    let parts = item.as_array().unwrap();
    assert_eq!(parts.len(), 5); // hello, "", world, "", test

    // With removeEmptyEntries = true
    let remove_empty = vec![1u8];
    let result = stdlib
        .string_split(&engine, &[string.clone(), separator.clone(), remove_empty])
        .unwrap();
    let item = BinarySerializer::deserialize(&result, engine.execution_limits(), None).unwrap();
    let parts = item.as_array().unwrap();
    assert_eq!(parts.len(), 3); // hello, world, test
}

#[test]
fn test_str_len_basic() {
    let stdlib = create_stdlib();

    // ASCII string
    let string = "hello".as_bytes().to_vec();
    let result = stdlib.str_len(&[string]).unwrap();
    assert_eq!(
        i32::from_le_bytes([result[0], result[1], result[2], result[3]]),
        5
    );

    // Empty string
    let string = "".as_bytes().to_vec();
    let result = stdlib.str_len(&[string]).unwrap();
    assert_eq!(
        i32::from_le_bytes([result[0], result[1], result[2], result[3]]),
        0
    );
}

#[test]
fn test_str_len_unicode() {
    let stdlib = create_stdlib();

    // Emoji (should count as 1 grapheme cluster)
    let string = "🦆".as_bytes().to_vec();
    let result = stdlib.str_len(&[string]).unwrap();
    assert_eq!(
        i32::from_le_bytes([result[0], result[1], result[2], result[3]]),
        1
    );

    // Combining character (should count as 1 grapheme cluster)
    let string = "ã".as_bytes().to_vec(); // a + combining tilde
    let result = stdlib.str_len(&[string]).unwrap();
    assert_eq!(
        i32::from_le_bytes([result[0], result[1], result[2], result[3]]),
        1
    );

    // Mixed ASCII and emoji
    let string = "hello🦆world".as_bytes().to_vec();
    let result = stdlib.str_len(&[string]).unwrap();
    assert_eq!(
        i32::from_le_bytes([result[0], result[1], result[2], result[3]]),
        11
    );

    // Multiple emojis
    let string = "🦆🦆🦆".as_bytes().to_vec();
    let result = stdlib.str_len(&[string]).unwrap();
    assert_eq!(
        i32::from_le_bytes([result[0], result[1], result[2], result[3]]),
        3
    );
}

#[test]
fn test_atoi_itoa() {
    let stdlib = create_stdlib();

    // Test itoa
    let number = 12345i64.to_le_bytes().to_vec();
    let result = stdlib.itoa(&[number]).unwrap();
    let string = String::from_utf8(result).unwrap();
    assert_eq!(string, "12345");

    // Test atoi
    let string = "12345".as_bytes().to_vec();
    let result = stdlib.atoi(&[string]).unwrap();
    let number = BigInt::from_signed_bytes_le(&result);
    assert_eq!(number, BigInt::from(12345));

    // Test negative number
    let number = (-12345i64).to_le_bytes().to_vec();
    let result = stdlib.itoa(&[number]).unwrap();
    let string = String::from_utf8(result).unwrap();
    assert_eq!(string, "-12345");

    // Hex negative formatting/parsing parity with C#
    let number = (-1i64).to_le_bytes().to_vec();
    let base = 16i64.to_le_bytes().to_vec();
    let result = stdlib.itoa(&[number, base.clone()]).unwrap();
    let string = String::from_utf8(result).unwrap();
    assert_eq!(string, "f");

    let string = "ff".as_bytes().to_vec();
    let result = stdlib.atoi(&[string, base.clone()]).unwrap();
    let number = BigInt::from_signed_bytes_le(&result);
    assert_eq!(number, BigInt::from(-1));

    // Positive values with sign bit set should include a leading 0 nibble.
    let number = 255i64.to_le_bytes().to_vec();
    let result = stdlib.itoa(&[number, base]).unwrap();
    let string = String::from_utf8(result).unwrap();
    assert_eq!(string, "0ff");
}

#[test]
fn test_base64_encode_decode() {
    let stdlib = create_stdlib();

    let data = b"Hello, World!".to_vec();

    // Encode
    let encoded = stdlib.base64_encode(std::slice::from_ref(&data)).unwrap();
    let encoded_str = String::from_utf8(encoded.clone()).unwrap();
    assert_eq!(encoded_str, "SGVsbG8sIFdvcmxkIQ==");

    // Decode
    let decoded = stdlib.base64_decode(&[encoded]).unwrap();
    assert_eq!(decoded, data);
}

#[test]
fn test_json_serialize_deserialize() {
    let stdlib = create_stdlib();
    let engine = make_engine();

    let data = "test string".as_bytes().to_vec();

    // Serialize
    let serialized = stdlib
        .json_serialize(&engine, std::slice::from_ref(&data))
        .unwrap();
    let json_str = String::from_utf8(serialized.clone()).unwrap();
    assert!(json_str.contains("test string"));

    // Deserialize
    let deserialized = stdlib.json_deserialize(&engine, &[serialized]).unwrap();
    let decoded = BinarySerializer::deserialize(&deserialized, engine.execution_limits(), None)
        .expect("deserialize");
    assert_eq!(decoded.as_bytes().unwrap(), data);
}
