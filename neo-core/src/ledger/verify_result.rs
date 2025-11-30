//! Verify result implementation.
//!
//! This module provides the VerifyResult functionality exactly matching C# Neo VerifyResult.

// Matches C# using directives exactly:
// using Neo.Network.P2P.Payloads;

/// namespace Neo.Ledger -> public enum VerifyResult : byte
use serde::{Deserialize, Deserializer, Serialize, Serializer};

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

impl Serialize for VerifyResult {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_u8(*self as u8)
    }
}

impl<'de> Deserialize<'de> for VerifyResult {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = u8::deserialize(deserializer)?;
        match value {
            0 => Ok(VerifyResult::Succeed),
            1 => Ok(VerifyResult::AlreadyExists),
            2 => Ok(VerifyResult::AlreadyInPool),
            3 => Ok(VerifyResult::OutOfMemory),
            4 => Ok(VerifyResult::UnableToVerify),
            5 => Ok(VerifyResult::Invalid),
            6 => Ok(VerifyResult::InvalidScript),
            7 => Ok(VerifyResult::InvalidAttribute),
            8 => Ok(VerifyResult::InvalidSignature),
            9 => Ok(VerifyResult::OverSize),
            10 => Ok(VerifyResult::Expired),
            11 => Ok(VerifyResult::InsufficientFunds),
            12 => Ok(VerifyResult::PolicyFail),
            13 => Ok(VerifyResult::HasConflicts),
            14 => Ok(VerifyResult::Unknown),
            other => Err(serde::de::Error::custom(format!(
                "Invalid VerifyResult value: {other}"
            ))),
        }
    }
}
