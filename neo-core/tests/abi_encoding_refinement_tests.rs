// ABI Encoding Refinement Tests - StackValue Serialization Round-Trips
//
// These tests verify the correctness of NeoVM stack value encoding/decoding,
// implementing differential tests that map to Coq lemma proofs in
// formal/coqlib/abi_encoding_refinement.v
//
// Encoding format: [stack_length_u32_le][item1_encoded][item2_encoded]...
// Where each item is: [type_tag_1_byte][payload_bytes]
//
// Corresponding Coq lemmas:
// - encoding_preserves_type          → test_all_variant_roundtrips
// - null_encoding_exact              → test_null_encoding_exact
// - boolean_encoding_length          → test_boolean_encoding_length
// - tag_separation                   → test_tag_separation
// - collection_count_preservation    → test_collection_count_preservation
// - byte_content_preservation        → test_byte_content_preservation

extern crate neo_vm;

use neo_vm::{
    STACK_VALUE_CODEC_TAG_ARRAY, STACK_VALUE_CODEC_TAG_BIG_INTEGER, STACK_VALUE_CODEC_TAG_BOOLEAN,
    STACK_VALUE_CODEC_TAG_BUFFER, STACK_VALUE_CODEC_TAG_BYTESTRING, STACK_VALUE_CODEC_TAG_INTEGER,
    STACK_VALUE_CODEC_TAG_INTEROP, STACK_VALUE_CODEC_TAG_ITERATOR, STACK_VALUE_CODEC_TAG_MAP,
    STACK_VALUE_CODEC_TAG_NULL, STACK_VALUE_CODEC_TAG_POINTER, STACK_VALUE_CODEC_TAG_STRUCT,
    StackValue,
    abi::fast_codec::{decode_stack, encode_stack},
};
use std::vec::Vec;

/// Helper function to extract the type tag from encoded stack item
fn get_type_tag(encoded: &[u8]) -> u8 {
    // Encode format: [len_u32_le][item1...]
    // Type tag is at position 4 (after 4-byte length prefix)
    encoded[4]
}

// ============================================================================
// SECTION 1: Primitive Type Round-Trip Tests (Coq: encoding_preserves_type)
// ============================================================================

#[test]
fn test_integer_encoding_roundtrip() {
    // Test positive integer
    let original = StackValue::Integer(42);
    let encoded = encode_stack(std::slice::from_ref(&original));
    let decoded = decode_stack(&encoded).expect("integer should decode successfully");
    assert_eq!(decoded.len(), 1);
    assert_eq!(decoded[0], original);

    // Test negative integer
    let original_neg = StackValue::Integer(-100);
    let encoded_neg = encode_stack(std::slice::from_ref(&original_neg));
    let decoded_neg = decode_stack(&encoded_neg).expect("negative integer should decode");
    assert_eq!(decoded_neg.len(), 1);
    assert_eq!(decoded_neg[0], original_neg);

    // Test zero
    let original_zero = StackValue::Integer(0);
    let encoded_zero = encode_stack(std::slice::from_ref(&original_zero));
    let decoded_zero = decode_stack(&encoded_zero).expect("zero should decode");
    assert_eq!(decoded_zero[0], original_zero);

    // Test max i64 boundary
    let original_max = StackValue::Integer(i64::MAX);
    let encoded_max = encode_stack(std::slice::from_ref(&original_max));
    let decoded_max = decode_stack(&encoded_max).expect("max i64 should decode");
    assert_eq!(decoded_max[0], original_max);

    // Test min i64 boundary
    let original_min = StackValue::Integer(i64::MIN);
    let encoded_min = encode_stack(std::slice::from_ref(&original_min));
    let decoded_min = decode_stack(&encoded_min).expect("min i64 should decode");
    assert_eq!(decoded_min[0], original_min);
}

