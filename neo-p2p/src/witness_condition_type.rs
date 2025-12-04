//! WitnessConditionType - matches C# Neo.Network.P2P.Payloads.WitnessConditionType exactly.

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;

/// The type of witness condition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum WitnessConditionType {
    /// Boolean condition.
    Boolean = 0x00,
    /// Not condition (logical NOT).
    Not = 0x01,
    /// And condition (logical AND).
    And = 0x02,
    /// Or condition (logical OR).
    Or = 0x03,
    /// Script hash condition.
    ScriptHash = 0x18,
    /// Group condition.
    Group = 0x19,
    /// Called by entry condition.
    CalledByEntry = 0x20,
    /// Called by contract condition.
    CalledByContract = 0x28,
    /// Called by group condition.
    CalledByGroup = 0x29,
}

impl WitnessConditionType {
    /// Converts to byte representation.
    pub fn to_byte(self) -> u8 {
        self as u8
    }

    /// Creates from byte representation.
    pub fn from_byte(value: u8) -> Option<Self> {
        match value {
            0x00 => Some(Self::Boolean),
            0x01 => Some(Self::Not),
            0x02 => Some(Self::And),
            0x03 => Some(Self::Or),
            0x18 => Some(Self::ScriptHash),
            0x19 => Some(Self::Group),
            0x20 => Some(Self::CalledByEntry),
            0x28 => Some(Self::CalledByContract),
            0x29 => Some(Self::CalledByGroup),
            _ => None,
        }
    }

    /// Returns the string representation.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Boolean => "Boolean",
            Self::Not => "Not",
            Self::And => "And",
            Self::Or => "Or",
            Self::ScriptHash => "ScriptHash",
            Self::Group => "Group",
            Self::CalledByEntry => "CalledByEntry",
            Self::CalledByContract => "CalledByContract",
            Self::CalledByGroup => "CalledByGroup",
        }
    }
}

impl fmt::Display for WitnessConditionType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl Serialize for WitnessConditionType {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_u8(self.to_byte())
    }
}

impl<'de> Deserialize<'de> for WitnessConditionType {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let byte = u8::deserialize(deserializer)?;
        WitnessConditionType::from_byte(byte).ok_or_else(|| {
            serde::de::Error::custom(format!("Invalid witness condition type byte: {byte}"))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_witness_condition_type_values() {
        assert_eq!(WitnessConditionType::Boolean.to_byte(), 0x00);
        assert_eq!(WitnessConditionType::Not.to_byte(), 0x01);
        assert_eq!(WitnessConditionType::And.to_byte(), 0x02);
        assert_eq!(WitnessConditionType::Or.to_byte(), 0x03);
        assert_eq!(WitnessConditionType::ScriptHash.to_byte(), 0x18);
        assert_eq!(WitnessConditionType::Group.to_byte(), 0x19);
        assert_eq!(WitnessConditionType::CalledByEntry.to_byte(), 0x20);
        assert_eq!(WitnessConditionType::CalledByContract.to_byte(), 0x28);
        assert_eq!(WitnessConditionType::CalledByGroup.to_byte(), 0x29);
    }

    #[test]
    fn test_witness_condition_type_from_byte() {
        assert_eq!(
            WitnessConditionType::from_byte(0x00),
            Some(WitnessConditionType::Boolean)
        );
        assert_eq!(
            WitnessConditionType::from_byte(0x01),
            Some(WitnessConditionType::Not)
        );
        assert_eq!(
            WitnessConditionType::from_byte(0x18),
            Some(WitnessConditionType::ScriptHash)
        );
        assert_eq!(
            WitnessConditionType::from_byte(0x20),
            Some(WitnessConditionType::CalledByEntry)
        );
        assert_eq!(WitnessConditionType::from_byte(0xFF), None);
    }

    #[test]
    fn test_witness_condition_type_roundtrip() {
        for cond_type in [
            WitnessConditionType::Boolean,
            WitnessConditionType::Not,
            WitnessConditionType::And,
            WitnessConditionType::Or,
            WitnessConditionType::ScriptHash,
            WitnessConditionType::Group,
            WitnessConditionType::CalledByEntry,
            WitnessConditionType::CalledByContract,
            WitnessConditionType::CalledByGroup,
        ] {
            let byte = cond_type.to_byte();
            let recovered = WitnessConditionType::from_byte(byte);
            assert_eq!(recovered, Some(cond_type));
        }
    }

    #[test]
    fn test_witness_condition_type_display() {
        assert_eq!(WitnessConditionType::Boolean.to_string(), "Boolean");
        assert_eq!(WitnessConditionType::ScriptHash.to_string(), "ScriptHash");
        assert_eq!(
            WitnessConditionType::CalledByEntry.to_string(),
            "CalledByEntry"
        );
    }
}
