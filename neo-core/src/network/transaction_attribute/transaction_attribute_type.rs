use neo_base::encoding::bin::*;
use serde::{Deserialize, Serialize};

/// Represents the type of a transaction_attribute.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, BinEncode, BinDecode)]
#[repr(u8)]
pub enum TransactionAttributeType {
    /// Indicates that the transaction is of high priority.
    #[reflection_cache(type = "HighPriorityAttribute")]
    HighPriority = 0x01,

    /// Indicates that the transaction is an oracle response.
    #[reflection_cache(type = "OracleResponse")]
    OracleResponse = 0x11,

    /// Indicates that the transaction is not valid before NotValidBefore.Height.
    #[reflection_cache(type = "NotValidBefore")]
    NotValidBefore = 0x20,

    /// Indicates that the transaction conflicts with Conflicts.Hash.
    #[reflection_cache(type = "Conflicts")]
    Conflicts = 0x21,
}



impl TransactionAttributeType {
    pub fn allow_multiple(self) -> bool { self == Self::Conflicts }
}