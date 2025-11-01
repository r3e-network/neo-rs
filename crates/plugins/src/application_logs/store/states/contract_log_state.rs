use super::notify_log_state::NotifyLogState;
use neo_core::neo_io::{BinaryWriter, IoError, IoResult, MemoryReader, Serializable};
use neo_core::smart_contract::TriggerType;
use neo_core::UInt256;

/// Notification metadata specialised with transaction context.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContractLogState {
    pub transaction_hash: UInt256,
    pub trigger: TriggerType,
    pub notify: NotifyLogState,
}

impl ContractLogState {
    pub fn create(
        transaction_hash: Option<UInt256>,
        trigger: TriggerType,
        notify: NotifyLogState,
    ) -> Self {
        Self {
            transaction_hash: transaction_hash.unwrap_or_default(),
            trigger,
            notify,
        }
    }
}

impl Serializable for ContractLogState {
    fn deserialize(reader: &mut MemoryReader) -> IoResult<Self> {
        let transaction_hash = UInt256::deserialize(reader)?;
        let trigger_byte = reader.read_u8()?;
        let trigger = TriggerType::from_bits(trigger_byte)
            .ok_or_else(|| IoError::invalid_data("Invalid trigger type"))?;
        let notify = NotifyLogState::deserialize(reader)?;
        Ok(Self {
            transaction_hash,
            trigger,
            notify,
        })
    }

    fn serialize(&self, writer: &mut BinaryWriter) -> IoResult<()> {
        self.transaction_hash.serialize(writer)?;
        writer.write_u8(self.trigger.bits())?;
        self.notify.serialize(writer)?;
        Ok(())
    }

    fn size(&self) -> usize {
        self.transaction_hash.size() + 1 + self.notify.size()
    }
}
