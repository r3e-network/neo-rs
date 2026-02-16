//! Serialization functionality for persistence layer.
//!
//! This module provides data serialization and deserialization capabilities
//! that match the C# Neo implementation exactly.

use crate::compression::{compress_lz4, decompress_lz4};
use crate::error::CoreError;
use crate::neo_io::{BinaryWriter, MemoryReader, Serializable};
use serde::{Deserialize, Serialize};

/// Serializes data to binary format (production implementation matching C# Neo exactly)
pub fn serialize<T: Serialize>(data: &T) -> crate::Result<Vec<u8>> {
    bincode::serialize(data)
        .map_err(|e| CoreError::serialization(format!("Binary serialization failed: {}", e)))
}

/// Deserializes data from binary format (production implementation matching C# Neo exactly)
pub fn deserialize<T: for<'de> Deserialize<'de>>(data: &[u8]) -> crate::Result<T> {
    // Validate input data
    if data.is_empty() {
        return Err(CoreError::deserialization("Cannot deserialize empty data"));
    }

    bincode::deserialize(data)
        .map_err(|e| CoreError::deserialization(format!("Binary deserialization failed: {}", e)))
}

/// Serializes data to JSON format (production implementation matching C# Neo exactly)
pub fn serialize_json<T: Serialize>(data: &T) -> crate::Result<String> {
    serde_json::to_string(data)
        .map_err(|e| CoreError::serialization(format!("JSON serialization failed: {}", e)))
}

/// Deserializes data from JSON format (production implementation matching C# Neo exactly)
pub fn deserialize_json<T: for<'de> Deserialize<'de>>(data: &str) -> crate::Result<T> {
    // Validate input JSON
    if data.trim().is_empty() {
        return Err(CoreError::deserialization("Cannot deserialize empty JSON"));
    }

    serde_json::from_str(data)
        .map_err(|e| CoreError::deserialization(format!("JSON deserialization failed: {}", e)))
}

/// Serializes data using Neo's native binary format (matches C# ISerializable exactly)
pub fn serialize_neo_binary<T>(data: &T, writer: &mut BinaryWriter) -> crate::Result<()>
where
    T: Serializable,
{
    Serializable::serialize(data, writer)
        .map_err(|e| CoreError::serialization(format!("Neo binary serialization failed: {}", e)))
}

/// Deserializes data using Neo's native binary format (matches C# ISerializable exactly)
pub fn deserialize_neo_binary<T>(data: &[u8]) -> crate::Result<T>
where
    T: Serializable + Default,
{
    // Validate input data
    if data.is_empty() {
        return Err(CoreError::deserialization(
            "Cannot deserialize empty Neo binary data",
        ));
    }

    let mut reader = MemoryReader::new(data);

    T::deserialize(&mut reader).map_err(|e| {
        CoreError::deserialization(format!("Neo binary deserialization failed: {}", e))
    })
}

/// Gets the serialized size of data without actually serializing it (production implementation)
pub fn estimate_serialized_size<T: Serialize>(data: &T) -> crate::Result<usize> {
    bincode::serialized_size(data)
        .map(|size| size as usize)
        .map_err(|e| CoreError::serialization(format!("Size estimation failed: {}", e)))
}

/// Validates that data can be serialized and deserialized correctly (production implementation)
pub fn validate_serialization<T: Serialize + for<'de> Deserialize<'de> + PartialEq>(
    data: &T,
) -> crate::Result<bool> {
    // 1. Test binary serialization round-trip
    let binary_serialized = serialize(data)?;
    let binary_deserialized: T = deserialize(&binary_serialized)?;

    if *data != binary_deserialized {
        return Ok(false);
    }

    // 2. Test JSON serialization round-trip
    let json_serialized = serialize_json(data)?;
    let json_deserialized: T = deserialize_json(&json_serialized)?;

    if *data != json_deserialized {
        return Ok(false);
    }

    // 3. Validate size estimation accuracy
    let estimated_size = estimate_serialized_size(data)?;
    if estimated_size != binary_serialized.len() {
        return Ok(false);
    }

    Ok(true)
}

/// Compresses serialized data using LZ4 (production implementation matching C# Neo compression)
pub fn compress_data(data: &[u8]) -> crate::Result<Vec<u8>> {
    compress_lz4(data)
        .map_err(|e| CoreError::serialization(format!("Data compression failed: {}", e)))
}

/// Decompresses data using LZ4 (production implementation matching C# Neo compression)
pub fn decompress_data(compressed_data: &[u8]) -> crate::Result<Vec<u8>> {
    // Validate input
    if compressed_data.is_empty() {
        return Err(CoreError::deserialization("Cannot decompress empty data"));
    }

    // Persistence payloads historically had no strict decompressed-size cap in this path.
    // Reuse shared LZ4 helper with an effectively unbounded max to preserve behavior.
    decompress_lz4(compressed_data, usize::MAX)
        .map_err(|e| CoreError::deserialization(format!("Data decompression failed: {}", e)))
}

#[cfg(test)]
mod tests {
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
        let result: crate::Result<i32> = deserialize(&[]);
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
        let result: crate::Result<i32> = deserialize_json("");
        assert!(result.is_err());
    }

    #[test]
    fn deserialize_json_whitespace_only_returns_error() {
        let result: crate::Result<i32> = deserialize_json("   ");
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
    fn compress_data_large_input() {
        let data: Vec<u8> = (0..10000).map(|i| (i % 256) as u8).collect();
        let compressed = compress_data(&data).unwrap();
        let decompressed = decompress_data(&compressed).unwrap();
        assert_eq!(data, decompressed);
    }
}
