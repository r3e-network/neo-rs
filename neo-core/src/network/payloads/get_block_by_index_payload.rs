use neo_io::{BinaryReader, BinaryWriter};
use std::io;
use crate::io::binary_reader::BinaryReader;
use crate::io::binary_writer::BinaryWriter;
use crate::io::serializable_trait::SerializableTrait;
use crate::network::payloads::HeadersPayload;

/// This message is sent to request for blocks by index.
pub struct GetBlockByIndexPayload {
    /// The starting index of the blocks to request.
    pub index_start: u32,

    /// The number of blocks to request.
    pub count: i16,
}

impl GetBlockByIndexPayload {
    /// Creates a new instance of the `GetBlockByIndexPayload` struct.
    ///
    /// # Arguments
    ///
    /// * `index_start` - The starting index of the blocks to request.
    /// * `count` - The number of blocks to request. Set this parameter to -1 to request as many blocks as possible.
    ///
    /// # Returns
    ///
    /// The created payload.
    pub fn new(index_start: u32, count: i16) -> Self {
        Self {
            index_start,
            count,
        }
    }
}

impl SerializableTrait for GetBlockByIndexPayload {
    fn size(&self) -> usize {
        std::mem::size_of::<u32>() + std::mem::size_of::<i16>()
    }

    fn serialize(&self, writer: &mut BinaryWriter) {
        writer.write_u32(self.index_start);
        writer.write_i16(self.count);
    }

    fn deserialize(reader: &mut MemoryReader) -> Result<Self, std::io::Error> {
        let index_start = reader.read_u32()?;
        let count = reader.read_i16()?;

        if count < -1 || count == 0 || count > HeadersPayload::MAX_HEADERS_COUNT as i16 {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "Invalid count"));
        }

        Ok(Self {
            index_start,
            count,
        })
    }
}
