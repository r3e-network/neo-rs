//! `VerifyResult` - matches C# Neo.Ledger.VerifyResult exactly.
//!
//! This is the single source of truth for `VerifyResult` enum. Both `neo-core::ledger`
//! and neo-p2p re-export this type for backward compatibility.

use crate::protocol_enum;

protocol_enum! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
    /// Represents a verifying result of `IInventory`.
    pub VerifyResult {
        Succeed = 0,
        AlreadyExists = 1,
        AlreadyInPool = 2,
        OutOfMemory = 3,
        UnableToVerify = 4,
        Invalid = 5,
        InvalidScript = 6,
        InvalidAttribute = 7,
        InvalidSignature = 8,
        OverSize = 9,
        Expired = 10,
        InsufficientFunds = 11,
        PolicyFail = 12,
        HasConflicts = 13,
        #[default]
        Unknown = 14,
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
    fn protocol_enum_guard_rejects_unknown_verify_result_serde_bytes() {
        assert_eq!(
            serde_json::from_str::<VerifyResult>("14").unwrap(),
            VerifyResult::Unknown
        );
        assert!(serde_json::from_str::<VerifyResult>("15").is_err());
        assert!(serde_json::from_str::<VerifyResult>("255").is_err());
    }

    #[test]
    fn test_verify_result_default() {
        assert_eq!(VerifyResult::default(), VerifyResult::Unknown);
    }
}
