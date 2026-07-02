//! Serializable implementations for neo-primitives types.
//!
//! This module provides `Serializable` implementations for primitive types
//! from the neo-primitives crate, keeping neo-primitives free of neo-io dependencies.

use crate::{BinaryWriter, IoResult, MemoryReader, Serializable};
use neo_primitives::{UInt160, UInt256};

/// Size of `UInt160` in bytes
const UINT160_SIZE: usize = 20;

/// Size of `UInt256` in bytes
const UINT256_SIZE: usize = 32;

impl Serializable for UInt160 {
    fn deserialize(reader: &mut MemoryReader) -> IoResult<Self> {
        let bytes = reader.read_bytes(UINT160_SIZE)?;
        Self::from_bytes(&bytes)
            .map_err(|e| crate::IoError::invalid_data(format!("Invalid UInt160: {e}")))
    }

    fn serialize(&self, writer: &mut BinaryWriter) -> IoResult<()> {
        writer.write_bytes(&self.to_bytes())
    }

    fn size(&self) -> usize {
        UINT160_SIZE
    }
}

impl Serializable for UInt256 {
    fn deserialize(reader: &mut MemoryReader) -> IoResult<Self> {
        let bytes = reader.read_bytes(UINT256_SIZE)?;
        Self::from_bytes(&bytes)
            .map_err(|e| crate::IoError::invalid_data(format!("Invalid UInt256: {e}")))
    }

    fn serialize(&self, writer: &mut BinaryWriter) -> IoResult<()> {
        writer.write_bytes(&self.to_bytes())
    }

    fn size(&self) -> usize {
        UINT256_SIZE
    }
}

#[cfg(test)]
#[path = "../tests/serializable/primitives.rs"]
mod tests;
