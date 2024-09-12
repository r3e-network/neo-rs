use neo_io::{MemoryReader, Serializable};
use neo_types::UInt256;
use std::io::{self, Write};
use crate::io::binary_writer::BinaryWriter;
use crate::io::iserializable::ISerializable;
use crate::io::memory_reader::MemoryReader;
use crate::uint256::UInt256;

/// This message is sent to request for blocks by hash.
pub struct GetBlocksPayload {
    /// The starting hash of the blocks to request.
    pub hash_start: UInt256,

    /// The number of blocks to request.
    pub count: i16,
}

impl GetBlocksPayload {

    /// Creates a new instance of the GetBlocksPayload struct.
    ///
    /// # Arguments
    ///
    /// * `hash_start` - The starting hash of the blocks to request.
    /// * `count` - The number of blocks to request. Set this parameter to -1 to request as many blocks as possible.
    ///
    /// # Returns
    ///
    /// The created payload.
    pub fn create(hash_start: UInt256, count: i16) -> Self {
        Self {
            hash_start,
            count,
        }
    }
}

impl ISerializable for GetBlocksPayload {
    fn size(&self) -> usize {
        self.hash_start.size() + std::mem::size_of::<i16>()
    }

    fn serialize(&self, writer: &mut BinaryWriter) {
        self.hash_start.serialize(writer)?;
        writer.write_all(&self.count.to_le_bytes())?;
    }

    fn deserialize(reader: &mut MemoryReader) -> Result<Self, std::io::Error> {
        let hash_start = UInt256::deserialize(reader)?;
        let count = reader.read_i16()?;
        if count < -1 || count == 0 {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "Invalid count"));
        }
        Ok(Self { hash_start, count })
    }
}
