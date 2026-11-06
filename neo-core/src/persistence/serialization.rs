//! Serialization functionality for persistence layer.
//!
//! This module provides data serialization and deserialization capabilities
//! that match the C# Neo implementation exactly.

use super::compression::codec_error_to_core;
use crate::neo_io::{BinaryWriter, Serializable};
use neo_io_crate::serialization as codec;
use serde::{Deserialize, Serialize};

/// Serializes data to binary format (production implementation matching C# Neo exactly)
pub fn serialize<T: Serialize>(data: &T) -> crate::Result<Vec<u8>> {
    codec::serialize(data).map_err(codec_error_to_core)
}

/// Deserializes data from binary format (production implementation matching C# Neo exactly)
pub fn deserialize<T: for<'de> Deserialize<'de>>(data: &[u8]) -> crate::Result<T> {
    codec::deserialize(data).map_err(codec_error_to_core)
}

/// Serializes data to JSON format (production implementation matching C# Neo exactly)
pub fn serialize_json<T: Serialize>(data: &T) -> crate::Result<String> {
    codec::serialize_json(data).map_err(codec_error_to_core)
}

/// Deserializes data from JSON format (production implementation matching C# Neo exactly)
pub fn deserialize_json<T: for<'de> Deserialize<'de>>(data: &str) -> crate::Result<T> {
    codec::deserialize_json(data).map_err(codec_error_to_core)
}

/// Serializes data using Neo's native binary format (matches C# ISerializable exactly)
pub fn serialize_neo_binary<T>(data: &T, writer: &mut BinaryWriter) -> crate::Result<()>
where
    T: Serializable,
{
    codec::serialize_neo_binary(data, writer).map_err(codec_error_to_core)
}

/// Deserializes data using Neo's native binary format (matches C# ISerializable exactly)
pub fn deserialize_neo_binary<T>(data: &[u8]) -> crate::Result<T>
where
    T: Serializable + Default,
{
    codec::deserialize_neo_binary(data).map_err(codec_error_to_core)
}

/// Gets the serialized size of data without actually serializing it (production implementation)
pub fn estimate_serialized_size<T: Serialize>(data: &T) -> crate::Result<usize> {
    codec::estimate_serialized_size(data).map_err(codec_error_to_core)
}

/// Validates that data can be serialized and deserialized correctly (production implementation)
pub fn validate_serialization<T: Serialize + for<'de> Deserialize<'de> + PartialEq>(
    data: &T,
) -> crate::Result<bool> {
    codec::validate_serialization(data).map_err(codec_error_to_core)
}

/// Compresses serialized data using LZ4 (production implementation matching C# Neo compression)
pub fn compress_data(data: &[u8]) -> crate::Result<Vec<u8>> {
    codec::compress_data(data).map_err(codec_error_to_core)
}

