//! TransactionAttributeType - matches C# Neo.Network.P2P.Payloads.TransactionAttributeType exactly.

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;

/// Represents the type of a TransactionAttribute.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum TransactionAttributeType {
    /// Indicates that the transaction is of high priority.
    HighPriority = 0x01,
    /// Indicates that the transaction is an oracle response.
    OracleResponse = 0x11,
    /// Indicates that the transaction is not valid before the specified block height.
    NotValidBefore = 0x20,
    /// Indicates that the transaction conflicts with the specified transaction.
    Conflicts = 0x21,
    /// Indicates that the transaction is notary assisted.
    NotaryAssisted = 0x22,
}

impl TransactionAttributeType {
    /// Converts to byte representation.
    pub fn to_byte(self) -> u8 {
        self as u8
    }

    /// Creates from byte representation.
    pub fn from_byte(value: u8) -> Option<Self> {
        match value {
            0x01 => Some(Self::HighPriority),
            0x11 => Some(Self::OracleResponse),
            0x20 => Some(Self::NotValidBefore),
            0x21 => Some(Self::Conflicts),
            0x22 => Some(Self::NotaryAssisted),
            _ => None,
        }
    }

    /// Returns the string representation.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::HighPriority => "HighPriority",
            Self::OracleResponse => "OracleResponse",
            Self::NotValidBefore => "NotValidBefore",
            Self::Conflicts => "Conflicts",
            Self::NotaryAssisted => "NotaryAssisted",
        }
    }

    /// Returns true if this attribute type allows multiple instances per transaction.
    pub fn allows_multiple(self) -> bool {
        matches!(self, Self::Conflicts | Self::NotaryAssisted)
    }
}

impl fmt::Display for TransactionAttributeType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl Serialize for TransactionAttributeType {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_u8(self.to_byte())
    }
}

impl<'de> Deserialize<'de> for TransactionAttributeType {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let byte = u8::deserialize(deserializer)?;
        TransactionAttributeType::from_byte(byte).ok_or_else(|| {
            serde::de::Error::custom(format!("Invalid transaction attribute type byte: {byte}"))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_transaction_attribute_type_values() {
        assert_eq!(TransactionAttributeType::HighPriority.to_byte(), 0x01);
        assert_eq!(TransactionAttributeType::OracleResponse.to_byte(), 0x11);
        assert_eq!(TransactionAttributeType::NotValidBefore.to_byte(), 0x20);
        assert_eq!(TransactionAttributeType::Conflicts.to_byte(), 0x21);
        assert_eq!(TransactionAttributeType::NotaryAssisted.to_byte(), 0x22);
    }

    #[test]
    fn test_transaction_attribute_type_from_byte() {
        assert_eq!(
            TransactionAttributeType::from_byte(0x01),
            Some(TransactionAttributeType::HighPriority)
        );
        assert_eq!(
            TransactionAttributeType::from_byte(0x11),
            Some(TransactionAttributeType::OracleResponse)
        );
        assert_eq!(
            TransactionAttributeType::from_byte(0x20),
            Some(TransactionAttributeType::NotValidBefore)
        );
        assert_eq!(
            TransactionAttributeType::from_byte(0x21),
            Some(TransactionAttributeType::Conflicts)
        );
        assert_eq!(
            TransactionAttributeType::from_byte(0x22),
            Some(TransactionAttributeType::NotaryAssisted)
        );
        assert_eq!(TransactionAttributeType::from_byte(0xFF), None);
    }

    #[test]
    fn test_transaction_attribute_type_roundtrip() {
        for attr_type in [
            TransactionAttributeType::HighPriority,
            TransactionAttributeType::OracleResponse,
            TransactionAttributeType::NotValidBefore,
            TransactionAttributeType::Conflicts,
            TransactionAttributeType::NotaryAssisted,
        ] {
            let byte = attr_type.to_byte();
            let recovered = TransactionAttributeType::from_byte(byte);
            assert_eq!(recovered, Some(attr_type));
        }
    }

    #[test]
    fn test_transaction_attribute_type_display() {
        assert_eq!(
            TransactionAttributeType::HighPriority.to_string(),
            "HighPriority"
        );
        assert_eq!(
            TransactionAttributeType::OracleResponse.to_string(),
            "OracleResponse"
        );
        assert_eq!(
            TransactionAttributeType::NotValidBefore.to_string(),
            "NotValidBefore"
        );
        assert_eq!(TransactionAttributeType::Conflicts.to_string(), "Conflicts");
        assert_eq!(
            TransactionAttributeType::NotaryAssisted.to_string(),
            "NotaryAssisted"
        );
    }

    #[test]
    fn test_transaction_attribute_type_allows_multiple() {
        assert!(!TransactionAttributeType::HighPriority.allows_multiple());
        assert!(!TransactionAttributeType::OracleResponse.allows_multiple());
        assert!(!TransactionAttributeType::NotValidBefore.allows_multiple());
        assert!(TransactionAttributeType::Conflicts.allows_multiple());
        assert!(TransactionAttributeType::NotaryAssisted.allows_multiple());
    }

    #[test]
    fn test_transaction_attribute_type_serde() {
        let attr_type = TransactionAttributeType::OracleResponse;
        let serialized = serde_json::to_string(&attr_type).unwrap();
        assert_eq!(serialized, "17"); // 0x11 = 17

        let deserialized: TransactionAttributeType = serde_json::from_str(&serialized).unwrap();
        assert_eq!(deserialized, attr_type);
    }
}
