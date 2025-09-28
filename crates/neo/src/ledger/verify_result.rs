//! Verify result implementation.
//!
//! This module provides the VerifyResult functionality exactly matching C# Neo VerifyResult.

// Matches C# using directives exactly:
// using Neo.Network.P2P.Payloads;

/// namespace Neo.Ledger -> public enum VerifyResult : byte

/// Represents a verifying result of IInventory.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerifyResult {
    /// Indicates that the verification was successful.
    Succeed = 0,

    /// Indicates that an IInventory with the same hash already exists.
    AlreadyExists,

    /// Indicates that an IInventory with the same hash already exists in the memory pool.
    AlreadyInPool,

    /// Indicates that the MemoryPool is full and the transaction cannot be verified.
    OutOfMemory,

    /// Indicates that the previous block of the current block has not been received, so the block cannot be verified.
    UnableToVerify,

    /// Indicates that the IInventory is invalid.
    Invalid,

    /// Indicates that the Transaction has an invalid script.
    InvalidScript,

    /// Indicates that the Transaction has an invalid attribute.
    InvalidAttribute,

    /// Indicates that the IInventory has an invalid signature.
    InvalidSignature,

    /// Indicates that the size of the IInventory is not allowed.
    OverSize,

    /// Indicates that the Transaction has expired.
    Expired,

    /// Indicates that the Transaction failed to verify due to insufficient fees.
    InsufficientFunds,

    /// Indicates that the Transaction failed to verify because it didn't comply with the policy.
    PolicyFail,

    /// Indicates that the Transaction failed to verify because it conflicts with on-chain or mempooled transactions.
    HasConflicts,

    /// Indicates that the IInventory failed to verify due to other reasons.
    Unknown,
}
