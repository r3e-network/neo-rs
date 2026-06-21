//! Transaction containment status identifiers.
//!
//! Mirrors `Neo.Ledger.ContainsTransactionType`.

use crate::protocol_enum;

protocol_enum! {
    /// Represents the type of transaction containment.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub ContainsTransactionType {
        /// Transaction does not exist.
        NotExist = 0,
        /// Transaction exists in the memory pool.
        ExistsInPool = 1,
        /// Transaction exists in the ledger.
        ExistsInLedger = 2,
    }
}

impl ContainsTransactionType {
    /// Returns true if the transaction exists either in the pool or ledger.
    #[must_use]
    pub const fn exists(self) -> bool {
        !matches!(self, Self::NotExist)
    }
}

#[cfg(test)]
#[path = "tests/contains_transaction_type.rs"]
mod tests;
