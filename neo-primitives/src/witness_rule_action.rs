//! WitnessRuleAction — matches C# Neo.Network.P2P.Payloads.WitnessRuleAction exactly.

use crate::protocol_enum;

protocol_enum! {
    all;
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

impl std::str::FromStr for WitnessRuleAction {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        for candidate in Self::ALL {
            if value == candidate.as_str() {
                return Ok(candidate);
            }
        }
        Err(format!("Invalid witness rule action: {value}"))
    }
}

#[cfg(test)]
#[path = "tests/witness_rule_action.rs"]
mod tests;
