use std::convert::TryFrom;

/// Indicates the action to be taken if the current context meets with the rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WitnessRuleAction {
    /// Deny the witness according to the rule.
    Deny = 0,

    /// Allow the witness according to the rule.
    Allow = 1,
}

impl TryFrom<u8> for WitnessRuleAction {
    type Error = ();

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(WitnessRuleAction::Deny),
            1 => Ok(WitnessRuleAction::Allow),
            _ => Err(()),
        }
    }
}

impl From<WitnessRuleAction> for u8 {
    fn from(action: WitnessRuleAction) -> Self {
        action as u8
    }
}
