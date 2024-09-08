use crate::network::payloads::transaction_attribute::TransactionAttribute;
use crate::network::payloads::transaction_attribute_type::TransactionAttributeType;
use crate::io::MemoryReader;
use crate::json::JObject;
use crate::persistence::DataCache;
use crate::smart_contract::native::NativeContract;
use crate::network::payloads::transaction::Transaction;

pub struct NotValidBefore {
    /// Indicates that the transaction is not valid before this height.
    pub height: u32,
}

impl TransactionAttribute for NotValidBefore {
    fn type_(&self) -> TransactionAttributeType {
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

    fn serialize_without_type(&self, writer: &mut dyn std::io::Write) {
        writer.write_all(&self.height.to_le_bytes()).unwrap();
    }

    fn to_json(&self) -> JObject {
        let mut json = self.base_to_json();
        json.insert("height".to_string(), self.height.into());
        json
    }

    fn verify(&self, snapshot: &DataCache, _tx: &Transaction) -> bool {
        let block_height = NativeContract::Ledger.current_index(snapshot);
        block_height >= self.height
    }
}
