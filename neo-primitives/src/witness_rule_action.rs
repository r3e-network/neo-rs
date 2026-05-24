//! WitnessRuleAction — matches C# Neo.Network.P2P.Payloads.WitnessRuleAction exactly.

use crate::protocol_enum;
use std::str::FromStr;

protocol_enum! {
    /// The action to be taken if the current context meets with the rule.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
    pub WitnessRuleAction {
        /// Deny the witness if the condition is met.
        #[default]
        Deny = 0,
        /// Allow the witness if the condition is met.
        Allow = 1,
    }
}

impl FromStr for WitnessRuleAction {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "deny" => Ok(Self::Deny),
            "allow" => Ok(Self::Allow),
            other => Err(format!("Invalid witness rule action: {other}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_witness_rule_action_values() {
        assert_eq!(WitnessRuleAction::Deny.to_byte(), 0);
        assert_eq!(WitnessRuleAction::Allow.to_byte(), 1);
    }

    #[test]
    fn test_witness_rule_action_from_byte() {
        assert_eq!(
            WitnessRuleAction::from_byte(0),
            Some(WitnessRuleAction::Deny)
        );
        assert_eq!(
            WitnessRuleAction::from_byte(1),
            Some(WitnessRuleAction::Allow)
        );
        assert_eq!(WitnessRuleAction::from_byte(2), None);
    }

    #[test]
    fn protocol_enum_guard_rejects_unknown_witness_rule_action_bytes() {
        assert_eq!(WitnessRuleAction::from_byte(2), None);
        assert_eq!(WitnessRuleAction::from_byte(255), None);
        assert!(serde_json::from_str::<WitnessRuleAction>("2").is_err());
        assert!(serde_json::from_str::<WitnessRuleAction>("255").is_err());
    }

    #[test]
    fn test_witness_rule_action_from_str() {
        assert_eq!(
            WitnessRuleAction::from_str("Deny").unwrap(),
            WitnessRuleAction::Deny
        );
        assert_eq!(
            WitnessRuleAction::from_str("Allow").unwrap(),
            WitnessRuleAction::Allow
        );
        assert_eq!(
            WitnessRuleAction::from_str("allow").unwrap(),
            WitnessRuleAction::Allow
        );
        assert!(WitnessRuleAction::from_str("Invalid").is_err());
    }

    #[test]
    fn test_witness_rule_action_display() {
        assert_eq!(WitnessRuleAction::Deny.to_string(), "Deny");
        assert_eq!(WitnessRuleAction::Allow.to_string(), "Allow");
    }

    #[test]
    fn test_witness_rule_action_default() {
        assert_eq!(WitnessRuleAction::default(), WitnessRuleAction::Deny);
    }
}
