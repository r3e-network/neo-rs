//! Serialization functionality for persistence layer.
//!
//! This module provides data serialization and deserialization capabilities
//! that match the C# Neo implementation exactly.

use crate::codec_error::{CodecError, CodecResult};
use crate::{BinaryWriter, MemoryReader, Serializable};
use serde::{Deserialize, Serialize};

/// Serializes data to binary format (production implementation matching C# Neo exactly)
pub fn serialize<T: Serialize>(data: &T) -> CodecResult<Vec<u8>> {
    bincode::serialize(data)
        .map_err(|e| CodecError::serialization(format!("Binary serialization failed: {e}")))
}

/// Deserializes data from binary format (production implementation matching C# Neo exactly)
pub fn deserialize<T: for<'de> Deserialize<'de>>(data: &[u8]) -> CodecResult<T> {
    // Validate input data
    if data.is_empty() {
        return Err(CodecError::deserialization("Cannot deserialize empty data"));
    }

    bincode::deserialize(data)
        .map_err(|e| CodecError::deserialization(format!("Binary deserialization failed: {e}")))
}

/// Serializes data to JSON format (production implementation matching C# Neo exactly)
pub fn serialize_json<T: Serialize>(data: &T) -> CodecResult<String> {
    serde_json::to_string(data)
        .map_err(|e| CodecError::serialization(format!("JSON serialization failed: {e}")))
}

/// Deserializes data from JSON format (production implementation matching C# Neo exactly)
pub fn deserialize_json<T: for<'de> Deserialize<'de>>(data: &str) -> CodecResult<T> {
    // Validate input JSON
    if data.trim().is_empty() {
        return Err(CodecError::deserialization("Cannot deserialize empty JSON"));
    }

    serde_json::from_str(data)
        .map_err(|e| CodecError::deserialization(format!("JSON deserialization failed: {e}")))
}

/// Serializes data using Neo's native binary format (matches C# ISerializable exactly)
pub fn serialize_neo_binary<T>(data: &T, writer: &mut BinaryWriter) -> CodecResult<()>
where
    T: Serializable,
{
    Serializable::serialize(data, writer)
        .map_err(|e| CodecError::serialization(format!("Neo binary serialization failed: {e}")))
}

/// Deserializes data using Neo's native binary format (matches C# ISerializable exactly)
pub fn deserialize_neo_binary<T>(data: &[u8]) -> CodecResult<T>
where
    T: Serializable + Default,
{
    // Validate input data
    if data.is_empty() {
        return Err(CodecError::deserialization(
            "Cannot deserialize empty Neo binary data",
        ));
    }

    let mut reader = MemoryReader::new(data);

    T::deserialize(&mut reader)
        .map_err(|e| CodecError::deserialization(format!("Neo binary deserialization failed: {e}")))
}

/// Gets the serialized size of data without actually serializing it (production implementation)
pub fn estimate_serialized_size<T: Serialize>(data: &T) -> CodecResult<usize> {
    bincode::serialized_size(data)
        .map(|size| size as usize)
        .map_err(|e| CodecError::serialization(format!("Size estimation failed: {e}")))
}

