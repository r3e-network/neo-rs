//! `VerifyResult` - matches C# Neo.Ledger.VerifyResult exactly.
//!
//! This is the single source of truth for `VerifyResult` enum. Both `neo-core::ledger`
//! and neo-p2p re-export this type for backward compatibility.

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;

/// Represents a verifying result of `IInventory`.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum VerifyResult {
    /// Indicates that the verification was successful.
    Succeed = 0,
    /// Indicates that an `IInventory` with the same hash already exists.
    AlreadyExists = 1,
    /// Indicates that an `IInventory` with the same hash already exists in the memory pool.
    AlreadyInPool = 2,
    /// Indicates that the `MemoryPool` is full and the transaction cannot be verified.
    OutOfMemory = 3,
    /// Indicates that the previous block of the current block has not been received.
    UnableToVerify = 4,
    /// Indicates that the `IInventory` is invalid.
    Invalid = 5,
    /// Indicates that the Transaction has an invalid script.
    InvalidScript = 6,
    /// Indicates that the Transaction has an invalid attribute.
    InvalidAttribute = 7,
    /// Indicates that the `IInventory` has an invalid signature.
    InvalidSignature = 8,
    /// Indicates that the size of the `IInventory` is not allowed.
    OverSize = 9,
    /// Indicates that the Transaction has expired.
    Expired = 10,
    /// Indicates that the Transaction failed to verify due to insufficient fees.
    InsufficientFunds = 11,
    /// Indicates that the Transaction failed to verify because it didn't comply with the policy.
    PolicyFail = 12,
    /// Indicates that the Transaction failed to verify because it conflicts with on-chain or mempooled transactions.
    HasConflicts = 13,
    /// Indicates that the `IInventory` failed to verify due to other reasons.
    #[default]
    Unknown = 14,
}

impl VerifyResult {
    /// Converts to byte representation.
    #[inline]
    #[must_use]
    pub const fn to_byte(self) -> u8 {
        self as u8
    }

    /// Creates from byte representation.
    #[must_use]
    pub const fn from_byte(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::Succeed),
            1 => Some(Self::AlreadyExists),
            2 => Some(Self::AlreadyInPool),
            3 => Some(Self::OutOfMemory),
            4 => Some(Self::UnableToVerify),
            5 => Some(Self::Invalid),
            6 => Some(Self::InvalidScript),
            7 => Some(Self::InvalidAttribute),
            8 => Some(Self::InvalidSignature),
            9 => Some(Self::OverSize),
            10 => Some(Self::Expired),
            11 => Some(Self::InsufficientFunds),
            12 => Some(Self::PolicyFail),
            13 => Some(Self::HasConflicts),
            14 => Some(Self::Unknown),
            _ => None,
        }
    }

    /// Returns the string representation.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Succeed => "Succeed",
            Self::AlreadyExists => "AlreadyExists",
            Self::AlreadyInPool => "AlreadyInPool",
            Self::OutOfMemory => "OutOfMemory",
            Self::UnableToVerify => "UnableToVerify",
            Self::Invalid => "Invalid",
            Self::InvalidScript => "InvalidScript",
            Self::InvalidAttribute => "InvalidAttribute",
            Self::InvalidSignature => "InvalidSignature",
            Self::OverSize => "OverSize",
            Self::Expired => "Expired",
            Self::InsufficientFunds => "InsufficientFunds",
            Self::PolicyFail => "PolicyFail",
            Self::HasConflicts => "HasConflicts",
            Self::Unknown => "Unknown",
        }
    }

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

impl fmt::Display for VerifyResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl Serialize for VerifyResult {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_u8(self.to_byte())
    }
}

impl<'de> Deserialize<'de> for VerifyResult {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = u8::deserialize(deserializer)?;
        Self::from_byte(value)
            .ok_or_else(|| serde::de::Error::custom(format!("Invalid VerifyResult value: {value}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_verify_result_values() {
        assert_eq!(VerifyResult::Succeed.to_byte(), 0);
        assert_eq!(VerifyResult::AlreadyExists.to_byte(), 1);
        assert_eq!(VerifyResult::AlreadyInPool.to_byte(), 2);
        assert_eq!(VerifyResult::OutOfMemory.to_byte(), 3);
        assert_eq!(VerifyResult::UnableToVerify.to_byte(), 4);
        assert_eq!(VerifyResult::Invalid.to_byte(), 5);
        assert_eq!(VerifyResult::InvalidScript.to_byte(), 6);
        assert_eq!(VerifyResult::InvalidAttribute.to_byte(), 7);
        assert_eq!(VerifyResult::InvalidSignature.to_byte(), 8);
        assert_eq!(VerifyResult::OverSize.to_byte(), 9);
        assert_eq!(VerifyResult::Expired.to_byte(), 10);
        assert_eq!(VerifyResult::InsufficientFunds.to_byte(), 11);
        assert_eq!(VerifyResult::PolicyFail.to_byte(), 12);
        assert_eq!(VerifyResult::HasConflicts.to_byte(), 13);
        assert_eq!(VerifyResult::Unknown.to_byte(), 14);
    }

    #[test]
    fn test_verify_result_from_byte() {
        assert_eq!(VerifyResult::from_byte(0), Some(VerifyResult::Succeed));
        assert_eq!(VerifyResult::from_byte(5), Some(VerifyResult::Invalid));
        assert_eq!(VerifyResult::from_byte(14), Some(VerifyResult::Unknown));
        assert_eq!(VerifyResult::from_byte(15), None);
        assert_eq!(VerifyResult::from_byte(255), None);
    }

    #[test]
    fn test_verify_result_roundtrip() {
        for i in 0..=14u8 {
            let result = VerifyResult::from_byte(i).unwrap();
            assert_eq!(result.to_byte(), i);
        }
    }

    #[test]
    fn test_verify_result_display() {
        assert_eq!(VerifyResult::Succeed.to_string(), "Succeed");
        assert_eq!(VerifyResult::Invalid.to_string(), "Invalid");
        assert_eq!(VerifyResult::PolicyFail.to_string(), "PolicyFail");
    }

    #[test]
    fn test_verify_result_is_success() {
        assert!(VerifyResult::Succeed.is_success());
        assert!(!VerifyResult::Invalid.is_success());
        assert!(!VerifyResult::Unknown.is_success());
    }

    #[test]
    fn test_verify_result_is_failure() {
        assert!(!VerifyResult::Succeed.is_failure());
        assert!(VerifyResult::Invalid.is_failure());
        assert!(VerifyResult::PolicyFail.is_failure());
    }

    #[test]
    fn test_verify_result_serde() {
        let result = VerifyResult::InvalidScript;
        let serialized = serde_json::to_string(&result).unwrap();
        assert_eq!(serialized, "6");

        let deserialized: VerifyResult = serde_json::from_str(&serialized).unwrap();
        assert_eq!(deserialized, result);
    }

    #[test]
    fn test_verify_result_default() {
        assert_eq!(VerifyResult::default(), VerifyResult::Unknown);
    }
}
