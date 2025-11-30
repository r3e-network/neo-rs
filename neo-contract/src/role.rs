//! Role - matches C# Neo.SmartContract.Native.Role exactly.

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;

/// Represents roles in the Neo network (matches C# Role enum).
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Role {
    /// State validator role.
    StateValidator = 4,
    /// Oracle role.
    Oracle = 8,
    /// NeoFS Alphabet Node role.
    NeoFSAlphabetNode = 16,
    /// P2P Notary role (for NotaryAssisted transactions attribute).
    P2PNotary = 32,
}

impl Role {
    const VALUES: [Role; 4] = [
        Role::StateValidator,
        Role::Oracle,
        Role::NeoFSAlphabetNode,
        Role::P2PNotary,
    ];

    /// Returns the static list of all roles.
    pub fn all() -> &'static [Role] {
        &Self::VALUES
    }

    /// Converts to byte representation.
    pub fn to_byte(self) -> u8 {
        self as u8
    }

    /// Creates a role from the provided numeric value.
    pub fn from_byte(value: u8) -> Option<Self> {
        match value {
            4 => Some(Role::StateValidator),
            8 => Some(Role::Oracle),
            16 => Some(Role::NeoFSAlphabetNode),
            32 => Some(Role::P2PNotary),
            _ => None,
        }
    }

    /// Gets the name of the role.
    pub fn as_str(self) -> &'static str {
        match self {
            Role::StateValidator => "StateValidator",
            Role::Oracle => "Oracle",
            Role::NeoFSAlphabetNode => "NeoFSAlphabetNode",
            Role::P2PNotary => "P2PNotary",
        }
    }
}

impl fmt::Display for Role {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl Serialize for Role {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_u8(self.to_byte())
    }
}

impl<'de> Deserialize<'de> for Role {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let byte = u8::deserialize(deserializer)?;
        Role::from_byte(byte)
            .ok_or_else(|| serde::de::Error::custom(format!("Invalid role byte: {byte}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_role_values() {
        assert_eq!(Role::StateValidator.to_byte(), 4);
        assert_eq!(Role::Oracle.to_byte(), 8);
        assert_eq!(Role::NeoFSAlphabetNode.to_byte(), 16);
        assert_eq!(Role::P2PNotary.to_byte(), 32);
    }

    #[test]
    fn test_role_from_byte() {
        assert_eq!(Role::from_byte(4), Some(Role::StateValidator));
        assert_eq!(Role::from_byte(8), Some(Role::Oracle));
        assert_eq!(Role::from_byte(16), Some(Role::NeoFSAlphabetNode));
        assert_eq!(Role::from_byte(32), Some(Role::P2PNotary));
        assert_eq!(Role::from_byte(0), None);
        assert_eq!(Role::from_byte(255), None);
    }

    #[test]
    fn test_role_roundtrip() {
        for role in Role::all() {
            let byte = role.to_byte();
            let recovered = Role::from_byte(byte);
            assert_eq!(recovered, Some(*role));
        }
    }

    #[test]
    fn test_role_display() {
        assert_eq!(Role::StateValidator.to_string(), "StateValidator");
        assert_eq!(Role::Oracle.to_string(), "Oracle");
        assert_eq!(Role::NeoFSAlphabetNode.to_string(), "NeoFSAlphabetNode");
        assert_eq!(Role::P2PNotary.to_string(), "P2PNotary");
    }

    #[test]
    fn test_role_all() {
        let all = Role::all();
        assert_eq!(all.len(), 4);
        assert!(all.contains(&Role::StateValidator));
        assert!(all.contains(&Role::Oracle));
        assert!(all.contains(&Role::NeoFSAlphabetNode));
        assert!(all.contains(&Role::P2PNotary));
    }
}
