//! Vote message for StateService validation.
//!
//! Matches `Neo.Plugins.StateService.Network.Vote`.

use crate::neo_io::{BinaryWriter, IoResult, MemoryReader, Serializable};

/// Vote payload carrying a validator signature for a state root.
#[derive(Debug, Clone)]
pub struct Vote {
    /// Validator index in the designated validator list.
    pub validator_index: i32,
    /// State root index.
    pub root_index: u32,
    /// Signature over the state root hash (64 bytes).
    pub signature: Vec<u8>,
}

impl Serializable for Vote {
    fn size(&self) -> usize {
        std::mem::size_of::<i32>()
            + std::mem::size_of::<u32>()
            + crate::neo_io::serializable::helper::get_var_size(self.signature.len() as u64)
            + self.signature.len()
    }

    fn serialize(&self, writer: &mut BinaryWriter) -> IoResult<()> {
        writer.write_i32(self.validator_index)?;
        writer.write_u32(self.root_index)?;
        writer.write_var_bytes(&self.signature)?;
        Ok(())
    }

    fn deserialize(reader: &mut MemoryReader) -> IoResult<Self> {
        let validator_index = reader.read_i32()?;
        let root_index = reader.read_u32()?;
        let signature = reader.read_var_bytes(64)?;
        Ok(Self {
            validator_index,
            root_index,
            signature,
        })
    }
}
