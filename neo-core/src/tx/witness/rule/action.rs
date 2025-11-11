use serde::{Deserialize, Serialize};

use neo_base::encoding::DecodeError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
pub enum Action {
    Deny = 0x00,
    Allow = 0x01,
}

impl TryFrom<u8> for Action {
    type Error = DecodeError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0x00 => Ok(Action::Deny),
            0x01 => Ok(Action::Allow),
            _ => Err(DecodeError::InvalidValue("WitnessRuleAction")),
        }
    }
}
