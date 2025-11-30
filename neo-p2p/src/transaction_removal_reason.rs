//! TransactionRemovalReason - matches C# Neo.Ledger.TransactionRemovalReason exactly.

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;

/// The reason a transaction was removed from the memory pool.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TransactionRemovalReason {
    /// The transaction was rejected since it was the lowest priority transaction
    /// and the memory pool capacity was exceeded.
    CapacityExceeded = 0,
    /// The transaction was rejected due to failing re-validation after a block was persisted.
    NoLongerValid = 1,
    /// The transaction was rejected due to conflict with higher priority transactions
    /// with Conflicts attribute.
    Conflict = 2,
}

impl TransactionRemovalReason {
    /// Converts to byte representation.
    pub fn to_byte(self) -> u8 {
        self as u8
    }

    /// Creates from byte representation.
    pub fn from_byte(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::CapacityExceeded),
            1 => Some(Self::NoLongerValid),
            2 => Some(Self::Conflict),
            _ => None,
        }
    }

    /// Returns the string representation.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::CapacityExceeded => "CapacityExceeded",
            Self::NoLongerValid => "NoLongerValid",
            Self::Conflict => "Conflict",
        }
    }
}

impl fmt::Display for TransactionRemovalReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl Serialize for TransactionRemovalReason {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_u8(self.to_byte())
    }
}

impl<'de> Deserialize<'de> for TransactionRemovalReason {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = u8::deserialize(deserializer)?;
        TransactionRemovalReason::from_byte(value).ok_or_else(|| {
            serde::de::Error::custom(format!("Invalid TransactionRemovalReason value: {value}"))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_transaction_removal_reason_values() {
        assert_eq!(TransactionRemovalReason::CapacityExceeded.to_byte(), 0);
        assert_eq!(TransactionRemovalReason::NoLongerValid.to_byte(), 1);
        assert_eq!(TransactionRemovalReason::Conflict.to_byte(), 2);
    }

    #[test]
    fn test_transaction_removal_reason_from_byte() {
        assert_eq!(
            TransactionRemovalReason::from_byte(0),
            Some(TransactionRemovalReason::CapacityExceeded)
        );
        assert_eq!(
            TransactionRemovalReason::from_byte(1),
            Some(TransactionRemovalReason::NoLongerValid)
        );
        assert_eq!(
            TransactionRemovalReason::from_byte(2),
            Some(TransactionRemovalReason::Conflict)
        );
        assert_eq!(TransactionRemovalReason::from_byte(3), None);
    }

    #[test]
    fn test_transaction_removal_reason_roundtrip() {
        for reason in [
            TransactionRemovalReason::CapacityExceeded,
            TransactionRemovalReason::NoLongerValid,
            TransactionRemovalReason::Conflict,
        ] {
            let byte = reason.to_byte();
            let recovered = TransactionRemovalReason::from_byte(byte);
            assert_eq!(recovered, Some(reason));
        }
    }

    #[test]
    fn test_transaction_removal_reason_display() {
        assert_eq!(
            TransactionRemovalReason::CapacityExceeded.to_string(),
            "CapacityExceeded"
        );
        assert_eq!(
            TransactionRemovalReason::NoLongerValid.to_string(),
            "NoLongerValid"
        );
        assert_eq!(TransactionRemovalReason::Conflict.to_string(), "Conflict");
    }

    #[test]
    fn test_transaction_removal_reason_serde() {
        let reason = TransactionRemovalReason::NoLongerValid;
        let serialized = serde_json::to_string(&reason).unwrap();
        assert_eq!(serialized, "1");

        let deserialized: TransactionRemovalReason = serde_json::from_str(&serialized).unwrap();
        assert_eq!(deserialized, reason);
    }
}
