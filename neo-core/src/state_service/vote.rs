//! Vote message for StateService signature aggregation.
//!
//! Matches C# Neo.Plugins.StateService.Network.Vote exactly.

use crate::macros::ValidateLength;
use crate::neo_io::serializable::helper::get_var_size;
use crate::neo_io::{BinaryWriter, IoError, IoResult, MemoryReader, Serializable};

const MAX_SIGNATURE_LENGTH: usize = 64;

/// Represents a state service vote message (validator signature over a state root).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Vote {
    pub validator_index: i32,
    pub root_index: u32,
    pub signature: Vec<u8>,
}

impl Serializable for Vote {
    fn size(&self) -> usize {
        4 + 4 + get_var_size(self.signature.len() as u64) + self.signature.len()
    }

    fn serialize(&self, writer: &mut BinaryWriter) -> IoResult<()> {
        self.signature
            .validate_max_length(MAX_SIGNATURE_LENGTH, "Signature")?;
        writer.write_i32(self.validator_index)?;
        writer.write_u32(self.root_index)?;
        writer.write_var_bytes(&self.signature)?;
        Ok(())
    }

    fn deserialize(reader: &mut MemoryReader) -> IoResult<Self> {
        let validator_index = reader.read_i32()?;
        let root_index = reader.read_u32()?;
        let signature = reader.read_var_bytes(MAX_SIGNATURE_LENGTH)?;
        if signature.is_empty() {
            return Err(IoError::invalid_data("Vote signature cannot be empty"));
        }
        Ok(Self {
            validator_index,
            root_index,
            signature,
        })
    }
}