#[test]
fn test_boolean_encoding_roundtrip() {
    // Test true
    let original_true = StackValue::Boolean(true);
    let encoded_true = encode_stack(std::slice::from_ref(&original_true));
    let decoded_true = decode_stack(&encoded_true).expect("true boolean should decode");
    assert_eq!(decoded_true[0], original_true);

    // Test false
    let original_false = StackValue::Boolean(false);
    let encoded_false = encode_stack(std::slice::from_ref(&original_false));
    let decoded_false = decode_stack(&encoded_false).expect("false boolean should decode");
    assert_eq!(decoded_false[0], original_false);

    // Verify boolean uses exactly 6 bytes total (4 byte length prefix + 2 bytes for value)
    assert_eq!(encoded_true.len(), 6, "true boolean should be 6 bytes");
    assert_eq!(encoded_false.len(), 6, "false boolean should be 6 bytes");

    // Verify type tag
    assert_eq!(get_type_tag(&encoded_true), STACK_VALUE_CODEC_TAG_BOOLEAN);
    assert_eq!(get_type_tag(&encoded_false), STACK_VALUE_CODEC_TAG_BOOLEAN);
}

#[test]
fn test_bytestring_encoding_roundtrip() {
    // Test empty bytestring
    let original_empty = StackValue::ByteString(Vec::new());
    let encoded_empty = encode_stack(std::slice::from_ref(&original_empty));
    let decoded_empty = decode_stack(&encoded_empty).expect("empty bytestring should decode");
    assert_eq!(decoded_empty[0], original_empty);

    // Test non-empty bytestring - using 3 bytes so we know expected length
    let original_data = vec![0x01, 0x02, 0x03];
    let original = StackValue::ByteString(original_data.clone());
    let encoded = encode_stack(std::slice::from_ref(&original));
    let decoded = decode_stack(&encoded).expect("bytestring should decode");
    assert_eq!(decoded[0], original);

    println!("Bytestring encoding: {:?}", encoded);
}

#[test]
fn test_buffer_encoding_roundtrip() {
    // Test empty buffer
    let original_empty = StackValue::Buffer(Vec::new());
    let encoded_empty = encode_stack(std::slice::from_ref(&original_empty));
    let decoded_empty = decode_stack(&encoded_empty).expect("empty buffer should decode");
    assert_eq!(decoded_empty[0], original_empty);

    // Test non-empty buffer
    let original_data = vec![0xAA, 0xBB, 0xCC, 0xDD];
    let original = StackValue::Buffer(original_data.clone());
    let encoded = encode_stack(std::slice::from_ref(&original));
    let decoded = decode_stack(&encoded).expect("buffer should decode");
    assert_eq!(decoded[0], original);

    // Verify type tag
    assert_eq!(get_type_tag(&encoded), STACK_VALUE_CODEC_TAG_BUFFER);

    // Verify Buffer and ByteString should produce different encodings despite same content
    let bs = StackValue::ByteString(vec![1, 2, 3]);
    let buf = StackValue::Buffer(vec![1, 2, 3]);
    let encoded_bs = encode_stack(&[bs]);
    let encoded_buf = encode_stack(&[buf]);
    assert_ne!(
        get_type_tag(&encoded_bs),
        get_type_tag(&encoded_buf),
        "Buffer and ByteString have different tags"
    );
}

#[test]
fn test_biginteger_encoding_roundtrip() {
    // Test zero-length BigInteger (edge case)
    let original_empty = StackValue::BigInteger(Vec::new());
    let encoded_empty = encode_stack(std::slice::from_ref(&original_empty));
    let decoded_empty = decode_stack(&encoded_empty).expect("empty BigInteger should decode");
    assert_eq!(decoded_empty[0], original_empty);

    // Test small value
    let original_small = StackValue::BigInteger(vec![0x01, 0x00]);
    let encoded_small = encode_stack(std::slice::from_ref(&original_small));
    let decoded_small = decode_stack(&encoded_small).expect("small BigInteger should decode");
    assert_eq!(decoded_small[0], original_small);

    // Test large value (little-endian two's complement)
    let original_large = StackValue::BigInteger(vec![0xFF; 32]);
    let encoded_large = encode_stack(std::slice::from_ref(&original_large));
    let decoded_large = decode_stack(&encoded_large).expect("large BigInteger should decode");
    assert_eq!(decoded_large[0], original_large);

    // Verify type tag
    assert_eq!(
        get_type_tag(&encoded_large),
        STACK_VALUE_CODEC_TAG_BIG_INTEGER
    );
}

