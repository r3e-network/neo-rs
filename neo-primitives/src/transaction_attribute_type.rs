//! `TransactionAttributeType` - matches C# Neo.Network.P2P.Payloads.TransactionAttributeType exactly.

use crate::protocol_enum;

protocol_enum! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    /// Represents the type of a `TransactionAttribute`.
    pub TransactionAttributeType {
        /// Marks a transaction as high priority.
        HighPriority = 0x01,
        /// Oracle response attribute.
        OracleResponse = 0x11,
        /// Not-valid-before block index attribute.
        NotValidBefore = 0x20,
        /// Transaction conflict declaration attribute.
        Conflicts = 0x21,
        /// Notary-assisted transaction attribute.
        NotaryAssisted = 0x22,
    }
}

impl TransactionAttributeType {
    /// Returns true if this attribute type allows multiple instances per transaction.
    #[must_use]
    pub const fn allows_multiple(self) -> bool {
        matches!(self, Self::Conflicts)
    }
}

#[cfg(test)]
#[path = "tests/transaction_attribute_type.rs"]
mod tests;
