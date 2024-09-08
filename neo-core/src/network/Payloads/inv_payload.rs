use neo_io::*;
use std::io::{self, Write};
use std::collections::VecDeque;
use crate::uint256::UInt256;

/// This message is sent to relay inventories.
#[derive(Debug, Clone)]
pub struct InvPayload {
    /// The type of the inventories.
    pub inv_type: InventoryType,

    /// The hashes of the inventories.
    pub hashes: Vec<UInt256>,
}

impl InvPayload {
    /// Indicates the maximum number of inventories sent each time.
    pub const MAX_HASHES_COUNT: usize = 500;

    /// Creates a new instance of the InvPayload struct.
    pub fn new(inv_type: InventoryType, hashes: Vec<UInt256>) -> Self {
        Self { inv_type, hashes }
    }

    /// Creates a group of InvPayload instances.
    pub fn create_group(inv_type: InventoryType, hashes: Vec<UInt256>) -> VecDeque<Self> {
        hashes.chunks(Self::MAX_HASHES_COUNT)
            .map(|chunk| Self::new(inv_type, chunk.to_vec()))
            .collect()
    }
}

impl ISerializable for InvPayload {
    fn serialize(&self, writer: &mut dyn Write) -> io::Result<()> {
        writer.write_u8(self.inv_type as u8)?;
        writer.write_var_vec(&self.hashes)?;
        Ok(())
    }

    fn deserialize(reader: &mut dyn io::Read) -> io::Result<Self> {
        let inv_type = InventoryType::try_from(reader.read_u8()?).map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "Invalid InventoryType"))?;
        let hashes = reader.read_var_vec(Self::MAX_HASHES_COUNT)?;
        Ok(Self::new(inv_type, hashes))
    }

    fn size(&self) -> usize {
        std::mem::size_of::<InventoryType>() + var_vec_size(&self.hashes)
    }
}