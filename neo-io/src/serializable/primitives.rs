//! Serializable implementations for neo-primitives types.
//!
//! This module provides `Serializable` implementations for primitive types
//! from the neo-primitives crate, keeping neo-primitives free of neo-io dependencies.

use crate::{BinaryWriter, IoResult, MemoryReader, Serializable};
use neo_primitives::{UInt160, UInt256};

/// Size of UInt160 in bytes
const UINT160_SIZE: usize = 20;

/// Size of UInt256 in bytes
const UINT256_SIZE: usize = 32;

impl Serializable for UInt160 {
    fn size(&self) -> usize {
        UINT160_SIZE
    }

    fn serialize(&self, writer: &mut BinaryWriter) -> IoResult<()> {
        writer.write_bytes(&self.to_bytes())
    }

    fn deserialize(reader: &mut MemoryReader) -> IoResult<Self> {
        let bytes = reader.read_bytes(UINT160_SIZE)?;
        UInt160::from_bytes(&bytes)
            .map_err(|e| crate::IoError::invalid_data(format!("Invalid UInt160: {}", e)))
    }
}

impl Serializable for UInt256 {
    fn size(&self) -> usize {
        UINT256_SIZE
    }

    fn serialize(&self, writer: &mut BinaryWriter) -> IoResult<()> {
        writer.write_bytes(&self.to_bytes())
    }

    fn deserialize(reader: &mut MemoryReader) -> IoResult<Self> {
        let bytes = reader.read_bytes(UINT256_SIZE)?;
        UInt256::from_bytes(&bytes)
            .map_err(|e| crate::IoError::invalid_data(format!("Invalid UInt256: {}", e)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_uint160_serialization() {
        let original = UInt160::from([1u8; 20]);
        let mut writer = BinaryWriter::new();
        original.serialize(&mut writer).unwrap();

        let data = writer.to_bytes();
        assert_eq!(data.len(), UINT160_SIZE);

        let mut reader = MemoryReader::new(&data);
        let deserialized = UInt160::deserialize(&mut reader).unwrap();
        assert_eq!(original, deserialized);
    }

    #[test]
    fn test_uint256_serialization() {
        let original = UInt256::from([2u8; 32]);
        let mut writer = BinaryWriter::new();
        original.serialize(&mut writer).unwrap();

        let data = writer.to_bytes();
        assert_eq!(data.len(), UINT256_SIZE);

        let mut reader = MemoryReader::new(&data);
        let deserialized = UInt256::deserialize(&mut reader).unwrap();
        assert_eq!(original, deserialized);
    }

    #[test]
    fn test_uint160_zero() {
        let zero = UInt160::zero();
        let mut writer = BinaryWriter::new();
        zero.serialize(&mut writer).unwrap();

        let data = writer.to_bytes();
        let mut reader = MemoryReader::new(&data);
        let deserialized = UInt160::deserialize(&mut reader).unwrap();
        assert_eq!(zero, deserialized);
    }

    #[test]
    fn test_uint256_zero() {
        let zero = UInt256::zero();
        let mut writer = BinaryWriter::new();
        zero.serialize(&mut writer).unwrap();

        let data = writer.to_bytes();
        let mut reader = MemoryReader::new(&data);
        let deserialized = UInt256::deserialize(&mut reader).unwrap();
        assert_eq!(zero, deserialized);
    }
}
