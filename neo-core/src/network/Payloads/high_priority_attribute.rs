
use std::io::Write;
use crate::io::memory_reader::MemoryReader;
use crate::network::Payloads::{Transaction, TransactionAttribute, TransactionAttributeType};
use crate::persistence::DataCache;

/// Indicates that the transaction is of high priority.
pub struct HighPriorityAttribute;

impl TransactionAttribute for HighPriorityAttribute {
    fn allow_multiple(&self) -> bool {
        false
    }

    fn attribute_type(&self) -> TransactionAttributeType {
        TransactionAttributeType::HighPriority
    }

    fn deserialize_without_type(&mut self, _reader: &mut MemoryReader) {
        // Empty implementation
    }

    fn serialize_without_type<W: Write>(&self, _writer: &mut W) {
        // Empty implementation
    }

    fn verify(&self, snapshot: &dyn DataCache, tx: &Transaction) -> bool {
        let committee = NativeContract::NEO.get_committee_address(snapshot);
        tx.signers.iter().any(|p| p.account == committee)
    }
}
