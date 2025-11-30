use neo_core::neo_io::{
    helper::get_var_size, BinaryWriter, IoError, IoResult, MemoryReader, Serializable,
};
use uuid::Uuid;

const GUID_SIZE: usize = 16;

/// Stores the identifiers of engine log entries associated with a transaction.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TransactionEngineLogState {
    pub log_ids: Vec<Uuid>,
}

impl TransactionEngineLogState {
    pub fn create(log_ids: Vec<Uuid>) -> Self {
        Self { log_ids }
    }
}

impl Serializable for TransactionEngineLogState {
    fn deserialize(reader: &mut MemoryReader) -> IoResult<Self> {
        let count = reader.read_u32()? as usize;
        let mut log_ids = Vec::with_capacity(count);
        for _ in 0..count {
            let bytes = reader.read_var_bytes(GUID_SIZE)?;
            if bytes.len() != GUID_SIZE {
                return Err(IoError::invalid_data("Invalid GUID length"));
            }
            let mut data = [0u8; GUID_SIZE];
            data.copy_from_slice(&bytes);
            log_ids.push(Uuid::from_bytes_le(data));
        }
        Ok(Self { log_ids })
    }

    fn serialize(&self, writer: &mut BinaryWriter) -> IoResult<()> {
        writer.write_u32(self.log_ids.len() as u32)?;
        for id in &self.log_ids {
            writer.write_var_bytes(&id.to_bytes_le())?;
        }
        Ok(())
    }

    fn size(&self) -> usize {
        4 + self.log_ids.len() * (get_var_size(GUID_SIZE as u64) + GUID_SIZE)
    }
}