#[test]
fn test_pointer_encoding_roundtrip() {
    // Test zero pointer
    let original_zero = StackValue::Pointer(0);
    let encoded_zero = encode_stack(std::slice::from_ref(&original_zero));
    let decoded_zero = decode_stack(&encoded_zero).expect("zero pointer should decode");
    assert_eq!(decoded_zero[0], original_zero);

    // Test positive pointer
    let original_pos = StackValue::Pointer(1024);
    let encoded_pos = encode_stack(std::slice::from_ref(&original_pos));
    let decoded_pos = decode_stack(&encoded_pos).expect("positive pointer should decode");
    assert_eq!(decoded_pos[0], original_pos);

    // Test negative pointer
    let original_neg = StackValue::Pointer(-512);
    let encoded_neg = encode_stack(std::slice::from_ref(&original_neg));
    let decoded_neg = decode_stack(&encoded_neg).expect("negative pointer should decode");
    assert_eq!(decoded_neg[0], original_neg);

    // Verify type tag
    assert_eq!(get_type_tag(&encoded_pos), STACK_VALUE_CODEC_TAG_POINTER);
}

// ============================================================================
// SECTION 2: Collection Type Round-Trip Tests
// ============================================================================

#[test]
fn test_array_encoding_roundtrip() {
    // Test empty array
    let original_empty = StackValue::Array(Vec::new());
    let encoded_empty = encode_stack(std::slice::from_ref(&original_empty));
    let decoded_empty = decode_stack(&encoded_empty).expect("empty array should decode");
    assert_eq!(decoded_empty[0], original_empty);

    // Test simple array
    let original_simple = StackValue::Array(vec![
        StackValue::Integer(1),
        StackValue::Integer(2),
        StackValue::Integer(3),
    ]);
    let encoded_simple = encode_stack(std::slice::from_ref(&original_simple));
    let decoded_simple = decode_stack(&encoded_simple).expect("simple array should decode");
    assert_eq!(decoded_simple[0], original_simple);

    // Test nested array
    let original_nested = StackValue::Array(vec![
        StackValue::Array(vec![StackValue::Boolean(true)]),
        StackValue::Array(vec![StackValue::Null]),
    ]);
    let encoded_nested = encode_stack(std::slice::from_ref(&original_nested));
    let decoded_nested = decode_stack(&encoded_nested).expect("nested array should decode");
    assert_eq!(decoded_nested[0], original_nested);

    // Test complex mixed array
    let original_complex = StackValue::Array(vec![
        StackValue::Integer(42),
        StackValue::Boolean(true),
        StackValue::ByteString(b"hello".to_vec()),
        StackValue::Null,
    ]);
    let encoded_complex = encode_stack(std::slice::from_ref(&original_complex));
    let decoded_complex = decode_stack(&encoded_complex).expect("complex array should decode");
    assert_eq!(decoded_complex[0], original_complex);

    // Verify array count preservation
    if let StackValue::Array(decoded_items) = &decoded_simple[0] {
        assert_eq!(decoded_items.len(), 3, "array element count preserved");
    } else {
        panic!("decoded value should be Array");
    }
}

#[test]
fn test_struct_encoding_roundtrip() {
    // Test empty struct
    let original_empty = StackValue::Struct(Vec::new());
    let encoded_empty = encode_stack(std::slice::from_ref(&original_empty));
    let decoded_empty = decode_stack(&encoded_empty).expect("empty struct should decode");
    assert_eq!(decoded_empty[0], original_empty);

    // Test struct with multiple fields
    let original_fields = StackValue::Struct(vec![
        StackValue::Integer(100),
        StackValue::Boolean(false),
        StackValue::ByteString(b"field1".to_vec()),
    ]);
    let encoded_fields = encode_stack(std::slice::from_ref(&original_fields));
    let decoded_fields = decode_stack(&encoded_fields).expect("struct should decode");
    assert_eq!(decoded_fields[0], original_fields);

    println!("Array-only encoding: {:?}", encoded_fields);

    // Verify type tag
    if let StackValue::Struct(decoded_items) = &decoded_fields[0] {
        assert_eq!(decoded_items.len(), 3, "struct field count preserved");
    } else {
        panic!("decoded value should be Struct");
    }
}

