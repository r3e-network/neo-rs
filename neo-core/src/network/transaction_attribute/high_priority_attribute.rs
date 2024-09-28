
use std::io::{Error, Write};
use crate::io::binary_writer::BinaryWriter;
use crate::io::serializable_trait::SerializableTrait;
use crate::io::memory_reader::MemoryReader;
use crate::network::payloads::{Transaction};
use crate::network::transaction_attribute::transaction_attribute::TransactionAttribute;
use crate::network::transaction_attribute::transaction_attribute_type::TransactionAttributeType;
use crate::persistence::DataCache;

/// Indicates that the transaction is of high priority.
pub struct HighPriorityAttribute;

impl SerializableTrait for HighPriorityAttribute {
    fn size(&self) -> usize {
        todo!()
    }

    fn serialize(&self, writer: &mut BinaryWriter) {
        todo!()
    }

    fn deserialize(reader: &mut MemoryReader) -> Result<Self, Error> {
        todo!()
    }
}

impl TransactionAttribute for HighPriorityAttribute {
    fn get_type(&self) -> TransactionAttributeType {
        TransactionAttributeType::HighPriority
    }

    fn allow_multiple(&self) -> bool {
        false
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
