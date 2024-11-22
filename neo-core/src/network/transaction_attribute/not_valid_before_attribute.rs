use std::io::Error;
use crate::io::binary_writer::BinaryWriter;
use crate::io::serializable_trait::SerializableTrait;
use crate::io::memory_reader::MemoryReader;
use crate::persistence::DataCache;
use crate::network::payloads::{Transaction};
use crate::network::transaction_attribute::transaction_attribute::TransactionAttribute;
use crate::network::transaction_attribute::transaction_attribute_type::TransactionAttributeType;
use serde::{Serialize, Deserialize};

#[derive(Serialize, Deserialize)]
pub struct NotValidBefore {
    /// Indicates that the transaction is not valid before this height.
    pub height: u32,
}

impl SerializableTrait for NotValidBefore {
    fn size(&self) -> usize {
        std::mem::size_of::<u32>()
    }

    fn serialize(&self, writer: &mut BinaryWriter) {
        writer.write_u32(self.height);
    }

    fn deserialize(reader: &mut MemoryReader) -> Result<Self, Error> {
        let height = reader.read_u32()?;
        Ok(NotValidBefore { height })
    }
}

impl TransactionAttribute for NotValidBefore {
    fn get_type(&self) -> TransactionAttributeType {
        TransactionAttributeType::NotValidBefore
    }

    fn allow_multiple(&self) -> bool {
        false
    }

    fn size(&self) -> usize {
        self.base_size() + std::mem::size_of::<u32>() // Height
    }

    fn deserialize_without_type(&mut self, reader: &mut MemoryReader) {
        self.height = reader.read_u32();
    }

}

impl NotValidBefore{
    fn serialize_without_type(&self, writer: &mut dyn std::io::Write) {
        writer.write_all(&self.height.to_le_bytes()).unwrap();
    }

    fn verify(&self, snapshot: &dyn DataCache, _tx: &Transaction) -> bool {
        let block_height = NativeContract::Ledger.current_index(snapshot);
        block_height >= self.height
    }
}

impl JsonConvertibleTrait for NotValidBefore {
    fn from_json(json: &JToken) -> Option<Self> {
        serde_json::from_value(json.clone()).ok()
    }

    fn to_json(&self) -> serde_json::Value {
        serde_json::to_value(self).unwrap_or_default()
    }
}