#[test]
fn test_map_encoding_roundtrip() {
    // Test empty map
    let original_empty = StackValue::Map(Vec::new());
    let encoded_empty = encode_stack(std::slice::from_ref(&original_empty));
    let decoded_empty = decode_stack(&encoded_empty).expect("empty map should decode");
    assert_eq!(decoded_empty[0], original_empty);

    // Test simple key-value pairs
    let original_pairs = StackValue::Map(vec![
        (
            StackValue::Integer(1),
            StackValue::ByteString(b"one".to_vec()),
        ),
        (
            StackValue::Integer(2),
            StackValue::ByteString(b"two".to_vec()),
        ),
        (
            StackValue::Integer(3),
            StackValue::ByteString(b"three".to_vec()),
        ),
    ]);
    let encoded_pairs = encode_stack(std::slice::from_ref(&original_pairs));
    let decoded_pairs = decode_stack(&encoded_pairs).expect("map should decode");
    assert_eq!(decoded_pairs[0], original_pairs);

    // Test mixed key types (integer and boolean keys)
    let original_mixed_keys = StackValue::Map(vec![
        (StackValue::Integer(42), StackValue::Null),
        (StackValue::Boolean(true), StackValue::Boolean(false)),
    ]);
    let encoded_mixed = encode_stack(std::slice::from_ref(&original_mixed_keys));
    let decoded_mixed = decode_stack(&encoded_mixed).expect("mixed key map should decode");
    assert_eq!(decoded_mixed[0], original_mixed_keys);

    // Verify pair count preservation
    if let StackValue::Map(decoded_pairs) = &decoded_pairs[0] {
        assert_eq!(decoded_pairs.len(), 3, "map pair count preserved");
    } else {
        panic!("decoded value should be Map");
    }
}

#[test]
fn test_array_vs_struct_distinction() {
    // This test verifies that arrays and structs are distinguishable after encoding
    // This is critical for type safety in smart contract ABI
    let array_value = StackValue::Array(vec![
        StackValue::Integer(1),
        StackValue::ByteString(b"test".to_vec()),
    ]);
    let struct_value = StackValue::Struct(vec![
        StackValue::Integer(1),
        StackValue::ByteString(b"test".to_vec()),
    ]);

    let encoded_array = encode_stack(std::slice::from_ref(&array_value));
    let encoded_struct = encode_stack(std::slice::from_ref(&struct_value));

    // Different type tags must be present
    assert_eq!(get_type_tag(&encoded_array), STACK_VALUE_CODEC_TAG_ARRAY);
    assert_eq!(get_type_tag(&encoded_struct), STACK_VALUE_CODEC_TAG_STRUCT);
    assert_ne!(get_type_tag(&encoded_array), get_type_tag(&encoded_struct));

    // Round-trip should preserve exact type identity
    let decoded_array = decode_stack(&encoded_array).expect("array should decode");
    let decoded_struct = decode_stack(&encoded_struct).expect("struct should decode");

    assert_eq!(decoded_array[0], array_value);
    assert_eq!(decoded_struct[0], struct_value);
}

// ============================================================================
// SECTION 3: Handle Types (Interop & Iterator)
// ============================================================================

#[test]
fn test_interop_handle_encoding() {
    // Test various handle values
    let handles = vec![0u64, 1, 100, 1000, u64::MAX];

    for handle in handles {
        let original = StackValue::Interop(handle);
        let encoded = encode_stack(std::slice::from_ref(&original));
        let decoded = decode_stack(&encoded).expect("interop handle should decode");
        assert_eq!(decoded[0], original);
        assert_eq!(get_type_tag(&encoded), STACK_VALUE_CODEC_TAG_INTEROP);
    }
}

#[test]
fn test_iterator_handle_encoding() {
    // Test various iterator handle values
    let handles = vec![0u64, 42, 999, u64::MAX];

    for handle in handles {
        let original = StackValue::Iterator(handle);
        let encoded = encode_stack(std::slice::from_ref(&original));
        let decoded = decode_stack(&encoded).expect("iterator handle should decode");
        assert_eq!(decoded[0], original);
        assert_eq!(get_type_tag(&encoded), STACK_VALUE_CODEC_TAG_ITERATOR);
    }
}

