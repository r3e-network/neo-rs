use neo_io::{BinaryWriter, IoResult, MemoryReader, Serializable};
use serde::{Deserialize, Serialize};

/// Indicates that the transaction is of high priority.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HighPriorityAttribute;

impl HighPriorityAttribute {
    /// Creates a new high priority attribute.
    pub fn new() -> Self {
        Self
    }

    // verify: handled by TransactionAttribute dispatch.

    /// Serialize without type byte.
    pub fn serialize_without_type(&self, writer: &mut BinaryWriter) -> IoResult<()> {
        <Self as Serializable>::serialize(self, writer)
    }
}

// Use macro to reduce boilerplate
neo_io::impl_default_via_new!(HighPriorityAttribute);

impl Serializable for HighPriorityAttribute {
    fn size(&self) -> usize {
        0 // No additional data
    }

    fn serialize(&self, _writer: &mut BinaryWriter) -> IoResult<()> {
        Ok(()) // No data to serialize
    }

    fn deserialize(_reader: &mut MemoryReader) -> IoResult<Self> {
        Ok(Self) // No data to deserialize
    }
}