/// Decompresses data using LZ4 (production implementation matching C# Neo compression)
pub fn decompress_data(compressed_data: &[u8]) -> crate::Result<Vec<u8>> {
    codec::decompress_data(compressed_data).map_err(codec_error_to_core)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::CoreError;
    use crate::neo_io::{IoError, IoResult, MemoryReader};
    use neo_primitives::UInt160;

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
        fn deserialize(_reader: &mut MemoryReader) -> IoResult<Self> {
            Err(IoError::Io(std::io::Error::other("fixture read failure")))
        }

        fn serialize(&self, _writer: &mut BinaryWriter) -> IoResult<()> {
            Err(IoError::Io(std::io::Error::other("fixture write failure")))
        }

        fn size(&self) -> usize {
            0
        }
    }

    #[test]
    fn legacy_serialization_success_values_match_io() {
        let data = vec![1i32, 2, 3];
        let bytes: crate::Result<Vec<u8>> = serialize(&data);
        let bytes = bytes.unwrap();
        assert_eq!(bytes, codec::serialize(&data).unwrap());
        assert_eq!(deserialize::<Vec<i32>>(&bytes).unwrap(), data);
        let json: crate::Result<String> = serialize_json(&data);
        let json = json.unwrap();
        assert_eq!(json, codec::serialize_json(&data).unwrap());
        assert_eq!(deserialize_json::<Vec<i32>>(&json).unwrap(), data);
        assert_eq!(estimate_serialized_size(&data).unwrap(), bytes.len());
        assert!(validate_serialization(&data).unwrap());
        let compressed: crate::Result<Vec<u8>> = compress_data(&bytes);
        let compressed = compressed.unwrap();
        assert_eq!(compressed, codec::compress_data(&bytes).unwrap());
        assert_eq!(decompress_data(&compressed).unwrap(), bytes);
        let value = UInt160::zero();
        let mut writer = BinaryWriter::new();
        let result: crate::Result<()> = serialize_neo_binary(&value, &mut writer);
        result.unwrap();
        assert_eq!(writer.as_bytes(), &[0; 20]);
        assert_eq!(
            deserialize_neo_binary::<UInt160>(writer.as_bytes()).unwrap(),
            value
        );
    }

    #[test]
    fn legacy_empty_errors_keep_core_codec_messages() {
        for (error, message) in [
            (
                deserialize::<u32>(&[]).unwrap_err(),
                "Cannot deserialize empty data",
            ),
            (
                deserialize_json::<u32>(" \n\t ").unwrap_err(),
                "Cannot deserialize empty JSON",
            ),
            (
                deserialize_neo_binary::<UInt160>(&[]).unwrap_err(),
                "Cannot deserialize empty Neo binary data",
            ),
            (
                decompress_data(&[]).unwrap_err(),
                "Cannot decompress empty data",
            ),
            (
                validate_serialization(&()).unwrap_err(),
                "Cannot deserialize empty data",
            ),
        ] {
            assert_eq!(error, CoreError::deserialization(message));
            assert_eq!(error.to_string(), format!("Codec error: {message}"));
            assert_eq!(error.category(), "serialization");
            assert!(!error.is_retryable());
        }
    }

    #[test]
    fn legacy_malformed_errors_keep_core_codec_messages() {
        for (error, expected) in [
            (
                deserialize::<u32>(&[1]).unwrap_err(),
                codec::deserialize::<u32>(&[1]).unwrap_err().to_string(),
            ),
            (
                deserialize_json::<u32>("{").unwrap_err(),
                codec::deserialize_json::<u32>("{").unwrap_err().to_string(),
            ),
            (
                deserialize_neo_binary::<UInt160>(&[1]).unwrap_err(),
                "Neo binary deserialization failed: format error".to_string(),
            ),
            (
                decompress_data(&[1, 2, 3]).unwrap_err(),
                codec::decompress_data(&[1, 2, 3]).unwrap_err().to_string(),
            ),
        ] {
            assert_eq!(error, CoreError::deserialization(&expected));
            assert_eq!(error.to_string(), format!("Codec error: {expected}"));
            assert!(!error.is_retryable());
        }
    }

    #[test]
    fn legacy_encoding_errors_keep_core_codec_messages() {
        for (error, message) in [
            (
                serialize(&RejectSerialize).unwrap_err(),
                "Binary serialization failed: fixture encode failure",
            ),
            (
                serialize_json(&RejectSerialize).unwrap_err(),
                "JSON serialization failed: fixture encode failure",
            ),
            (
                estimate_serialized_size(&RejectSerialize).unwrap_err(),
                "Size estimation failed: fixture encode failure",
            ),
        ] {
            assert_eq!(error, CoreError::serialization(message));
            assert_eq!(error.to_string(), format!("Codec error: {message}"));
            assert_eq!(error.category(), "serialization");
            assert!(!error.is_retryable());
        }
    }

    #[test]
    fn legacy_neo_io_failures_remain_nonretryable_codec_errors() {
        let write_error = serialize_neo_binary(&RejectNeo, &mut BinaryWriter::new()).unwrap_err();
        assert_eq!(
            write_error,
            CoreError::serialization("Neo binary serialization failed: fixture write failure")
        );
        assert!(!write_error.is_retryable());
        let read_error = deserialize_neo_binary::<RejectNeo>(&[1]).unwrap_err();
        assert_eq!(
            read_error,
            CoreError::deserialization("Neo binary deserialization failed: fixture read failure")
        );
        assert!(!read_error.is_retryable());
    }
}
