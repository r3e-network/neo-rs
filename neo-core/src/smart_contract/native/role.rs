//! Role - matches C# Neo.SmartContract.Native.Role exactly

use neo_primitives::protocol_enum_repr;
use serde::{Deserialize, Serialize};

protocol_enum_repr! {
    /// Represents roles in the Neo network (matches C# Role enum)
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
    pub Role {
        /// State validator role
        StateValidator = 4,
        /// Oracle role
        Oracle = 8,
        /// NeoFS Alphabet Node role
        NeoFSAlphabetNode = 16,
        /// P2P Notary role (for NotaryAssisted transactions attribute)
        P2PNotary = 32,
    }
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

    /// Compatibility alias for callers that still use the older helper name.
    pub const fn from_u8(value: u8) -> Option<Self> {
        Self::from_byte(value)
    }

    /// Checks if the role is valid.
    #[must_use]
    pub const fn is_valid(&self) -> bool {
        matches!(
            self,
            Self::StateValidator | Self::Oracle | Self::NeoFSAlphabetNode | Self::P2PNotary
        )
    }

    /// Gets the name of the role.
    #[must_use]
    pub const fn name(&self) -> &'static str {
        self.as_str()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn role_values_match_protocol_bytes() {
        assert_eq!(Role::StateValidator.to_byte(), 4);
        assert_eq!(Role::Oracle.to_byte(), 8);
        assert_eq!(Role::NeoFSAlphabetNode.to_byte(), 16);
        assert_eq!(Role::P2PNotary.to_byte(), 32);
    }

    #[test]
    fn role_from_byte_rejects_unknown_values() {
        assert_eq!(Role::from_byte(4), Some(Role::StateValidator));
        assert_eq!(Role::from_u8(8), Some(Role::Oracle));
        assert_eq!(Role::from_byte(32), Some(Role::P2PNotary));
        assert_eq!(Role::from_byte(0), None);
        assert_eq!(Role::from_byte(255), None);
    }

    #[test]
    fn role_display_and_name_match_variant_names() {
        assert_eq!(Role::Oracle.name(), "Oracle");
        assert_eq!(Role::NeoFSAlphabetNode.to_string(), "NeoFSAlphabetNode");
    }

    #[test]
    fn role_serde_keeps_derived_enum_shape() {
        let serialized = serde_json::to_string(&Role::Oracle).unwrap();
        assert_eq!(serialized, "\"Oracle\"");

        let deserialized: Role = serde_json::from_str("\"StateValidator\"").unwrap();
        assert_eq!(deserialized, Role::StateValidator);
    }
}
