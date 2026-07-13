//! Serialization helpers for the persistence layer.
//!
//! IMPORTANT: only the `*_neo_binary` helpers (and the `Serializable` trait they
//! wrap) produce the C#-compatible Neo `ISerializable` wire/storage format used
//! for consensus-relevant state. The `*_json` helpers are diagnostic utilities
//! and MUST NOT be used to persist consensus-relevant data or compute state roots.

use neo_error::CoreError;
use neo_io::{BinaryWriter, MemoryReader, Serializable};
use serde::{Deserialize, Serialize};

use crate::compression::{Compression, CompressionAlgorithm};

/// Serializes data to JSON format (production implementation matching C# Neo exactly)
pub fn serialize_json<T: Serialize>(data: &T) -> neo_error::Result<String> {
    serde_json::to_string(data)
        .map_err(|e| CoreError::serialization(format!("JSON serialization failed: {}", e)))
}

/// Deserializes data from JSON format (production implementation matching C# Neo exactly)
pub fn deserialize_json<T: for<'de> Deserialize<'de>>(data: &str) -> neo_error::Result<T> {
    // Validate input JSON
    if data.trim().is_empty() {
        return Err(CoreError::deserialization("Cannot deserialize empty JSON"));
    }

    serde_json::from_str(data)
        .map_err(|e| CoreError::deserialization(format!("JSON deserialization failed: {}", e)))
}

/// Serializes data using Neo's native binary format (matches C# ISerializable exactly)
pub fn serialize_neo_binary<T>(data: &T, writer: &mut BinaryWriter) -> neo_error::Result<()>
where
    T: Serializable,
{
    Serializable::serialize(data, writer)
        .map_err(|e| CoreError::serialization(format!("Neo binary serialization failed: {}", e)))
}

/// Deserializes data using Neo's native binary format (matches C# ISerializable exactly)
pub fn deserialize_neo_binary<T>(data: &[u8]) -> neo_error::Result<T>
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

/// Compresses serialized data using the canonical Neo LZ4 helper.
pub fn compress_data(data: &[u8]) -> neo_error::Result<Vec<u8>> {
    Compression::compress(data, CompressionAlgorithm::Lz4)
}

/// Decompresses serialized data using the canonical Neo LZ4 helper.
pub fn decompress_data(compressed_data: &[u8]) -> neo_error::Result<Vec<u8>> {
    // Validate input
    if compressed_data.is_empty() {
        return Err(CoreError::deserialization("Cannot decompress empty data"));
    }

    Compression::decompress(compressed_data, CompressionAlgorithm::Lz4)
        .map_err(|e| CoreError::deserialization(format!("Data decompression failed: {e}")))
}

#[cfg(test)]
#[path = "../tests/codec/serialization.rs"]
mod tests;