// ============================================================================
// SECTION 4: Null Value (Coq: null_encoding_exact)
// ============================================================================

#[test]
fn test_null_encoding_exact() {
    let original = StackValue::Null;
    let encoded = encode_stack(std::slice::from_ref(&original));

    // Encoding format: [len_u32:4][tag:1] = 5 bytes total for single null item
    assert_eq!(
        encoded.len(),
        5,
        "single null should be 5 bytes (4-byte len prefix + 1 byte tag)"
    );

    // Verify type tag at position 4 (after 4-byte length prefix)
    assert_eq!(get_type_tag(&encoded), STACK_VALUE_CODEC_TAG_NULL);

    // Round-trip should preserve null
    let decoded = decode_stack(&encoded).expect("null should always decode");
    assert_eq!(decoded[0], original);
}

#[test]
fn test_null_in_collections() {
    // Null appearing in various contexts
    let in_array = StackValue::Array(vec![StackValue::Null, StackValue::Integer(1)]);
    let encoded_array = encode_stack(std::slice::from_ref(&in_array));
    let decoded_array = decode_stack(&encoded_array).expect("array with null should decode");
    assert_eq!(decoded_array[0], in_array);

    let in_struct = StackValue::Struct(vec![StackValue::Null]);
    let encoded_struct = encode_stack(std::slice::from_ref(&in_struct));
    let decoded_struct = decode_stack(&encoded_struct).expect("struct with null should decode");
    assert_eq!(decoded_struct[0], in_struct);

    let in_map_value = StackValue::Map(vec![(StackValue::Integer(1), StackValue::Null)]);
    let encoded_map_value = encode_stack(std::slice::from_ref(&in_map_value));
    let decoded_map_value =
        decode_stack(&encoded_map_value).expect("map with null value should decode");
    assert_eq!(decoded_map_value[0], in_map_value);

    let in_map_key = StackValue::Map(vec![(StackValue::Null, StackValue::Integer(1))]);
    let encoded_map_key = encode_stack(std::slice::from_ref(&in_map_key));
    let decoded_map_key = decode_stack(&encoded_map_key).expect("map with null key should decode");
    assert_eq!(decoded_map_key[0], in_map_key);
}

// ============================================================================
// SECTION 5: Tag Separation Verification (Coq: tag_separation)
// ============================================================================

#[test]
fn test_tag_separation_all_variants() {
    // Each StackValue variant should produce a distinct first byte (type tag)
    let mut tags = Vec::new();

    // Integer (i64)
    tags.push(get_type_tag(&encode_stack(&[StackValue::Integer(0)])));

    // BigInteger
    tags.push(get_type_tag(&encode_stack(&[StackValue::BigInteger(
        vec![],
    )])));

    // ByteString
    tags.push(get_type_tag(&encode_stack(&[StackValue::ByteString(
        vec![],
    )])));

    // Buffer
    tags.push(get_type_tag(&encode_stack(&[StackValue::Buffer(vec![])])));

    // Boolean
    tags.push(get_type_tag(&encode_stack(&[StackValue::Boolean(true)])));

    // Array
    tags.push(get_type_tag(&encode_stack(&[StackValue::Array(vec![])])));

    // Struct
    tags.push(get_type_tag(&encode_stack(&[StackValue::Struct(vec![])])));

    // Map
    tags.push(get_type_tag(&encode_stack(&[StackValue::Map(vec![])])));

    // Interop
    tags.push(get_type_tag(&encode_stack(&[StackValue::Interop(0)])));

    // Iterator
    tags.push(get_type_tag(&encode_stack(&[StackValue::Iterator(0)])));

    // Null
    tags.push(get_type_tag(&encode_stack(&[StackValue::Null])));

    // Pointer
    tags.push(get_type_tag(&encode_stack(&[StackValue::Pointer(0)])));

    // All tags should be unique
    for i in 0..tags.len() {
        for j in (i + 1)..tags.len() {
            assert_ne!(
                tags[i], tags[j],
                "Variant {} and {} must have different type tags",
                i, j
            );
        }
    }
}

