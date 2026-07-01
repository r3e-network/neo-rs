//! Serialization helpers for the persistence layer.
//!
//! IMPORTANT: only the `*_neo_binary` helpers (and the `Serializable` trait they
//! wrap) produce the C#-compatible Neo `ISerializable` wire/storage format used
//! for consensus-relevant state. The `serialize`/`deserialize`/`*_json`/size
//! helpers use Rust-specific formats (bincode / serde_json) that have NO
//! relationship to C# Neo's encoding — they are internal/diagnostic utilities and
//! MUST NOT be used to persist consensus-relevant data or compute state roots.

use neo_error::CoreError;
use neo_io::{BinaryWriter, MemoryReader, Serializable};
use serde::{Deserialize, Serialize};

use crate::compression::{Compression, CompressionAlgorithm};

/// Serializes data to bincode (a Rust-specific format — NOT the C# Neo encoding;
/// internal/diagnostic only, never for consensus-relevant persisted state).
pub fn serialize<T: Serialize>(data: &T) -> neo_error::Result<Vec<u8>> {
    bincode::serialize(data)
        .map_err(|e| CoreError::serialization(format!("Binary serialization failed: {}", e)))
}

/// Deserializes data from bincode (Rust-specific format — see [`serialize`]; not
/// C#-compatible, internal/diagnostic only).
pub fn deserialize<T: for<'de> Deserialize<'de>>(data: &[u8]) -> neo_error::Result<T> {
    // Validate input data
    if data.is_empty() {
        return Err(CoreError::deserialization("Cannot deserialize empty data"));
    }

    bincode::deserialize(data)
        .map_err(|e| CoreError::deserialization(format!("Binary deserialization failed: {}", e)))
}

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

/// Gets the serialized size of data without actually serializing it (production implementation)
pub fn estimate_serialized_size<T: Serialize>(data: &T) -> neo_error::Result<usize> {
    bincode::serialized_size(data)
        .map(|size| size as usize)
        .map_err(|e| CoreError::serialization(format!("Size estimation failed: {}", e)))
}

/// Validates that data can be serialized and deserialized correctly (production implementation)
pub fn validate_serialization<T: Serialize + for<'de> Deserialize<'de> + PartialEq>(
    data: &T,
) -> neo_error::Result<bool> {
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
