//! `WitnessConditionType` - matches C# Neo.Network.P2P.Payloads.WitnessConditionType exactly.
//!
//! This is the single source of truth for witness condition protocol bytes. Both
//! `neo-core::witness_rule` and `neo-p2p` re-export this type for backward compatibility.

use crate::protocol_enum;

protocol_enum! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    /// The type of witness condition.
    pub WitnessConditionType {
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn witness_condition_type_values_match_protocol_bytes() {
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
    fn witness_condition_type_from_byte_rejects_unknown_values() {
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
        assert_eq!(WitnessConditionType::from_byte(0x04), None);
        assert_eq!(WitnessConditionType::from_byte(0x1a), None);
        assert_eq!(WitnessConditionType::from_byte(0xff), None);
    }

    #[test]
    fn witness_condition_type_roundtrips_protocol_bytes() {
        for condition_type in [
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
            assert_eq!(
                WitnessConditionType::from_byte(condition_type.to_byte()),
                Some(condition_type)
            );
        }
    }

    #[test]
    fn witness_condition_type_display_matches_variant_names() {
        assert_eq!(WitnessConditionType::Boolean.to_string(), "Boolean");
        assert_eq!(WitnessConditionType::ScriptHash.to_string(), "ScriptHash");
        assert_eq!(
            WitnessConditionType::CalledByEntry.to_string(),
            "CalledByEntry"
        );
    }

    #[test]
    fn witness_condition_type_serde_rejects_unknown_values() {
        assert!(serde_json::from_str::<WitnessConditionType>("4").is_err());
        assert!(serde_json::from_str::<WitnessConditionType>("26").is_err());
        assert!(serde_json::from_str::<WitnessConditionType>("255").is_err());
    }
}