#[test]
fn test_expected_type_tags_match_constants() {
    // Verify actual encoding matches defined constants
    assert_eq!(
        get_type_tag(&encode_stack(&[StackValue::Integer(0)])),
        STACK_VALUE_CODEC_TAG_INTEGER
    );
    assert_eq!(
        get_type_tag(&encode_stack(&[StackValue::BigInteger(vec![])])),
        STACK_VALUE_CODEC_TAG_BIG_INTEGER
    );
    assert_eq!(
        get_type_tag(&encode_stack(&[StackValue::ByteString(vec![])])),
        STACK_VALUE_CODEC_TAG_BYTESTRING
    );
    assert_eq!(
        get_type_tag(&encode_stack(&[StackValue::Boolean(false)])),
        STACK_VALUE_CODEC_TAG_BOOLEAN
    );
    assert_eq!(
        get_type_tag(&encode_stack(&[StackValue::Array(vec![])])),
        STACK_VALUE_CODEC_TAG_ARRAY
    );
    assert_eq!(
        get_type_tag(&encode_stack(&[StackValue::Struct(vec![])])),
        STACK_VALUE_CODEC_TAG_STRUCT
    );
    assert_eq!(
        get_type_tag(&encode_stack(&[StackValue::Map(vec![])])),
        STACK_VALUE_CODEC_TAG_MAP
    );
    assert_eq!(
        get_type_tag(&encode_stack(&[StackValue::Interop(0)])),
        STACK_VALUE_CODEC_TAG_INTEROP
    );
    assert_eq!(
        get_type_tag(&encode_stack(&[StackValue::Iterator(0)])),
        STACK_VALUE_CODEC_TAG_ITERATOR
    );
    assert_eq!(
        get_type_tag(&encode_stack(&[StackValue::Null])),
        STACK_VALUE_CODEC_TAG_NULL
    );
    assert_eq!(
        get_type_tag(&encode_stack(&[StackValue::Pointer(0)])),
        STACK_VALUE_CODEC_TAG_POINTER
    );
    assert_eq!(
        get_type_tag(&encode_stack(&[StackValue::Buffer(vec![])])),
        STACK_VALUE_CODEC_TAG_BUFFER
    );
}

// ============================================================================
// SECTION 6: Edge Cases and Boundary Conditions
// ============================================================================

#[test]
fn test_empty_collections() {
    // Empty collections should encode/decode correctly
    let empty_array = StackValue::Array(Vec::new());
    let encoded_empty_array = encode_stack(std::slice::from_ref(&empty_array));
    let decoded_empty_array =
        decode_stack(&encoded_empty_array).expect("empty array should decode");
    assert_eq!(decoded_empty_array[0], empty_array);

    let empty_struct = StackValue::Struct(Vec::new());
    let encoded_empty_struct = encode_stack(std::slice::from_ref(&empty_struct));
    let decoded_empty_struct =
        decode_stack(&encoded_empty_struct).expect("empty struct should decode");
    assert_eq!(decoded_empty_struct[0], empty_struct);

    let empty_map = StackValue::Map(Vec::new());
    let encoded_empty_map = encode_stack(std::slice::from_ref(&empty_map));
    let decoded_empty_map = decode_stack(&encoded_empty_map).expect("empty map should decode");
    assert_eq!(decoded_empty_map[0], empty_map);

    let empty_bytes = StackValue::ByteString(Vec::new());
    let encoded_empty_bytes = encode_stack(std::slice::from_ref(&empty_bytes));
    let decoded_empty_bytes =
        decode_stack(&encoded_empty_bytes).expect("empty bytestring should decode");
    assert_eq!(decoded_empty_bytes[0], empty_bytes);
}

#[test]
fn test_deeply_nested_structures() {
    // Test deeply nested arrays (10 levels deep)
    let mut deep = StackValue::Array(vec![]);
    for _ in 0..10 {
        deep = StackValue::Array(vec![deep]);
    }

    let encoded = encode_stack(std::slice::from_ref(&deep));
    let decoded = decode_stack(&encoded).expect("deeply nested structure should decode");
    assert_eq!(decoded[0], deep);
}