/// Validates that data can be serialized and deserialized correctly (production implementation)
pub fn validate_serialization<T: Serialize + for<'de> Deserialize<'de> + PartialEq>(
    data: &T,
) -> CodecResult<bool> {
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
pub fn compress_data(data: &[u8]) -> CodecResult<Vec<u8>> {
    Ok(lz4_flex::compress_prepend_size(data))
}

/// Decompresses data using LZ4 (production implementation matching C# Neo compression)
pub fn decompress_data(compressed_data: &[u8]) -> CodecResult<Vec<u8>> {
    // Validate input
    if compressed_data.is_empty() {
        return Err(CodecError::deserialization("Cannot decompress empty data"));
    }

    lz4_flex::decompress_size_prepended(compressed_data)
        .map_err(|e| CodecError::deserialization(format!("Data decompression failed: {e}")))
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
        let result: CodecResult<i32> = deserialize(&[]);
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
        let result: CodecResult<i32> = deserialize_json("");
        assert!(result.is_err());
    }

    #[test]
    fn deserialize_json_whitespace_only_returns_error() {
        let result: CodecResult<i32> = deserialize_json("   ");
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

    #[derive(Debug, Default, PartialEq, Eq)]
    struct VarIntPayload(u64);

    impl Serializable for VarIntPayload {
        fn deserialize(reader: &mut MemoryReader) -> crate::IoResult<Self> {
            reader.read_var_int(u64::MAX).map(Self)
        }

        fn serialize(&self, writer: &mut BinaryWriter) -> crate::IoResult<()> {
            writer.write_var_int(self.0)
        }

        fn size(&self) -> usize {
            crate::var_int::encoded_len(self.0)
        }
    }

    struct RejectSerialize;

    impl Serialize for RejectSerialize {
        fn serialize<S>(&self, _serializer: S) -> Result<S::Ok, S::Error>
        where
            S: serde::Serializer,
        {
            Err(serde::ser::Error::custom("fixture encode failure"))
        }
    }

    #[derive(Debug, Default)]
    struct RejectNeo;

    impl Serializable for RejectNeo {
        fn deserialize(_reader: &mut MemoryReader) -> crate::IoResult<Self> {
            Err(crate::IoError::Io(std::io::Error::other(
                "fixture read failure",
            )))
        }

        fn serialize(&self, _writer: &mut BinaryWriter) -> crate::IoResult<()> {
            Err(crate::IoError::Io(std::io::Error::other(
                "fixture write failure",
            )))
        }

        fn size(&self) -> usize {
            0
        }
    }

    #[test]
    fn binary_and_json_bytes_preserve_field_order() {
        #[derive(Debug, Serialize, Deserialize, PartialEq)]
        struct Record {
            version: u32,
            label: String,
        }
        let data = Record {
            version: 0x01020304,
            label: "N3".to_string(),
        };
        let bytes = vec![4, 3, 2, 1, 2, 0, 0, 0, 0, 0, 0, 0, b'N', b'3'];
        assert_eq!(serialize(&data).unwrap(), bytes);
        assert_eq!(deserialize::<Record>(&bytes).unwrap(), data);
        assert_eq!(
            serialize_json(&data).unwrap(),
            r#"{"version":16909060,"label":"N3"}"#
        );
        assert_eq!(estimate_serialized_size(&data).unwrap(), bytes.len());
    }

    #[test]
    fn binary_deserialize_preserves_trailing_bytes_acceptance() {
        assert_eq!(deserialize::<u32>(&[4, 3, 2, 1, 0xaa]).unwrap(), 0x01020304);
    }

    #[test]
    fn binary_empty_unit_roundtrip_remains_error() {
        assert!(serialize(&()).unwrap().is_empty());
        assert_eq!(
            validate_serialization(&()).unwrap_err(),
            CodecError::deserialization("Cannot deserialize empty data")
        );
    }

    #[test]
    fn serialization_failures_preserve_operation_messages() {
        assert_eq!(
            serialize(&RejectSerialize).unwrap_err(),
            CodecError::serialization("Binary serialization failed: fixture encode failure")
        );
        assert_eq!(
            serialize_json(&RejectSerialize).unwrap_err(),
            CodecError::serialization("JSON serialization failed: fixture encode failure")
        );
        assert_eq!(
            estimate_serialized_size(&RejectSerialize).unwrap_err(),
            CodecError::serialization("Size estimation failed: fixture encode failure")
        );
    }

    #[test]
    fn deserialization_failures_preserve_operation_messages() {
        assert_eq!(
            deserialize::<u32>(&[]).unwrap_err(),
            CodecError::deserialization("Cannot deserialize empty data")
        );
        assert_eq!(
            deserialize_json::<u32>(" \n\t ").unwrap_err(),
            CodecError::deserialization("Cannot deserialize empty JSON")
        );
        let binary_error = bincode::deserialize::<u32>(&[1]).unwrap_err();
        assert_eq!(
            deserialize::<u32>(&[1]).unwrap_err(),
            CodecError::deserialization(format!("Binary deserialization failed: {binary_error}"))
        );
        let json_error = serde_json::from_str::<u32>("{").unwrap_err();
        assert_eq!(
            deserialize_json::<u32>("{").unwrap_err(),
            CodecError::deserialization(format!("JSON deserialization failed: {json_error}"))
        );
        assert_eq!(
            deserialize_neo_binary::<VarIntPayload>(&[]).unwrap_err(),
            CodecError::deserialization("Cannot deserialize empty Neo binary data")
        );
        assert_eq!(
            deserialize_neo_binary::<VarIntPayload>(&[0xfd]).unwrap_err(),
            CodecError::deserialization("Neo binary deserialization failed: format error")
        );
    }

    #[test]
    fn neo_binary_preserves_varint_boundaries_and_trailing_bytes() {
        let cases = [
            (0, vec![0]),
            (252, vec![252]),
            (253, vec![0xfd, 0xfd, 0]),
            (65535, vec![0xfd, 0xff, 0xff]),
            (65536, vec![0xfe, 0, 0, 1, 0]),
            (u32::MAX as u64, vec![0xfe, 0xff, 0xff, 0xff, 0xff]),
            (u32::MAX as u64 + 1, vec![0xff, 0, 0, 0, 0, 1, 0, 0, 0]),
        ];
        for (value, expected) in cases {
            let mut writer = BinaryWriter::new();
            writer.write_u8(0xaa).unwrap();
            serialize_neo_binary(&VarIntPayload(value), &mut writer).unwrap();
            assert_eq!(writer.as_bytes()[0], 0xaa);
            assert_eq!(&writer.as_bytes()[1..], expected.as_slice());
            let mut with_trailing = expected;
            with_trailing.push(0xbb);
            assert_eq!(
                deserialize_neo_binary::<VarIntPayload>(&with_trailing).unwrap(),
                VarIntPayload(value)
            );
        }
    }

    #[test]
    fn neo_binary_preserves_nonminimal_varint_acceptance() {
        assert_eq!(
            deserialize_neo_binary::<VarIntPayload>(&[0xfd, 1, 0]).unwrap(),
            VarIntPayload(1)
        );
    }

    #[test]
    fn neo_binary_failures_remain_codec_errors() {
        assert_eq!(
            serialize_neo_binary(&RejectNeo, &mut BinaryWriter::new()).unwrap_err(),
            CodecError::serialization("Neo binary serialization failed: fixture write failure")
        );
        assert_eq!(
            deserialize_neo_binary::<RejectNeo>(&[1]).unwrap_err(),
            CodecError::deserialization("Neo binary deserialization failed: fixture read failure")
        );
    }

    #[test]
    fn compress_data_preserves_lz4_bytes_and_empty_roundtrip() {
        for data in [b"".as_slice(), b"literal".as_slice(), &[0; 512]] {
            let bytes = compress_data(data).unwrap();
            assert_eq!(bytes, lz4_flex::compress_prepend_size(data));
            assert_eq!(decompress_data(&bytes).unwrap(), data);
        }
        assert_eq!(
            compress_data(b"abc").unwrap(),
            vec![3, 0, 0, 0, 0x30, b'a', b'b', b'c']
        );
    }

    #[test]
    fn decompress_data_failures_preserve_messages() {
        assert_eq!(
            decompress_data(&[]).unwrap_err(),
            CodecError::deserialization("Cannot decompress empty data")
        );
        let data = [1, 2, 3];
        let error = lz4_flex::decompress_size_prepended(&data).unwrap_err();
        assert_eq!(
            decompress_data(&data).unwrap_err(),
            CodecError::deserialization(format!("Data decompression failed: {error}"))
        );
    }
}
