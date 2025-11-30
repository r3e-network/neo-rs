use neo_core::neo_io::{helper::get_var_size, BinaryWriter, IoResult, MemoryReader, Serializable};
use neo_core::UInt160;

/// Persistent representation of a VM log entry emitted during execution.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EngineLogState {
    pub script_hash: UInt160,
    pub message: String,
}

impl EngineLogState {
    pub fn create(script_hash: UInt160, message: impl Into<String>) -> Self {
        Self {
            script_hash,
            message: message.into(),
        }
    }
}

impl Serializable for EngineLogState {
    fn deserialize(reader: &mut MemoryReader) -> IoResult<Self> {
        let script_hash = UInt160::deserialize(reader)?;
        let message = reader.read_var_string(usize::MAX)?;
        Ok(Self {
            script_hash,
            message,
        })
    }

    fn serialize(&self, writer: &mut BinaryWriter) -> IoResult<()> {
        self.script_hash.serialize(writer)?;
        writer.write_var_string(&self.message)?;
        Ok(())
    }

    fn size(&self) -> usize {
        self.script_hash.size() + get_var_size(self.message.len() as u64) + self.message.len()
    }
}