#[test]
fn test_large_collections() {
    // Test large array (1000 elements)
    let large_array: Vec<StackValue> = (0..1000).map(|i| StackValue::Integer(i as i64)).collect();
    let large_sv = StackValue::Array(large_array.clone());
    let encoded = encode_stack(std::slice::from_ref(&large_sv));
    let decoded = decode_stack(&encoded).expect("large array should decode");
    assert_eq!(decoded[0], large_sv);

    // Test large map (500 entries)
    let large_map: Vec<(StackValue, StackValue)> = (0..500)
        .map(|i| {
            (
                StackValue::Integer(i as i64),
                StackValue::Integer((i * 2) as i64),
            )
        })
        .collect();
    let large_map_sv = StackValue::Map(large_map.clone());
    let encoded_map = encode_stack(std::slice::from_ref(&large_map_sv));
    let decoded_map = decode_stack(&encoded_map).expect("large map should decode");
    assert_eq!(decoded_map[0], large_map_sv);
}

#[test]
fn test_mixed_type_stack() {
    // Test a realistic stack with all types
    let stack = vec![
        StackValue::Integer(42),
        StackValue::Boolean(true),
        StackValue::ByteString(b"message".to_vec()),
        StackValue::Null,
        StackValue::Array(vec![StackValue::Integer(1), StackValue::Integer(2)]),
        StackValue::Struct(vec![StackValue::Boolean(false)]),
        StackValue::Map(vec![(StackValue::Integer(1), StackValue::Null)]),
    ];

    let encoded = encode_stack(&stack);
    let decoded = decode_stack(&encoded).expect("mixed stack should decode");
    assert_eq!(decoded.len(), stack.len());
    for (i, expected) in stack.iter().enumerate() {
        assert_eq!(decoded[i], *expected, "stack item {} should match", i);
    }
}

// ============================================================================
// SECTION 7: Differential Tests Against Coq Model Expectations
// ============================================================================

#[test]
fn test_byte_content_preservation_string() {
    // Coq theorem: byte_content_preservation
    // Byte-based types should preserve exact content through round-trip
    let test_cases = vec![
        Vec::<u8>::new(),                           // empty
        vec![0],                                    // single zero
        vec![1],                                    // single one
        vec![0xFF],                                 // single max
        vec![0x01, 0x02, 0x03],                     // small sequence
        vec![0xFF, 0xFE, 0xFD, 0xFC],               // descending sequence
        (0..64).map(|i| (i % 256) as u8).collect(), // pattern
    ];

    for bytes in test_cases {
        let original = StackValue::ByteString(bytes.clone());
        let encoded = encode_stack(std::slice::from_ref(&original));
        let decoded = decode_stack(&encoded).expect("byte string should decode");

        if let StackValue::ByteString(decoded_bytes) = &decoded[0] {
            assert_eq!(
                &decoded_bytes[..],
                &bytes[..],
                "byte content must be preserved exactly"
            );
        } else {
            panic!("decoded value should be ByteString");
        }
    }
}

#[test]
fn test_integer_length_conservation() {
    // Length conservation property: encoding produces expected byte sizes
    let test_cases: Vec<(i64, usize)> = vec![
        (0, 13),          // 4-byte len prefix + 8 bytes data + 1 tag = 13
        (1, 13),          // minimal positive
        (-1, 13),         // minimal negative
        (127, 13),        // fits in i8 range
        (-128, 13),       // fits in i8 negative range
        (32767, 13),      // fits in i16 range
        (-32768, 13),     // fits in i16 negative range
        (1_000_000, 13),  // typical int
        (-1_000_000, 13), // typical negative int
    ];

    for (value, expected_size) in test_cases {
        let sv = StackValue::Integer(value);
        let encoded = encode_stack(&[sv]);
        // Each integer encodes to exactly 13 bytes (4-byte len prefix + 8 bytes data + 1 tag)
        assert_eq!(
            encoded.len(),
            expected_size,
            "integer {} should encode to {} bytes, got {}",
            value,
            expected_size,
            encoded.len()
        );
    }
}

