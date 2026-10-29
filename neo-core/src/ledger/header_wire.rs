//! Unsigned Neo N3 block-header fields, in wire order.
//!
//! `BlockHeader` (ledger) and `Header` (P2P payload) are different types: one
//! stores a witness list, the other a single witness. The bytes before the
//! witness are the block hash preimage and must stay identical. This module is
//! that layout. Witness framing stays with each type.

use crate::neo_io::{BinaryWriter, IoResult, MemoryReader, Serializable};
use crate::{UInt160, UInt256};

/// The unsigned header. Field order is the Neo N3 wire order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct UnsignedHeader {
    pub version: u32,
    pub previous_hash: UInt256,
    pub merkle_root: UInt256,
    pub timestamp: u64,
    pub nonce: u64,
    pub index: u32,
    pub primary_index: u8,
    pub next_consensus: UInt160,
}

impl UnsignedHeader {
    /// Byte length of `write`. Field widths are the Neo N3 wire widths.
    pub(crate) const WIRE_SIZE: usize =
        4 + UInt256::LENGTH + UInt256::LENGTH + 8 + 8 + 4 + 1 + UInt160::LENGTH;

    pub(crate) fn write(&self, writer: &mut BinaryWriter) -> IoResult<()> {
        writer.write_u32(self.version)?;
        writer.write_serializable(&self.previous_hash)?;
        writer.write_serializable(&self.merkle_root)?;
        writer.write_u64(self.timestamp)?;
        writer.write_u64(self.nonce)?;
        writer.write_u32(self.index)?;
        writer.write_u8(self.primary_index)?;
        writer.write_serializable(&self.next_consensus)?;
        Ok(())
    }

    pub(crate) fn read(reader: &mut MemoryReader) -> IoResult<Self> {
        Ok(Self {
            version: reader.read_u32()?,
            previous_hash: <UInt256 as Serializable>::deserialize(reader)?,
            merkle_root: <UInt256 as Serializable>::deserialize(reader)?,
            timestamp: reader.read_u64()?,
            nonce: reader.read_u64()?,
            index: reader.read_u32()?,
            primary_index: reader.read_u8()?,
            next_consensus: <UInt160 as Serializable>::deserialize(reader)?,
        })
    }
}
