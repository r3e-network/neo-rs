//! WitnessRuleAction - matches C# Neo.Network.P2P.Payloads.WitnessRuleAction exactly.

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;
use std::str::FromStr;

/// The action to be taken if the current context meets with the rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum WitnessRuleAction {
    /// Deny the witness if the condition is met.
    Deny = 0,
    /// Allow the witness if the condition is met.
    Allow = 1,
}

impl WitnessRuleAction {
    /// Converts to byte representation.
    pub fn to_byte(self) -> u8 {
        self as u8
    }

    /// Creates from byte representation.
    pub fn from_byte(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::Deny),
            1 => Some(Self::Allow),
            _ => None,
        }
    }

    /// Returns the string representation.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Deny => "Deny",
            Self::Allow => "Allow",
        }
    }
}

impl Default for WitnessRuleAction {
    fn default() -> Self {
        Self::Deny
    }
}

impl fmt::Display for WitnessRuleAction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
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

impl Serialize for WitnessRuleAction {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_u8(self.to_byte())
    }
}

impl<'de> Deserialize<'de> for WitnessRuleAction {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let byte = u8::deserialize(deserializer)?;
        WitnessRuleAction::from_byte(byte).ok_or_else(|| {
            serde::de::Error::custom(format!("Invalid witness rule action byte: {byte}"))
        })
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