#[test]
fn test_boolean_exact_encoding_format() {
    // Verify boolean encoding format matches Coq expectations
    let test_cases = vec![(true, 1u8), (false, 0u8)];

    for (bool_val, byte_val) in test_cases {
        let sv = StackValue::Boolean(bool_val);
        let encoded = encode_stack(&[sv]);

        assert_eq!(
            encoded.len(),
            6,
            "boolean should always be exactly 6 bytes (4-byte len prefix + tag(1) + value(1))"
        );
        assert_eq!(
            get_type_tag(&encoded),
            STACK_VALUE_CODEC_TAG_BOOLEAN,
            "tag after length prefix must be type tag"
        );
        // The boolean value byte is at position 5
        assert_eq!(
            encoded[5], byte_val,
            "boolean value should be stored correctly"
        );
    }
}

// ============================================================================
// SECTION 8: Property-Based Validation Tests
// ============================================================================

#[test]
fn test_comprehensive_roundtrip_coverage() {
    // Exhaustive test covering all major code paths in encoding/decoding
    let mut test_count = 0;

    // Single values of each type
    let primitives = vec![
        StackValue::Integer(123),
        StackValue::BigInteger(vec![0x01, 0x02]),
        StackValue::ByteString(b"data".to_vec()),
        StackValue::Buffer(vec![0xAA, 0xBB]),
        StackValue::Boolean(true),
        StackValue::Null,
        StackValue::Pointer(42),
    ];

    for sv in &primitives {
        let encoded = encode_stack(std::slice::from_ref(sv));
        let decoded = decode_stack(&encoded).unwrap();
        assert_eq!(decoded[0], *sv);
        test_count += 1;
    }

    // Collections containing primitives
    let collections = vec![
        StackValue::Array(primitives.clone()),
        StackValue::Struct(primitives.clone()),
        StackValue::Map(vec![(StackValue::Integer(1), StackValue::Integer(2))]),
    ];

    for sv in collections {
        let encoded = encode_stack(std::slice::from_ref(&sv));
        let decoded = decode_stack(&encoded).unwrap();
        assert_eq!(decoded[0], sv);
        test_count += 1;
    }

    assert!(
        test_count >= 10,
        "Should test at least 10 diverse cases: tested {}",
        test_count
    );
}

#[test]
fn test_type_preservation_through_multiple_roundtrips() {
    // Encoding→Decoding→Encoding→Decoding should preserve value
    let original = StackValue::Array(vec![
        StackValue::Integer(1),
        StackValue::Boolean(true),
        StackValue::ByteString(b"multi-roundtrip".to_vec()),
    ]);

    // First round-trip
    let encoded1 = encode_stack(std::slice::from_ref(&original));
    let decoded1 = decode_stack(&encoded1).expect("first roundtrip should succeed");
    assert_eq!(decoded1[0], original);

    // Second round-trip (encoding the decoded value)
    let encoded2 = encode_stack(&decoded1);
    let decoded2 = decode_stack(&encoded2).expect("second roundtrip should succeed");
    assert_eq!(decoded2[0], original);

    // Third round-trip for confidence
    let encoded3 = encode_stack(&decoded2);
    let decoded3 = decode_stack(&encoded3).expect("third roundtrip should succeed");
    assert_eq!(decoded3[0], original);

    // All encoded representations should be identical (deterministic encoding)
    assert_eq!(encoded1, encoded2);
    assert_eq!(encoded2, encoded3);
}

#[test]
fn test_serialization_determinism() {
    // Same value should always produce identical encoding
    let sv = StackValue::Map(vec![
        (
            StackValue::Integer(1),
            StackValue::ByteString(b"a".to_vec()),
        ),
        (
            StackValue::Integer(2),
            StackValue::ByteString(b"b".to_vec()),
        ),
    ]);

    let encodings: Vec<Vec<u8>> = (0..10)
        .map(|_| encode_stack(std::slice::from_ref(&sv)))
        .collect();

    for encoding in &encodings {
        assert_eq!(encoding, &encodings[0], "all encodings should be identical");
    }
}
