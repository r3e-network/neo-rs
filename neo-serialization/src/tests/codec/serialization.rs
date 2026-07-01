use super::*;

// ============================================================================
// Binary Serialization Tests
// ============================================================================

#[test]
fn serialize_and_deserialize_roundtrip() {
    let data: Vec<i32> = vec![1, 2, 3, 4, 5];
    let serialized = serialize(&data).unwrap();
    let deserialized: Vec<i32> = deserialize(&serialized).unwrap();
    assert_eq!(data, deserialized);
}

#[test]
fn serialize_string() {
    let data = "Hello, Neo!".to_string();
    let serialized = serialize(&data).unwrap();
    let deserialized: String = deserialize(&serialized).unwrap();
    assert_eq!(data, deserialized);
}

#[test]
fn deserialize_empty_data_returns_error() {
    let result: neo_error::Result<i32> = deserialize(&[]);
    assert!(result.is_err());
}

// ============================================================================
// JSON Serialization Tests
// ============================================================================

#[test]
fn serialize_json_and_deserialize_roundtrip() {
    let data: Vec<i32> = vec![1, 2, 3, 4, 5];
    let json = serialize_json(&data).unwrap();
    let deserialized: Vec<i32> = deserialize_json(&json).unwrap();
    assert_eq!(data, deserialized);
}

#[test]
fn serialize_json_string() {
    let data = "Hello, Neo!".to_string();
    let json = serialize_json(&data).unwrap();
    assert!(json.contains("Hello, Neo!"));
}

#[test]
fn deserialize_json_empty_returns_error() {
    let result: neo_error::Result<i32> = deserialize_json("");
    assert!(result.is_err());
}

#[test]
fn deserialize_json_whitespace_only_returns_error() {
    let result: neo_error::Result<i32> = deserialize_json("   ");
    assert!(result.is_err());
}

// ============================================================================
// Size Estimation Tests
// ============================================================================

#[test]
fn estimate_serialized_size_matches_actual() {
    let data: Vec<i32> = vec![1, 2, 3, 4, 5];
    let estimated = estimate_serialized_size(&data).unwrap();
    let actual = serialize(&data).unwrap().len();
    assert_eq!(estimated, actual);
}

// ============================================================================
// Validation Tests
// ============================================================================

#[test]
fn validate_serialization_returns_true_for_valid_data() {
    let data: Vec<i32> = vec![1, 2, 3, 4, 5];
    let result = validate_serialization(&data).unwrap();
    assert!(result);
}

#[test]
fn validate_serialization_string() {
    let data = "Test string".to_string();
    let result = validate_serialization(&data).unwrap();
    assert!(result);
}

// ============================================================================
// Compression Tests
// ============================================================================

#[test]
fn compress_data_and_decompress_roundtrip() {
    let data = b"Hello, Neo blockchain! This is test data.".to_vec();
    let compressed = compress_data(&data).unwrap();
    let decompressed = decompress_data(&compressed).unwrap();
    assert_eq!(data, decompressed);
}

#[test]
fn decompress_data_empty_returns_error() {
    let result = decompress_data(&[]);
    assert!(result.is_err());
}

#[test]
fn compression_wrapper_rejects_declared_size_bomb() {
    let mut compressed = Vec::new();
    compressed.extend_from_slice(&1025u32.to_le_bytes());
    compressed.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]);

    let result = crate::compression::Compression::decompress_lz4(&compressed, 1024);
    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("exceeds maximum size"),
        "canonical LZ4 wrapper must reject declared output sizes before decompression"
    );
}

#[test]
fn compress_data_large_input() {
    let data: Vec<u8> = (0..10000).map(|i| (i % 256) as u8).collect();
    let compressed = compress_data(&data).unwrap();
    let decompressed = decompress_data(&compressed).unwrap();
    assert_eq!(data, decompressed);
}
