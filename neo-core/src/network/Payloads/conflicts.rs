use std::io::Write;
use crate::io::memory_reader::MemoryReader;
use crate::network::Payloads::{Transaction, TransactionAttribute, TransactionAttributeType};
use crate::persistence::DataCache;
use crate::uint256::UInt256;

pub struct Conflicts {
    /// Indicates the conflict transaction hash.
    pub hash: UInt256,
}

impl TransactionAttribute for Conflicts {
    fn attribute_type(&self) -> TransactionAttributeType {
        TransactionAttributeType::Conflicts
    }

    fn allow_multiple(&self) -> bool {
        true
    }

    fn size(&self) -> usize {
        self.base_size() + self.hash.size()
    }

    fn deserialize_without_type(&mut self, reader: &mut MemoryReader) {
        self.hash = UInt256::deserialize(reader).unwrap();
    }

    fn serialize_without_type<W: Write>(&self, writer: &mut W) -> std::io::Result<()> {
        self.hash.serialize(writer)
    }

    fn to_json(&self) -> JObject {
        let mut json = self.base_to_json();
        json.insert("hash", self.hash.to_string());
        json
    }

    fn verify(&self, snapshot: &DataCache, tx: &Transaction) -> bool {
        // Only check if conflicting transaction is on chain. It's OK if the
        // conflicting transaction was in the Conflicts attribute of some other
        // on-chain transaction.
        !NativeContract::Ledger.contains_transaction(snapshot, &self.hash)
    }

    fn calculate_network_fee(&self, snapshot: &DataCache, tx: &Transaction) -> i64 {
        tx.signers.len() as i64 * self.base_calculate_network_fee(snapshot, tx)
    }
}
