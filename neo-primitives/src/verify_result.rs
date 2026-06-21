//! `VerifyResult` - matches C# Neo.Ledger.VerifyResult exactly.
//!
//! This is the single source of truth for `VerifyResult` enum. Both `neo-core::ledger`
//! and neo-p2p re-export this type for backward compatibility.

use crate::protocol_enum;

protocol_enum! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
    /// Represents a verifying result of `Inventory`.
    pub VerifyResult {
        /// Verification succeeded.
        Succeed = 0,
        /// The inventory already exists.
        AlreadyExists = 1,
        /// The inventory is already in the memory pool.
        AlreadyInPool = 2,
        /// Verification could not continue because memory was exhausted.
        OutOfMemory = 3,
        /// The inventory could not be verified.
        UnableToVerify = 4,
        /// The inventory is invalid.
        Invalid = 5,
        /// The script is invalid.
        InvalidScript = 6,
        /// A transaction attribute is invalid.
        InvalidAttribute = 7,
        /// A signature is invalid.
        InvalidSignature = 8,
        /// The inventory exceeds the allowed size.
        OverSize = 9,
        /// The inventory has expired.
        Expired = 10,
        /// The transaction's `ValidUntilBlock` is too far in the future (more
        /// than `MaxValidUntilBlockIncrement` ahead of the current height).
        /// C# v3.10.0 `VerifyResult.NotYetValid`.
        NotYetValid = 11,
        /// The sender has insufficient funds.
        InsufficientFunds = 12,
        /// Policy validation failed.
        PolicyFail = 13,
        /// The transaction conflicts with another transaction.
        HasConflicts = 14,
        #[default]
        /// The verification result is unknown.
        Unknown = 15,
    }
}

impl VerifyResult {
    /// Returns true if the verification was successful.
    #[inline]
    #[must_use]
    pub const fn is_success(self) -> bool {
        matches!(self, Self::Succeed)
    }

    /// Returns true if the result indicates a failure.
    #[inline]
    #[must_use]
    pub fn is_failure(self) -> bool {
        !self.is_success()
    }
}

#[cfg(test)]
#[path = "tests/verify_result.rs"]
mod tests;
