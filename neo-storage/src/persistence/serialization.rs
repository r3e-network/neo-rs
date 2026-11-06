//! Storage-owned serialization facade.
//!
//! Thin wrappers over [`neo_io::serialization`] that surface codec failures as
//! [`StorageError`] instead of the raw [`CodecError`]. This makes neo-storage the
//! persistence authority for serialization: callers inside the storage crate (and
//! downstream crates that depend on it) get a single, storage-native error type
//! without reaching for neo-core's `CoreError`.
//!
//! The concrete binary/JSON/Neo-native codecs live in the `neo-io` crate and match
//! the C# Neo implementation exactly; this module only adapts the error channel.

use crate::error::{StorageError, StorageResult};
use neo_io::codec_error::CodecError;
use neo_io::serialization as codec;
use neo_io::{BinaryWriter, Serializable};
use serde::{Deserialize, Serialize};

/// Maps a codec failure onto the storage-native error type, preserving the
/// original operation-specific message.
///
/// `Serialization`, `Deserialization`, and `Compression` codec failures all map to
/// [`StorageError::Serialization`]; stream `Io` failures map to [`StorageError::Io`]
/// so their distinct retryability semantics are retained by callers.
pub fn codec_error_to_storage(error: CodecError) -> StorageError {
    match error {
        CodecError::Serialization { message } => StorageError::serialization(message),
        CodecError::Deserialization { message } => StorageError::serialization(message),
        CodecError::Compression { message } => StorageError::serialization(message),
        CodecError::Io { message } => StorageError::io(message),
    }
}

/// Serializes data to binary format (matches C# Neo exactly).
pub fn serialize<T: Serialize>(data: &T) -> StorageResult<Vec<u8>> {
    codec::serialize(data).map_err(codec_error_to_storage)
}

/// Deserializes data from binary format (matches C# Neo exactly).
pub fn deserialize<T: for<'de> Deserialize<'de>>(data: &[u8]) -> StorageResult<T> {
    codec::deserialize(data).map_err(codec_error_to_storage)
}

/// Serializes data to JSON format (matches C# Neo exactly).
pub fn serialize_json<T: Serialize>(data: &T) -> StorageResult<String> {
    codec::serialize_json(data).map_err(codec_error_to_storage)
}

/// Deserializes data from JSON format (matches C# Neo exactly).
pub fn deserialize_json<T: for<'de> Deserialize<'de>>(data: &str) -> StorageResult<T> {
    codec::deserialize_json(data).map_err(codec_error_to_storage)
}

/// Serializes data using Neo's native binary format (matches C# `ISerializable`).
pub fn serialize_neo_binary<T>(data: &T, writer: &mut BinaryWriter) -> StorageResult<()>
where
    T: Serializable,
{
    codec::serialize_neo_binary(data, writer).map_err(codec_error_to_storage)
}

/// Deserializes data using Neo's native binary format (matches C# `ISerializable`).
pub fn deserialize_neo_binary<T>(data: &[u8]) -> StorageResult<T>
where
    T: Serializable + Default,
{
    codec::deserialize_neo_binary(data).map_err(codec_error_to_storage)
}

/// Gets the serialized size of data without actually serializing it.
pub fn estimate_serialized_size<T: Serialize>(data: &T) -> StorageResult<usize> {
    codec::estimate_serialized_size(data).map_err(codec_error_to_storage)
}

/// Validates that data can be serialized and deserialized correctly.
pub fn validate_serialization<T: Serialize + for<'de> Deserialize<'de> + PartialEq>(
    data: &T,
) -> StorageResult<bool> {
    codec::validate_serialization(data).map_err(codec_error_to_storage)
}

/// Compresses serialized data using LZ4 (matches C# Neo compression).
pub fn compress_data(data: &[u8]) -> StorageResult<Vec<u8>> {
    codec::compress_data(data).map_err(codec_error_to_storage)
}

/// Decompresses data using LZ4 (matches C# Neo compression).
pub fn decompress_data(compressed_data: &[u8]) -> StorageResult<Vec<u8>> {
    codec::decompress_data(compressed_data).map_err(codec_error_to_storage)
}

#[cfg(test)]
mod tests {
    use super::*;
    use neo_io::{IoError, IoResult, MemoryReader};
    use neo_primitives::UInt160;

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
    fn round_trip_matches_codec_and_uses_storage_result() {
        let data = vec![1i32, 2, 3];
        let bytes: StorageResult<Vec<u8>> = serialize(&data);
        let bytes = bytes.unwrap();
        assert_eq!(bytes, codec::serialize(&data).unwrap());
        assert_eq!(deserialize::<Vec<i32>>(&bytes).unwrap(), data);

        let json = serialize_json(&data).unwrap();
        assert_eq!(json, codec::serialize_json(&data).unwrap());
        assert_eq!(deserialize_json::<Vec<i32>>(&json).unwrap(), data);

        assert_eq!(estimate_serialized_size(&data).unwrap(), bytes.len());
        assert!(validate_serialization(&data).unwrap());

        let compressed = compress_data(&bytes).unwrap();
        assert_eq!(compressed, codec::compress_data(&bytes).unwrap());
        assert_eq!(decompress_data(&compressed).unwrap(), bytes);

        let value = UInt160::zero();
        let mut writer = BinaryWriter::new();
        serialize_neo_binary(&value, &mut writer).unwrap();
        assert_eq!(writer.as_bytes(), &[0; 20]);
        assert_eq!(
            deserialize_neo_binary::<UInt160>(writer.as_bytes()).unwrap(),
            value
        );
    }

    #[test]
    fn codec_failures_surface_as_storage_serialization_errors() {
        // Empty-input failures keep the codec message but become StorageError.
        let error = deserialize::<u32>(&[]).unwrap_err();
        assert_eq!(
            error,
            StorageError::serialization("Cannot deserialize empty data")
        );

        // Neo-native I/O failures remain StorageError::Io (retryable channel).
        let write_error = serialize_neo_binary(&RejectNeo, &mut BinaryWriter::new()).unwrap_err();
        assert_eq!(
            write_error,
            StorageError::serialization("Neo binary serialization failed: fixture write failure")
        );
        let read_error = deserialize_neo_binary::<RejectNeo>(&[1]).unwrap_err();
        assert_eq!(
            read_error,
            StorageError::serialization("Neo binary deserialization failed: fixture read failure")
        );
    }

    #[test]
    fn codec_error_mapping_preserves_messages_and_io_channel() {
        assert_eq!(
            codec_error_to_storage(CodecError::serialization("encode")),
            StorageError::serialization("encode")
        );
        assert_eq!(
            codec_error_to_storage(CodecError::deserialization("decode")),
            StorageError::serialization("decode")
        );
        assert_eq!(
            codec_error_to_storage(CodecError::compression("lz4")),
            StorageError::serialization("lz4")
        );
        assert_eq!(
            codec_error_to_storage(CodecError::io("stream")),
            StorageError::io("stream")
        );
    }
}
