use neo_core::neo_io::{
    helper::get_var_size, BinaryWriter, IoError, IoResult, MemoryReader, Serializable,
};
use neo_core::neo_ledger::ApplicationExecuted;
use neo_vm::VMState;
use uuid::Uuid;

const GUID_SIZE: usize = 16;

/// Captures execution metadata for transactions and system persistence steps.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExecutionLogState {
    pub vm_state: VMState,
    pub exception: String,
    pub gas_consumed: i64,
    pub stack_item_ids: Vec<Uuid>,
}

impl ExecutionLogState {
    pub fn create(app_execution: &ApplicationExecuted, stack_item_ids: Vec<Uuid>) -> Self {
        Self {
            vm_state: app_execution.vm_state,
            exception: app_execution.exception.clone().unwrap_or_default(),
            gas_consumed: app_execution.gas_consumed,
            stack_item_ids,
        }
    }
}

impl Serializable for ExecutionLogState {
    fn deserialize(reader: &mut MemoryReader) -> IoResult<Self> {
        let vm_state = match reader.read_u8()? {
            0 => VMState::NONE,
            1 => VMState::HALT,
            2 => VMState::FAULT,
            4 => VMState::BREAK,
            value => {
                return Err(IoError::invalid_data(format!(
                    "Invalid VMState value: {value}"
                )))
            }
        };

        let exception = reader.read_var_string(usize::MAX)?;
        let gas_consumed = reader.read_i64()?;
        let count = reader.read_u32()? as usize;
        let mut stack_item_ids = Vec::with_capacity(count);
        for _ in 0..count {
            let bytes = reader.read_var_bytes(GUID_SIZE)?;
            if bytes.len() != GUID_SIZE {
                return Err(IoError::invalid_data("Invalid GUID length"));
            }
            let mut buffer = [0u8; GUID_SIZE];
            buffer.copy_from_slice(&bytes);
            stack_item_ids.push(Uuid::from_bytes_le(buffer));
        }

        Ok(Self {
            vm_state,
            exception,
            gas_consumed,
            stack_item_ids,
        })
    }

    fn serialize(&self, writer: &mut BinaryWriter) -> IoResult<()> {
        writer.write_u8(self.vm_state as u8)?;
        writer.write_var_string(&self.exception)?;
        writer.write_i64(self.gas_consumed)?;
        writer.write_u32(self.stack_item_ids.len() as u32)?;
        for id in &self.stack_item_ids {
            writer.write_var_bytes(&id.to_bytes_le())?;
        }
        Ok(())
    }

    fn size(&self) -> usize {
        1 + get_var_size(self.exception.as_bytes().len() as u64)
            + self.exception.as_bytes().len()
            + 8
            + 4
            + self.stack_item_ids.len() * (get_var_size(GUID_SIZE as u64) + GUID_SIZE)
    }
}
