use neo_core::neo_io::{helper::get_var_size, BinaryWriter, IoError, IoResult, MemoryReader, Serializable};
use neo_core::UInt160;
use uuid::Uuid;

const GUID_SIZE: usize = 16;

/// Persistent contract notification payload metadata.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NotifyLogState {
    pub script_hash: UInt160,
    pub event_name: String,
    pub stack_item_ids: Vec<Uuid>,
}

impl NotifyLogState {
    pub fn create(script_hash: UInt160, event_name: impl Into<String>, stack_item_ids: Vec<Uuid>) -> Self {
        Self {
            script_hash,
            event_name: event_name.into(),
            stack_item_ids,
        }
    }
}

impl Serializable for NotifyLogState {
    fn deserialize(reader: &mut MemoryReader) -> IoResult<Self> {
        let script_hash = UInt160::deserialize(reader)?;
        let event_name = reader.read_var_string(usize::MAX)?;
        let count = reader.read_u32()? as usize;
        let mut stack_item_ids = Vec::with_capacity(count);
        for _ in 0..count {
            let bytes = reader.read_var_bytes(GUID_SIZE)?;
            if bytes.len() != GUID_SIZE {
                return Err(IoError::invalid_data("Invalid GUID length"));
            }
            let mut data = [0u8; GUID_SIZE];
            data.copy_from_slice(&bytes);
            stack_item_ids.push(Uuid::from_bytes_le(data));
        }

        Ok(Self {
            script_hash,
            event_name,
            stack_item_ids,
        })
    }

    fn serialize(&self, writer: &mut BinaryWriter) -> IoResult<()> {
        self.script_hash.serialize(writer)?;
        writer.write_var_string(&self.event_name)?;
        writer.write_u32(self.stack_item_ids.len() as u32)?;
        for id in &self.stack_item_ids {
            writer.write_var_bytes(&id.to_bytes_le())?;
        }
        Ok(())
    }

    fn size(&self) -> usize {
        self.script_hash.size()
            + get_var_size(self.event_name.as_bytes().len() as u64)
            + self.event_name.as_bytes().len()
            + 4
            + self.stack_item_ids.len() * (get_var_size(GUID_SIZE as u64) + GUID_SIZE)
    }
}
