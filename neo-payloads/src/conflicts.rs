use neo_io::{BinaryWriter, IoResult, Serializable, impl_serializable};
use neo_primitives::UInt256;
use serde::{Deserialize, Serialize};

/// Represents a conflicts transaction attribute.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Conflicts {
    /// Indicates the conflict transaction hash.
    pub hash: UInt256,
}

impl Conflicts {
    /// Creates a new conflicts attribute.
    pub fn new(hash: UInt256) -> Self {
        Self { hash }
    }

    // verify: handled by TransactionAttribute dispatch.

    /// Calculate network fee for this attribute.
    pub fn calculate_network_fee(
        &self,
        base_fee: i64,
        tx: &super::transaction::Transaction,
    ) -> i64 {
        tx.signers().len() as i64 * base_fee
    }

    /// Serialize without type byte.
    pub fn serialize_without_type(&self, writer: &mut BinaryWriter) -> IoResult<()> {
        <Self as Serializable>::serialize(self, writer)
    }
}

impl_serializable! {
    struct Conflicts {
        hash: UInt256,
    }
}
