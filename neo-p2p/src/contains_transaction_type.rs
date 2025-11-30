//! ContainsTransactionType - matches C# Neo.Ledger.ContainsTransactionType exactly.

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;

/// Represents the type of transaction containment.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ContainsTransactionType {
    /// Transaction does not exist.
    NotExist = 0,
    /// Transaction exists in the memory pool.
    ExistsInPool = 1,
    /// Transaction exists in the ledger.
    ExistsInLedger = 2,
}

impl ContainsTransactionType {
    /// Converts to byte representation.
    pub fn to_byte(self) -> u8 {
        self as u8
    }

    /// Creates from byte representation.
    pub fn from_byte(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::NotExist),
            1 => Some(Self::ExistsInPool),
            2 => Some(Self::ExistsInLedger),
            _ => None,
        }
    }

    /// Returns the string representation.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NotExist => "NotExist",
            Self::ExistsInPool => "ExistsInPool",
            Self::ExistsInLedger => "ExistsInLedger",
        }
    }

    /// Returns true if the transaction exists (either in pool or ledger).
    pub fn exists(self) -> bool {
        !matches!(self, Self::NotExist)
    }
}

impl fmt::Display for ContainsTransactionType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl Serialize for ContainsTransactionType {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_u8(self.to_byte())
    }
}

impl<'de> Deserialize<'de> for ContainsTransactionType {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = u8::deserialize(deserializer)?;
        ContainsTransactionType::from_byte(value).ok_or_else(|| {
            serde::de::Error::custom(format!("Invalid ContainsTransactionType value: {value}"))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_contains_transaction_type_values() {
        assert_eq!(ContainsTransactionType::NotExist.to_byte(), 0);
        assert_eq!(ContainsTransactionType::ExistsInPool.to_byte(), 1);
        assert_eq!(ContainsTransactionType::ExistsInLedger.to_byte(), 2);
    }

    #[test]
    fn test_contains_transaction_type_from_byte() {
        assert_eq!(
            ContainsTransactionType::from_byte(0),
            Some(ContainsTransactionType::NotExist)
        );
        assert_eq!(
            ContainsTransactionType::from_byte(1),
            Some(ContainsTransactionType::ExistsInPool)
        );
        assert_eq!(
            ContainsTransactionType::from_byte(2),
            Some(ContainsTransactionType::ExistsInLedger)
        );
        assert_eq!(ContainsTransactionType::from_byte(3), None);
    }

    #[test]
    fn test_contains_transaction_type_roundtrip() {
        for ctt in [
            ContainsTransactionType::NotExist,
            ContainsTransactionType::ExistsInPool,
            ContainsTransactionType::ExistsInLedger,
        ] {
            let byte = ctt.to_byte();
            let recovered = ContainsTransactionType::from_byte(byte);
            assert_eq!(recovered, Some(ctt));
        }
    }

    #[test]
    fn test_contains_transaction_type_display() {
        assert_eq!(ContainsTransactionType::NotExist.to_string(), "NotExist");
        assert_eq!(
            ContainsTransactionType::ExistsInPool.to_string(),
            "ExistsInPool"
        );
        assert_eq!(
            ContainsTransactionType::ExistsInLedger.to_string(),
            "ExistsInLedger"
        );
    }

    #[test]
    fn test_contains_transaction_type_exists() {
        assert!(!ContainsTransactionType::NotExist.exists());
        assert!(ContainsTransactionType::ExistsInPool.exists());
        assert!(ContainsTransactionType::ExistsInLedger.exists());
    }

    #[test]
    fn test_contains_transaction_type_serde() {
        let ctt = ContainsTransactionType::ExistsInPool;
        let serialized = serde_json::to_string(&ctt).unwrap();
        assert_eq!(serialized, "1");

        let deserialized: ContainsTransactionType = serde_json::from_str(&serialized).unwrap();
        assert_eq!(deserialized, ctt);
    }
}
